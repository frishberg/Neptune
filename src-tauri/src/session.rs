// The one state machine. Hotkeys, the pill and the main window send Inputs
// here; this decides what happens, tells the pill what to show, and writes
// every dictation to history.
//
// Speed comes from chunking: while you talk, the audio so far is cut at
// pauses into ~20 s pieces and handed to a worker thread that runs the model
// as you go. When you let go only the tail is left, so the wait is the cost
// of one short chunk instead of the whole take. The worker owns the model;
// this thread never blocks on inference, so the pill keeps moving.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use crate::audio::Recorder;
use crate::focus;
use crate::history::{self, Entry};
use crate::overlay::{self, State};
use crate::settings;
use crate::transcribe::{Engine, Parakeet};
use crate::{debug, hotkeys, paste};

/// On while the onboarding's "Try it" step is showing: a take aimed at
/// neptune's own window then lands in that step instead of offering Copy.
static TRY_MODE: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub fn set_try_mode(on: bool) {
    TRY_MODE.store(on, Ordering::Relaxed);
}

/// The shortcut pressed while neptune's own window has the keyboard: the
/// window reports it ("down", "up", "handsfree") and the take runs exactly as
/// if the hook had seen it.
#[tauri::command]
pub fn window_key(app: AppHandle, kind: String) {
    debug::log(&format!("window key {kind}"));
    match kind.as_str() {
        "down" => send(&app, Input::PttDown),
        "up" => send(&app, Input::PttUp),
        "handsfree" => send(&app, Input::HandsfreeToggle),
        _ => {}
    }
}

#[derive(Debug, Clone)]
pub enum Input {
    PttDown,
    PttUp,
    HandsfreeToggle,
    Cancel,
    PasteLast,
    PillClick,
    OpenHistory,
    /// Re-run the model on a history entry's saved audio. Replies with the
    /// new cleaned text.
    Retry { id: u64, reply: Sender<Result<String, String>> },
    /// Dev only: run a 16 kHz WAV through the whole pipeline as if it had
    /// just been spoken. Set BLURT_DICTATE=<file.wav> in a debug build.
    DictateFile(std::path::PathBuf),
    /// The model on disk changed; drop the loaded one.
    ReloadModel,
}

