// Everything the main window can ask the backend to do.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;

use crate::history::{self, Entry};
use crate::session::{self, Input};
use crate::settings::{self, Settings};
use crate::{audio, hotkeys, overlay, paste, transcribe};

#[tauri::command]
pub fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// ---------- history ----------

#[tauri::command]
pub fn history_list(app: AppHandle) -> Vec<Entry> {
    history::list(&app)
}

#[tauri::command]
pub fn history_delete(app: AppHandle, id: u64) {
    history::delete(&app, id);
}

#[tauri::command]
pub fn history_update(app: AppHandle, id: u64, text: String) {
    history::update(&app, id, Some(&text), None, None);
}

// Sync commands run on the main thread and would freeze every window while
// they wait, so anything that blocks is async and runs on a worker.

#[tauri::command]
pub async fn history_retry(app: AppHandle, id: u64) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (tx, rx) = mpsc::channel();
        session::send(&app, Input::Retry { id, reply: tx });
        rx.recv_timeout(Duration::from_secs(120)).map_err(|_| "timed out".to_string())?
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn copy_text(text: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut clip = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        clip.set_text(text).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Pastes into whatever window had focus before ours. The main window is
/// hidden first so the keystroke does not land back in neptune.
#[tauri::command]
pub async fn paste_text(app: AppHandle, text: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        overlay::hide_main(&app);
        std::thread::sleep(Duration::from_millis(250));
        paste::paste(&text).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---------- settings ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    #[serde(flatten)]
    pub settings: Settings,
    pub mics: Vec<String>,
    pub model_state: String,
    /// Running as the Microsoft Store package: updates come from the Store.
    pub packaged: bool,
}

fn model_dir_for(app: &AppHandle, id: &str) -> Result<std::path::PathBuf, String> {
    app.path().app_data_dir().map(|d| transcribe::models_dir(&d).join(id)).map_err(|e| e.to_string())
}

/// Hands physical keys to the main window while a page with keycaps shows.
#[tauri::command]
pub fn key_relay(on: bool) {
    #[cfg(windows)]
    crate::ptt_hook::set_relay(on);
    #[cfg(not(windows))]
    let _ = on;
}

/// True while a model download is running (in this run of the app).
static DL_RUNNING: AtomicBool = AtomicBool::new(false);

pub fn model_state(app: &AppHandle) -> String {
    if DL_RUNNING.load(Ordering::SeqCst) {
        return "downloading".into();
    }
    let spec = settings::get(app).model_spec;
    let ok = model_dir_for(app, &spec.id).map(|d| spec.files.iter().all(|f| d.join(f).exists())).unwrap_or(false);
    if ok { "ready".into() } else { "missing".into() }
}

#[tauri::command]
pub async fn settings_get(app: AppHandle) -> Result<SettingsView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut settings = settings::get(&app);
        // packaged: Windows owns start at login (the user can switch it in
        // Settings > Apps > Startup), so show what Windows says
        if let Some(on) = crate::package::startup_enabled() {
            settings.autostart = on;
        }
        SettingsView { settings, mics: audio::list_inputs(), model_state: model_state(&app), packaged: crate::package::is_packaged() }
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn settings_set(app: AppHandle, settings: Settings) {
    let old = settings::get(&app);
    settings::set(&app, settings.clone());
    let s = settings;

    if [&s.ptt, &s.handsfree, &s.paste_last, &s.history] != [&old.ptt, &old.handsfree, &old.paste_last, &old.history] {
        let _ = hotkeys::apply(&app, &s);
    }
    if s.autostart != old.autostart {
        if crate::package::is_packaged() {
            // the Store package starts at login through its StartupTask
            if let Err(e) = crate::package::set_startup(s.autostart) {
                log::error!("start at login: {e}");
            }
        } else {
            let al = app.autolaunch();
            let _ = if s.autostart { al.enable() } else { al.disable() };
        }
    }
    if s.pill != old.pill {
        if s.pill { overlay::show(&app) } else { overlay::hide(&app) }
    }
    if s.keep_audio != old.keep_audio || s.keep_history != old.keep_history {
        history::prune(&app, &s.keep_audio, &s.keep_history);
        let _ = app.emit_to(overlay::MAIN, "history_changed", ());
    }
}

// ---------- microphone meter ----------

pub struct Meter(pub Mutex<Option<Arc<AtomicBool>>>);

/// Streams mic level to the main window as "level" events until stopped.
#[tauri::command]
pub fn meter_start(app: AppHandle) {
    let stop = Arc::new(AtomicBool::new(false));
    {
        let state = app.state::<Meter>();
        let mut slot = state.0.lock().unwrap();
        if let Some(old) = slot.take() {
            old.store(true, Ordering::Relaxed);
        }
        *slot = Some(stop.clone());
    }
    let prefer = settings::get(&app).mic;
    let app = app.clone();
    std::thread::spawn(move || {
        let Ok(rec) = audio::Recorder::start(&prefer, false) else {
            let _ = app.emit_to(overlay::MAIN, "level", -1.0f32);
            return;
        };
        let _ = app.emit_to(overlay::MAIN, "mic_name", rec.device.clone());
        while !stop.load(Ordering::Relaxed) {
            let _ = app.emit_to(overlay::MAIN, "level", rec.level());
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = rec.finish();
    });
}

#[tauri::command]
pub fn meter_stop(app: AppHandle) {
    if let Some(stop) = app.state::<Meter>().0.lock().unwrap().take() {
        stop.store(true, Ordering::Relaxed);
    }
}

// ---------- model download and update ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub file: String,
    pub done: u64,
    pub total: u64,
    pub finished: bool,
    pub error: Option<String>,
}

fn http() -> ureq::Agent {
    // Connect and response caps so a dead network fails instead of hanging
    // forever; the body cap is generous for slow links.
    let cfg = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_recv_body(Some(Duration::from_secs(45 * 60)))
        .build();
    ureq::Agent::new_with_config(cfg)
}

/// Downloads a model into the models dir, emitting "download" progress. With
/// no spec it fetches the installed one (first run); with a spec it fetches
/// that one and, once complete, makes it the installed model and removes the
/// old files.
static PAUSED: AtomicBool = AtomicBool::new(false);

/// Pauses or resumes the running download. The loop simply waits.
#[tauri::command]
pub fn model_download_pause(paused: bool) {
    PAUSED.store(paused, Ordering::SeqCst);
}

#[tauri::command]
pub fn model_download(app: AppHandle, spec: Option<transcribe::ModelSpec>) -> Result<(), String> {
    use DL_RUNNING as RUNNING;
    PAUSED.store(false, Ordering::SeqCst);
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Ok(()); // already downloading; the running one keeps emitting progress
    }
    let installed = settings::get(&app).model_spec;
    let target = spec.unwrap_or_else(|| installed.clone());
    let dir = match model_dir_for(&app, &target.id) {
        Ok(d) => d,
        Err(e) => { RUNNING.store(false, Ordering::SeqCst); return Err(e); }
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        RUNNING.store(false, Ordering::SeqCst);
        return Err(e.to_string());
    }
    std::thread::spawn(move || {
        struct Done;
        impl Drop for Done { fn drop(&mut self) { RUNNING.store(false, Ordering::SeqCst); } }
        let _done = Done;
        let emit = |p: Progress| { let _ = app.emit_to(overlay::MAIN, "download", p); };
        let total = target.total();
        let mut done_before: u64 = 0;
        for (i, file) in target.files.iter().enumerate() {
            let dest = dir.join(file);
            let expected = target.sizes.get(i).copied().unwrap_or(0);
            if dest.exists() && std::fs::metadata(&dest).map(|m| m.len() >= expected / 2).unwrap_or(false) {
                done_before += expected;
                emit(Progress { file: file.to_string(), done: done_before, total, finished: false, error: None });
                continue;
            }
            let url = format!("{}/{}", target.url, file);
            let r = (|| -> anyhow::Result<()> {
                // Pick up where an earlier try stopped (app closed, network
                // dropped): ask only for the bytes the .part file lacks.
                let tmp = dest.with_extension("part");
                let have = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
                let mut req = http().get(&url);
                if have > 0 {
                    req = req.header("Range", &format!("bytes={have}-"));
                }
                let resp = req.call()?;
                let resumed = have > 0 && resp.status().as_u16() == 206;
                let body_len = resp.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
                let want = body_len.map(|n| n + if resumed { have } else { 0 });
                let mut reader = resp.into_body().into_reader();
                let mut out = if resumed {
                    std::fs::OpenOptions::new().append(true).open(&tmp)?
                } else {
                    std::fs::File::create(&tmp)?
                };
                let mut buf = [0u8; 256 * 1024];
                let mut got: u64 = if resumed { have } else { 0 };
                if resumed {
                    emit(Progress { file: file.to_string(), done: done_before + got.min(expected), total, finished: false, error: None });
                }
                let mut last = std::time::Instant::now();
                loop {
                    while PAUSED.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(120));
                    }
                    let n = std::io::Read::read(&mut reader, &mut buf)?;
                    if n == 0 { break; }
                    std::io::Write::write_all(&mut out, &buf[..n])?;
                    got += n as u64;
                    if last.elapsed() > Duration::from_millis(120) {
                        last = std::time::Instant::now();
                        emit(Progress { file: file.to_string(), done: done_before + got.min(expected), total, finished: false, error: None });
                    }
                }
                std::io::Write::flush(&mut out)?;
                out.sync_all()?;
                drop(out);
                // a body that ended early is not a model file
                if let Some(n) = want {
                    if got != n {
                        anyhow::bail!("the connection dropped ({} of {} MB). Try again to continue", got / 1_048_576, n / 1_048_576);
                    }
                }
                std::fs::rename(&tmp, &dest)?;
                Ok(())
            })();
            if let Err(e) = r {
                emit(Progress { file: file.to_string(), done: done_before, total, finished: true, error: Some(e.to_string()) });
                return;
            }
            done_before += expected;
        }
        if target.id != installed.id {
            // Swap: settings point at the new model, the worker drops the old
            // one, and its files go.
            let mut s = settings::get(&app);
            s.model_spec = target.clone();
            s.model_update = None;
            settings::set(&app, s);
            session::send(&app, Input::ReloadModel);
            std::thread::sleep(Duration::from_millis(1500));
            if let Ok(old) = model_dir_for(&app, &installed.id) {
                let _ = std::fs::remove_dir_all(old);
            }
        }
        emit(Progress { file: String::new(), done: total, total, finished: true, error: None });
        let _ = app.emit_to(overlay::MAIN, "model_state", "ready");
    });
    Ok(())
}

