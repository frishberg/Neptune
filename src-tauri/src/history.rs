// Dictation history. Append-only JSON lines in the app data dir, one record
// per dictation, plus the 16 kHz audio next to it when the user keeps audio.
// Everything here stays on this machine.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: u64,
    /// Unix milliseconds.
    pub at: u64,
    pub app: String,
    pub app_key: String,
    pub text: String,
    pub raw: String,
    pub words: u32,
    pub secs: f32,
    /// "pasted" | "typed" | "paste_failed" | "nothing" | "error"
    pub status: String,
    pub audio: bool,
}

pub struct Lock(pub Mutex<()>);

fn dir(app: &AppHandle) -> Option<PathBuf> {
    let d = app.path().app_data_dir().ok()?.join("history");
    let _ = std::fs::create_dir_all(d.join("audio"));
    Some(d)
}

fn file(app: &AppHandle) -> Option<PathBuf> {
    dir(app).map(|d| d.join("history.jsonl"))
}

pub fn audio_path(app: &AppHandle, id: u64) -> Option<PathBuf> {
    dir(app).map(|d| d.join("audio").join(format!("{id}.wav")))
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub fn list(app: &AppHandle) -> Vec<Entry> {
    let lock = app.state::<Lock>();
    let _g = lock.0.lock().unwrap();
    read(app)
}

fn read(app: &AppHandle) -> Vec<Entry> {
    let Some(p) = file(app) else { return vec![] };
    let Ok(text) = std::fs::read_to_string(p) else { return vec![] };
    let mut v: Vec<Entry> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    v.sort_by(|a, b| b.at.cmp(&a.at));
    v
}

fn write_all(app: &AppHandle, entries: &[Entry]) {
    let Some(p) = file(app) else { return };
    let mut out = String::new();
    // Oldest first on disk so appends stay in order.
    for e in entries.iter().rev() {
        if let Ok(l) = serde_json::to_string(e) {
            out.push_str(&l);
            out.push('\n');
        }
    }
    let _ = std::fs::write(p, out);
}

fn changed(app: &AppHandle) {
    let _ = app.emit_to("main", "history_changed", ());
}

/// Adds an entry, optionally with its audio, and tells the window.
pub fn add(app: &AppHandle, mut e: Entry, samples_16k: Option<&[f32]>) -> Entry {
    {
        let lock = app.state::<Lock>();
    let _g = lock.0.lock().unwrap();
        if e.id == 0 {
            e.id = now_ms();
        }
        if let Some(s) = samples_16k {
            e.audio = audio_path(app, e.id).map(|p| write_wav(&p, s)).unwrap_or(false);
        }
        if let Some(p) = file(app) {
            if let Ok(l) = serde_json::to_string(&e) {
                use std::io::Write;
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
                    let _ = writeln!(f, "{l}");
                }
            }
        }
    }
    changed(app);
    e
}

pub fn update(app: &AppHandle, id: u64, text: Option<&str>, raw: Option<&str>, status: Option<&str>) {
    {
        let lock = app.state::<Lock>();
    let _g = lock.0.lock().unwrap();
        let mut all = read(app);
        if let Some(e) = all.iter_mut().find(|e| e.id == id) {
            if let Some(t) = text {
                e.text = t.to_string();
                e.words = t.split_whitespace().count() as u32;
            }
            if let Some(r) = raw {
                e.raw = r.to_string();
            }
            if let Some(s) = status {
                e.status = s.to_string();
            }
        }
        write_all(app, &all);
    }
    changed(app);
}

pub fn delete(app: &AppHandle, id: u64) {
    {
        let lock = app.state::<Lock>();
    let _g = lock.0.lock().unwrap();
        let all: Vec<Entry> = read(app).into_iter().filter(|e| e.id != id).collect();
        write_all(app, &all);
        if let Some(p) = audio_path(app, id) {
            let _ = std::fs::remove_file(p);
        }
    }
    changed(app);
}

pub fn get(app: &AppHandle, id: u64) -> Option<Entry> {
    list(app).into_iter().find(|e| e.id == id)
}

/// "forever" -> None, "off" -> Some(0), "10d" / "6mo" / "2y" -> that many ms.
/// Units: h, d, w, mo, y, dec, c. Anything unreadable keeps everything.
pub fn span_ms(s: &str) -> Option<u64> {
    let day = 86_400_000u64;
    match s {
        "forever" => None,
        "off" => Some(0),
        _ => {
            let n_end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
            let n: u64 = s[..n_end].parse().ok()?;
            let per = match &s[n_end..] {
                "h" => 3_600_000,
                "d" => day,
                "w" => 7 * day,
                "mo" => 30 * day,
                "y" => 365 * day,
                "dec" => 3650 * day,
                "c" => 36_500 * day,
                _ => return None,
            };
            Some(n.saturating_mul(per))
        }
    }
}

/// Drops audio and entries older than the retention settings.
pub fn prune(app: &AppHandle, keep_audio: &str, keep_history: &str) {
    let now = now_ms();
    let audio_ms = span_ms(keep_audio);
    let hist_ms = span_ms(keep_history);
    let lock = app.state::<Lock>();
    let _g = lock.0.lock().unwrap();
    let mut all = read(app);
    let mut dirty = false;
    if let Some(ms) = hist_ms {
        let before = all.len();
        for e in all.iter().filter(|e| now.saturating_sub(e.at) > ms) {
            if let Some(p) = audio_path(app, e.id) {
                let _ = std::fs::remove_file(p);
            }
        }
        all.retain(|e| now.saturating_sub(e.at) <= ms);
        dirty |= all.len() != before;
    }
    if let Some(ms) = audio_ms {
        for e in all.iter_mut().filter(|e| e.audio && now.saturating_sub(e.at) > ms) {
            if let Some(p) = audio_path(app, e.id) {
                let _ = std::fs::remove_file(p);
            }
            e.audio = false;
            dirty = true;
        }
    }
    if dirty {
        write_all(app, &all);
    }
}

pub fn write_wav(path: &std::path::Path, samples: &[f32]) -> bool {
    let spec = hound::WavSpec { channels: 1, sample_rate: 16_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let Ok(mut w) = hound::WavWriter::create(path, spec) else { return false };
    for s in samples {
        if w.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).is_err() {
            return false;
        }
    }
    w.finalize().is_ok()
}

pub fn read_wav(path: &std::path::Path) -> anyhow::Result<Vec<f32>> {
    let mut r = hound::WavReader::open(path)?;
    let v = r.samples::<i16>().map(|s| s.map(|v| v as f32 / i16::MAX as f32)).collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}