/// Inputs compare by kind only; the payloads (reply channels, paths) are
/// not part of what a test needs to say about them.
impl PartialEq for Input {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

struct Mailbox(Sender<Input>);

pub fn send(app: &AppHandle, input: Input) {
    if let Some(m) = app.try_state::<Mailbox>() {
        let _ = m.0.send(input);
    }
}

#[tauri::command]
pub fn pill_clicked(app: AppHandle) {
    send(&app, Input::PillClick);
}

#[tauri::command]
pub fn paste_last(app: AppHandle) {
    send(&app, Input::PasteLast);
}

#[tauri::command]
pub fn cancel(app: AppHandle) {
    send(&app, Input::Cancel);
}

/// The last transcript to the clipboard; the pill offers this when a paste
/// could not land.
#[tauri::command]
pub fn copy_last(app: AppHandle) -> Result<(), String> {
    let last = app.state::<Mutex<String>>().lock().unwrap().clone();
    if last.is_empty() {
        return Err("nothing yet".into());
    }
    let mut clip = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clip.set_text(last).map_err(|e| e.to_string())
}

const RATE: usize = 16_000;
/// A chunk closes at the first pause after this much audio...
const CHUNK_TARGET: usize = RATE * 20;
/// ...or here, pause or not. Peak memory grows with chunk length (about
/// 860 MB at 20 s, 950 MB at 35 s), so this stays close to the target.
const CHUNK_CAP: usize = RATE * 26;
/// A pause is this many 20 ms frames below the energy floor.
const PAUSE_FRAMES: usize = 15;
/// A sentence-sized pause, enough to cut on early.
const LONG_PAUSE_FRAMES: usize = 50;
/// No chunk shorter than this, however long the pause.
const MIN_CHUNK: usize = RATE * 4;

// ---------- the worker: owns the model ----------

/// The id of the last cancelled take. Its queued chunks are skipped, so a
/// cancel never leaves the model grinding through audio nobody wants.
static CANCELLED: AtomicU64 = AtomicU64::new(0);

enum Job {
    Load,
    Unload,
    Chunk { take: u64, idx: usize, samples: Vec<f32> },
    Retry { id: u64, reply: Sender<Result<String, String>> },
}

enum Reply {
    Chunk { take: u64, idx: usize, text: Result<String, String> },
}

fn model_dir(app: &AppHandle) -> Option<std::path::PathBuf> {
    let id = settings::get(app).model_spec.id;
    app.path().app_data_dir().map(|d| crate::transcribe::models_dir(&d).join(id)).ok()
}

/// Loads the model if it is not loaded. False if it cannot be.
fn ensure(app: &AppHandle, engine: &mut Option<Box<dyn Engine>>) -> bool {
    if engine.is_some() {
        return true;
    }
    let Some(dir) = model_dir(app) else { return false };
    let t = Instant::now();
    match Parakeet::load(&dir) {
        Ok(p) => {
            debug::log(&format!("model loaded in {:.2}s", t.elapsed().as_secs_f32()));
            *engine = Some(Box::new(p));
            true
        }
        Err(e) => {
            log::error!("model load failed: {e}");
            false
        }
    }
}

fn worker(app: AppHandle, jobs: Receiver<Job>, replies: Sender<Reply>) {
    let mut engine: Option<Box<dyn Engine>> = None;
    for job in jobs {
        match job {
            Job::Load => {
                ensure(&app, &mut engine);
            }
            Job::Unload => {
                if engine.take().is_some() {
                    debug::log("model unloaded after idle");
                }
            }
            Job::Chunk { take, .. } if take == CANCELLED.load(Ordering::Relaxed) => {}
            Job::Chunk { take, idx, samples } => {
                crate::power::busy();
                let text = if !ensure(&app, &mut engine) {
                    Err("model not available".to_string())
                } else {
                    let trimmed = crate::trim::trim(&samples);
                    let t = Instant::now();
                    let r = engine.as_mut().unwrap().transcribe(&trimmed.samples).map_err(|e| e.to_string());
                    debug::log(&format!(
                        "chunk {idx} {:.1}s -> {:.2}s: {:?}",
                        trimmed.samples.len() as f32 / RATE as f32,
                        t.elapsed().as_secs_f32(),
                        r.as_ref().map(|s| s.chars().take(50).collect::<String>())
                    ));
                    r
                };
                crate::power::idle();
                let _ = replies.send(Reply::Chunk { take, idx, text });
            }
            Job::Retry { id, reply } => {
                crate::power::busy();
                let r = retry(&app, id, &mut engine);
                crate::power::idle();
                let _ = reply.send(r);
            }
        }
    }
}

// ---------- a take in progress ----------

struct Take {
    id: u64,
    started: Instant,
    target: focus::App,
    handsfree: bool,
    /// Samples already cut into chunks and sent to the worker.
    consumed: usize,
    /// Number of chunks sent.
    sent: usize,
}

/// Finds where to cut the pending audio. A cut lands in the middle of a
/// pause: a long one (a full second, a sentence boundary) once there is at
/// least MIN_CHUNK of audio, a short one after CHUNK_TARGET, or hard at
/// CHUNK_CAP if it never goes quiet. Cutting at sentence pauses keeps the
/// tail left at release small, which is what makes release feel instant.
fn find_cut(pending: &[f32]) -> Option<usize> {
    if pending.len() < MIN_CHUNK {
        return None;
    }
    let frame = RATE / 50;
    let rms: Vec<f32> = pending
        .chunks(frame)
        .map(|f| (f.iter().map(|x| x * x).sum::<f32>() / f.len().max(1) as f32).sqrt())
        .map(|r: f32| if r.is_finite() { r } else { 0.0 }) // NaN from a bad driver is silence
        .collect();
    let mut sorted = rms.clone();
    sorted.sort_by(f32::total_cmp);
    let floor = sorted[sorted.len() / 10];
    let peak = *sorted.last().unwrap_or(&0.0);
    // Quiet is a few times the room, but never more than a sliver of the
    // loudest frame: on a short buffer of continuous speech the "room"
    // estimate is speech itself, and that must not turn words into pauses.
    let thr = (floor * 3.0).max(0.002).min((peak * 0.15).max(0.002));
    let mut quiet = 0usize;
    for (i, r) in rms.iter().enumerate() {
        quiet = if *r < thr { quiet + 1 } else { 0 };
        let pos = i * frame;
        let long = quiet >= LONG_PAUSE_FRAMES && pos >= MIN_CHUNK;
        let short = quiet >= PAUSE_FRAMES && pos >= CHUNK_TARGET;
        if long || short {
            return Some((i - quiet / 2) * frame);
        }
    }
    (pending.len() >= CHUNK_CAP).then_some(CHUNK_CAP)
}

/// True when nothing in the buffer rises above the noise floor, so there is
/// no point sending it to the model.
fn is_silent(samples: &[f32]) -> bool {
    let frame = RATE / 50;
    samples
        .chunks(frame)
        .all(|f| (f.iter().map(|x| x * x).sum::<f32>() / f.len().max(1) as f32).sqrt() < 0.003)
}

pub fn start(app: AppHandle) {
    let (tx, rx) = mpsc::channel::<Input>();
    app.manage(Mailbox(tx));
    app.manage(Mutex::new(String::new())); // last transcript

    let (job_tx, job_rx) = mpsc::channel::<Job>();
    let (reply_tx, reply_rx) = mpsc::channel::<Reply>();
    {
        let app = app.clone();
        std::thread::spawn(move || worker(app, job_rx, reply_tx));
    }

    std::thread::spawn(move || {
        let mut recorder: Option<Recorder> = None;
        let mut take: Option<Take> = None;
        let mut last_used = Instant::now();
        let mut loaded = false;
        let mut tick: u32 = 0;
        // Inputs kept back by drain_stale, handled before the mailbox.
        let mut side: std::collections::VecDeque<Input> = Default::default();

        loop {
            // Pump mic level to the pill while listening, and look for a
            // chunk to cut a few times a second; otherwise wait for input and
            // wake up now and then to unload an idle model.
            let input = if let Some(i) = side.pop_front() {
                i
            } else if recorder.is_some() {
                match rx.recv_timeout(Duration::from_millis(16)) {
                    Ok(i) => i,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        let rec = recorder.as_ref().unwrap();
                        overlay::set_level(&app, rec.level());
                        tick = tick.wrapping_add(1);
                        if tick % 16 == 0 {
                            if let Some(t) = take.as_mut() {
                                let len = rec.len();
                                let pending = rec.copy_range(t.consumed, len);
                                if let Some(cut) = find_cut(&pending) {
                                    if !is_silent(&pending[..cut]) {
                                        let _ = job_tx.send(Job::Chunk { take: t.id, idx: t.sent, samples: pending[..cut].to_vec() });
                                        t.sent += 1;
                                    }
                                    t.consumed += cut;
                                }
                            }
                        }
                        continue;
                    }
                    Err(_) => return,
                }
            } else {
                // Idle. Nothing ticks here: with no model loaded the thread
                // simply blocks until a key is pressed; with one loaded it
                // sleeps exactly until the model is due to be dropped.
                let keep = settings::get(&app).model_keep_secs;
                let deadline = (loaded && keep >= 0).then(|| last_used + Duration::from_secs(keep as u64));
                let got = match deadline {
                    None => rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected),
                    Some(d) => rx.recv_timeout(d.saturating_duration_since(Instant::now())),
                };
                match got {
                    Ok(i) => i,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        let _ = job_tx.send(Job::Unload);
                        loaded = false;
                        continue;
                    }
                    Err(_) => return,
                }
            };

            debug::log(&format!("input {:?} (recording={})", input, recorder.is_some()));
            match input {
                Input::PttDown | Input::HandsfreeToggle | Input::PillClick if recorder.is_none() => {
                    // No model on disk yet (skipped, deleted, still coming
                    // down): say so now, instead of listening and typing nothing.
                    match crate::commands::model_state(&app).as_str() {
                        "missing" => { overlay::set_error(&app, "No speech model yet. Open Neptune to get it"); continue; }
                        "downloading" => { overlay::set_error(&app, "Still downloading the speech model"); continue; }
                        _ => {}
                    }
                    crate::power::busy();
                    let handsfree = !matches!(input, Input::PttDown);
                    // The pill answers the key before anything slow happens.
                    let state = if handsfree { State::Handsfree } else { State::Listening };
                    overlay::set_state(&app, state);
                    let _ = job_tx.send(Job::Load);
                    loaded = true;
                    recorder = begin(&app, state);
                    if recorder.is_none() {
                        crate::power::idle();
                    }
                    if recorder.is_some() {
                        let target = focus::foreground();
                        crate::icons::ensure(&app, &target.key, &target.exe);
                        take = Some(Take { id: history::now_ms(), started: Instant::now(), target, handsfree, consumed: 0, sent: 0 });
                    }
                }
                Input::PttUp if recorder.is_some() && !take.as_ref().unwrap().handsfree => {
                    let samples = recorder.take().unwrap().finish();
                    finish(&app, samples, take.take().unwrap(), &job_tx, &reply_rx);
                    crate::power::idle();
                    last_used = Instant::now();
                    drain_stale(&rx, &mut side);
                }
                // A second press while a held take records is the same hold,
                // seen twice (the window and the hook both report keys). It
                // must never end the take.
                Input::PttDown if recorder.is_some() && !take.as_ref().unwrap().handsfree => {
                    debug::log("second press during a held take, ignored");
                }
                // Held dictation locked on: keep recording, stop listening for
                // the release, and let the pill show the stop button.
                Input::HandsfreeToggle | Input::PillClick if recorder.is_some() && !take.as_ref().unwrap().handsfree => {
                    take.as_mut().unwrap().handsfree = true;
                    overlay::set_state(&app, State::Handsfree);
                    debug::log("locked on to hands-free");
                }
                // Ending a hands-free take: the hands-free combo, the pill's
                // stop button, or a press of the hold-to-talk combo.
                Input::HandsfreeToggle | Input::PillClick | Input::PttDown if recorder.is_some() => {
                    let samples = recorder.take().unwrap().finish();
                    finish(&app, samples, take.take().unwrap(), &job_tx, &reply_rx);
                    crate::power::idle();
                    last_used = Instant::now();
                    drain_stale(&rx, &mut side);
                }
                Input::DictateFile(path) if recorder.is_none() && cfg!(debug_assertions) => match history::read_wav(&path) {
                    Ok(samples) => {
                        let target = focus::foreground();
                        crate::icons::ensure(&app, &target.key, &target.exe);
                        let _ = job_tx.send(Job::Load);
                        loaded = true;
                        overlay::set_state(&app, State::Listening);
                        let mut t = Take { id: history::now_ms(), started: Instant::now(), target, handsfree: false, consumed: 0, sent: 0 };
                        // Feed it the way a live take would be fed: chunk by chunk.
                        while let Some(cut) = find_cut(&samples[t.consumed..]) {
                            if !is_silent(&samples[t.consumed..t.consumed + cut]) {
                                let _ = job_tx.send(Job::Chunk { take: t.id, idx: t.sent, samples: samples[t.consumed..t.consumed + cut].to_vec() });
                                t.sent += 1;
                            }
                            t.consumed += cut;
                        }
                        finish(&app, samples, t, &job_tx, &reply_rx);
                        last_used = Instant::now();
                    }
                    Err(e) => debug::log(&format!("dictate file failed: {e}")),
                },
                Input::Cancel if recorder.is_some() => {
                    // the pill closes first; the cleanup happens after
                    overlay::set_state(&app, State::Idle);
                    if let Some(t) = take.take() {
                        CANCELLED.store(t.id, Ordering::Relaxed);
                    }
                    let _ = recorder.take().unwrap().finish();
                    hotkeys::grab_escape(&app, false);
                    crate::power::idle();
                }
                Input::PasteLast => {
                    let last = app.state::<Mutex<String>>().lock().unwrap().clone();
                    if !last.is_empty() {
                        let _ = paste::paste(&last);
                    }
                }
                Input::OpenHistory => overlay::show_main(&app, Some("history")),
                Input::ReloadModel => {
                    let _ = job_tx.send(Job::Unload);
                    loaded = false;
                }
                Input::Retry { id, reply } => {
                    let _ = job_tx.send(Job::Retry { id, reply });
                    loaded = true;
                    last_used = Instant::now();
                }
                _ => {}
            }
        }
    });
}