#[derive(serde::Deserialize)]
struct Manifest {
    model: transcribe::ModelSpec,
}

/// Asks the manifest whether a newer model exists. Quiet on any failure:
/// no network, no manifest, nothing new all come back as None.
#[tauri::command]
pub async fn model_check(app: AppHandle, force: Option<bool>) -> Result<Option<transcribe::ModelSpec>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut s = settings::get(&app);
        let day = 86_400_000u64;
        if !force.unwrap_or(false) && history::now_ms().saturating_sub(s.model_checked) < day {
            return Ok(s.model_update);
        }
        let fetched = (|| -> anyhow::Result<Manifest> {
            let cfg = ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(12)))
                .build();
            let body = ureq::Agent::new_with_config(cfg).get(transcribe::MODEL_MANIFEST_URL).call()?.body_mut().read_to_string()?;
            Ok(serde_json::from_str(&body)?)
        })();
        s.model_checked = history::now_ms();
        match fetched {
            Ok(m) => {
                let newer = m.model.id != s.model_spec.id && !m.model.files.is_empty() && m.model.files.len() == m.model.sizes.len();
                s.model_update = newer.then_some(m.model);
            }
            Err(e) => log::info!("model manifest not available: {e}"),
        }
        let out = s.model_update.clone();
        settings::set(&app, s);
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---------- app updates ----------