/// Key presses that arrived while a take was being finished are stale: a tap
/// of the hold key that was released long ago, a toggle meant for the take
/// that just ended. Those are dropped. Everything else is kept in order, and
/// a hold that is still down (a PttDown with no PttUp after it) is kept too,
/// so someone who pressed early and is still holding gets their dictation.
fn drain_stale(rx: &Receiver<Input>, side: &mut std::collections::VecDeque<Input>) {
    let mut queued = vec![];
    while let Ok(i) = rx.try_recv() {
        queued.push(i);
    }
    if queued.is_empty() {
        return;
    }
    let last_down = queued.iter().rposition(|i| matches!(i, Input::PttDown));
    let last_up = queued.iter().rposition(|i| matches!(i, Input::PttUp));
    let still_held = matches!((last_down, last_up), (Some(d), u) if u.map_or(true, |u| u < d));
    let total = queued.len();
    let keep: Vec<Input> = queued
        .into_iter()
        .filter(|i| !matches!(i, Input::PttDown | Input::PttUp | Input::HandsfreeToggle | Input::PillClick))
        .collect();
    debug::log(&format!("dropped {} stale inputs, still held: {still_held}", total - keep.len()));
    side.extend(keep);
    if still_held {
        side.push_back(Input::PttDown);
    }
}

fn begin(app: &AppHandle, state: State) -> Option<Recorder> {
    let prefer = settings::get(app).mic;
    match Recorder::start(&prefer, true) {
        Ok(r) => {
            hotkeys::grab_escape(app, true);
            let _ = state; // already shown
            Some(r)
        }
        Err(e) => {
            debug::log(&format!("mic failed: {e}"));
            overlay::set_error(app, "Microphone not available");
            None
        }
    }
}

/// The take is over: send the tail, collect every chunk in order, clean,
/// paste, and record it.
fn finish(app: &AppHandle, samples: Vec<f32>, mut take: Take, jobs: &Sender<Job>, replies: &Receiver<Reply>) {
    hotkeys::grab_escape(app, false);
    overlay::set_state(app, State::Processing);
    let secs = samples.len() as f32 / RATE as f32;
    let wav = debug::save_wav(&samples).unwrap_or_default();
    let s = settings::get(app);

    // Under a quarter second of audio is a mis-tap, not a sentence.
    if samples.len() < RATE / 4 {
        debug::log(&format!("audio {secs:.2}s too short, ignored"));
        overlay::set_state(app, State::Idle);
        return;
    }

    // The tail: whatever was not chunked yet.
    if take.consumed < samples.len() && !is_silent(&samples[take.consumed..]) {
        let _ = jobs.send(Job::Chunk { take: take.id, idx: take.sent, samples: samples[take.consumed..].to_vec() });
        take.sent += 1;
    }

    // Collect the pieces in order. Takes are strictly sequential, so any
    // reply for another take is stale and dropped.
    let t = Instant::now();
    let mut parts: Vec<Option<String>> = vec![None; take.sent];
    let mut got = 0;
    let mut failed: Option<String> = None;
    while got < take.sent {
        match replies.recv_timeout(Duration::from_secs(120)) {
            Ok(Reply::Chunk { take: id, idx, text }) if id == take.id => {
                match text {
                    Ok(txt) => parts[idx] = Some(txt),
                    Err(e) => {
                        failed = Some(e);
                        parts[idx] = Some(String::new());
                    }
                }
                got += 1;
            }
            Ok(_) => {}
            Err(_) => {
                failed = Some("timed out".into());
                break;
            }
        }
    }
    let raw = parts.into_iter().flatten().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ");
    debug::log(&format!(
        "audio {secs:.2}s wav={wav} in {} chunks, waited {:.2}s at release: {raw:?}",
        take.sent,
        t.elapsed().as_secs_f32()
    ));

    let keep_audio = s.keep_audio != "off";
    let mut entry = Entry {
        id: 0,
        at: history::now_ms() - take.started.elapsed().as_millis() as u64,
        app: take.target.name.clone(),
        app_key: take.target.key.clone(),
        text: String::new(),
        raw: raw.clone(),
        words: 0,
        secs,
        status: "error".into(),
        audio: false,
    };

    if let Some(e) = failed {
        if raw.is_empty() {
            debug::log(&format!("transcribe failed: {e}"));
            overlay::set_error(app, "Transcription failed");
            history::add(app, entry, keep_audio.then_some(samples.as_slice()));
            return;
        }
    }

    let text = crate::cleanup::clean(&raw, s.fillers);
    if text != raw {
        debug::log(&format!("cleaned: {text:?}"));
    }
    entry.words = text.split_whitespace().count() as u32;
    entry.text = text.clone();
    if text.is_empty() {
        debug::log("transcribed to nothing");
        entry.status = "nothing".into();
        overlay::set_state(app, State::Idle);
        history::add(app, entry, keep_audio.then_some(samples.as_slice()));
        return;
    }
    *app.state::<Mutex<String>>().lock().unwrap() = text.clone();

    // The text goes wherever the cursor is now. Changing windows during a
    // take is fine; the only thing that matters is whether the window in
    // front can take text. The desktop and neptune's own window cannot: then
    // the paste is skipped and the pill offers the text instead.
    let now = focus::foreground();
    if !now.key.is_empty() && now.key != take.target.key {
        crate::icons::ensure(app, &now.key, &now.exe);
        entry.app = now.name.clone();
        entry.app_key = now.key.clone();
    }
    let moved = false;
    let to_try = TRY_MODE.load(Ordering::Relaxed) && matches!(now.key.as_str(), "neptune" | "blurt");
    let void_target = !to_try && s.textless_copy && matches!(now.key.as_str(), "explorer" | "neptune" | "blurt");
    // Dev: BLURT_NO_PASTE=1 in a debug build runs everything but the paste.
    let dry = cfg!(debug_assertions) && std::env::var("BLURT_NO_PASTE").is_ok();
    let result = if moved {
        debug::log(&format!("focus moved {} -> {}, not pasting", take.target.name, now.name));
        Err(anyhow::anyhow!("focus moved"))
    } else if void_target {
        debug::log(&format!("nowhere to paste ({}), offering copy", take.target.name));
        Err(anyhow::anyhow!("nowhere to paste"))
    } else if to_try {
        // the onboarding types it into its own box, with its own animation
        use tauri::Emitter;
        debug::log("try it: handing the text to the onboarding");
        let _ = app.emit_to(overlay::MAIN, "try_text", text.clone());
        Ok(())
    } else if dry {
        debug::log("dry run, not pasting");
        Ok(())
    } else {
        paste::paste(&text)
    };
    match result {
        Ok(()) => {
            debug::log("pasted ok");
            entry.status = "pasted".into();
            overlay::set_done(app)
        }
        Err(e) => {
            debug::log(&format!("paste failed: {e}"));
            entry.status = "paste_failed".into();
            if void_target && !moved {
                overlay::set_offer(app);
            } else {
                overlay::set_error(app, if moved { "Window changed, saved to history" } else { "Could not paste, saved to history" });
            }
        }
    }
    history::add(app, entry, keep_audio.then_some(samples.as_slice()));
}