pub struct UpdateSlot(pub Mutex<Option<tauri_plugin_updater::Update>>);

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub notes: Option<String>,
}

/// Looks for a newer release. None when there is none, or when the check
/// cannot be made (offline, no release feed yet).
#[tauri::command]
pub async fn app_update_check(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    use tauri_plugin_updater::UpdaterExt;
    // the Store keeps its own copy up to date; the in-app updater would
    // fetch the .exe installer, which is not how a package updates
    if crate::package::is_packaged() {
        return Ok(None);
    }
    let updater = match app.updater() {
        Ok(u) => u,
        Err(e) => { log::info!("updater not configured: {e}"); return Ok(None); }
    };
    match updater.check().await {
        Ok(Some(u)) => {
            let info = UpdateInfo { version: u.version.clone(), notes: u.body.clone() };
            *app.state::<UpdateSlot>().0.lock().unwrap() = Some(u);
            Ok(Some(info))
        }
        Ok(None) => Ok(None),
        Err(e) => { log::info!("update check failed: {e}"); Ok(None) }
    }
}

/// Downloads and installs the release found by app_update_check, emitting
/// "update" progress, then restarts.
#[tauri::command]
pub async fn app_update_install(app: AppHandle) -> Result<(), String> {
    let update = app.state::<UpdateSlot>().0.lock().unwrap().take().ok_or("check for an update first")?;
    let a = app.clone();
    let mut done: u64 = 0;
    update
        .download_and_install(
            move |chunk, total| {
                done += chunk as u64;
                let _ = a.emit_to(overlay::MAIN, "update", Progress { file: String::new(), done, total: total.unwrap_or(0), finished: false, error: None });
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    let _ = app.emit_to(overlay::MAIN, "update", Progress { file: String::new(), done, total: done, finished: true, error: None });
    std::thread::sleep(Duration::from_millis(400));
    app.restart();
}

#[tauri::command]
pub fn model_state_get(app: AppHandle) -> String {
    model_state(&app)
}

/// The cached icon of an app seen in history, as a data URL.
#[tauri::command]
pub fn app_icon(app: AppHandle, key: String) -> Option<String> {
    crate::icons::data_url(&app, &key)
}

/// Screenshot mode, for the website: only when launched with NEPTUNE_DEMO=1.
/// The window then shows sample history, and the icons for the sample apps
/// are pulled from the real installed executables, the same way as always.
#[tauri::command]
pub fn demo_mode(app: AppHandle) -> bool {
    if std::env::var_os("NEPTUNE_DEMO").is_none() {
        return false;
    }
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let (local, pf, pf86) = (env("LOCALAPPDATA"), env("ProgramFiles"), env("ProgramFiles(x86)"));
    let discord = std::fs::read_dir(format!("{local}\\Discord"))
        .ok()
        .and_then(|d| d.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("app-"))).max())
        .map(|p| p.join("Discord.exe").to_string_lossy().into_owned())
        .unwrap_or_default();
    let apps = [
        ("chrome", format!("{pf}\\Google\\Chrome\\Application\\chrome.exe")),
        ("outlook", format!("{pf}\\Microsoft Office\\root\\Office16\\OUTLOOK.EXE")),
        ("code", format!("{local}\\Programs\\Microsoft VS Code\\Code.exe")),
        ("msedge", format!("{pf86}\\Microsoft\\Edge\\Application\\msedge.exe")),
        ("discord", discord),
    ];
    for (key, exe) in apps {
        if std::path::Path::new(&exe).exists() {
            crate::icons::ensure(&app, key, &exe);
        }
    }
    true
}

#[tauri::command]
pub fn open_main(app: AppHandle, view: Option<String>) {
    overlay::show_main(&app, view.as_deref());
}