/// Re-transcribes a history entry from its saved audio and updates it.
fn retry(app: &AppHandle, id: u64, engine: &mut Option<Box<dyn Engine>>) -> Result<String, String> {
    let e = history::get(app, id).ok_or("entry not found")?;
    if !e.audio {
        return Err("no audio kept for this dictation".into());
    }
    let path = history::audio_path(app, id).ok_or("no data dir")?;
    let samples = history::read_wav(&path).map_err(|e| e.to_string())?;
    if !ensure(app, engine) {
        return Err("model not available".into());
    }
    let engine = engine.as_mut().unwrap();
    // Same chunking as a live take, so a long entry does not take forever.
    let mut consumed = 0;
    let mut parts = vec![];
    while consumed < samples.len() {
        let rest = &samples[consumed..];
        let cut = find_cut(rest).unwrap_or(rest.len());
        let piece = crate::trim::trim(&rest[..cut]);
        let txt = engine.transcribe(&piece.samples).map_err(|e| e.to_string())?;
        if !txt.is_empty() {
            parts.push(txt);
        }
        consumed += cut;
    }
    let raw = parts.join(" ");
    let s = settings::get(app);
    let text = crate::cleanup::clean(&raw, s.fillers);
    let status = if text.is_empty() { "nothing" } else { "retried" };
    history::update(app, id, Some(&text), Some(&raw), Some(status));
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(n: f32) -> usize {
        (n * RATE as f32) as usize
    }

    /// Speech-like: bursts of noise with short gaps between words.
    fn speech(n: usize) -> Vec<f32> {
        let mut q = 7u32;
        (0..n)
            .map(|i| {
                q = q.wrapping_mul(1664525).wrapping_add(1013904223);
                let word = (i / 4000) % 3 != 2; // 250 ms on, 250 ms on, 250 ms gap
                if word { ((q >> 8) as f32 / (1u32 << 24) as f32 - 0.5) * 0.4 } else { 0.0005 }
            })
            .collect()
    }

    #[test]
    fn nothing_to_cut_in_a_short_buffer() {
        assert_eq!(find_cut(&speech(secs(3.0))), None);
    }

    #[test]
    fn a_sentence_pause_cuts_early() {
        let mut s = speech(secs(5.0));
        s.extend(std::iter::repeat(0.0004).take(secs(1.5)));
        s.extend(speech(secs(2.0)));
        let cut = find_cut(&s).expect("a cut inside the pause");
        assert!(cut > secs(5.0) && cut < secs(6.5), "cut at {:.2}s", cut as f32 / RATE as f32);
    }

    #[test]
    fn continuous_sound_is_not_mistaken_for_pauses() {
        // A steady tone has no quiet frames; without the peak cap the
        // percentile floor would call all of it quiet and cut in the middle.
        let tone: Vec<f32> = (0..secs(10.0)).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
        assert_eq!(find_cut(&tone), None);
    }

    #[test]
    fn silence_is_silent_and_speech_is_not() {
        assert!(is_silent(&vec![0.001; secs(2.0)]));
        assert!(!is_silent(&speech(secs(2.0))));
    }
}
