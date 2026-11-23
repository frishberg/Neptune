// User settings. One JSON file in the app config dir, loaded at startup,
// held in app state, written on every change. Field names match the JS side.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub onboarded: bool,
    pub ptt: Vec<String>,
    pub handsfree: Vec<String>,
    pub paste_last: Vec<String>,
    pub history: Vec<String>,
    /// Substring of the microphone name to prefer. Empty = system default.
    pub mic: String,
    pub fillers: bool,
    /// A paste that lands nowhere (desktop, this window) turns the pill into a Copy button.
    pub textless_copy: bool,
    /// "forever", "off", or a span like "10d", "6mo", "2y" (h, d, w, mo, y, dec, c).
    pub keep_audio: String,
    /// Same form, minus "off".
    pub keep_history: String,
    pub autostart: bool,
    pub pill: bool,
    /// How long the speech model stays in memory after the last dictation,
    /// in seconds. 0 drops it as soon as a take is done; -1 never drops it.
    pub model_keep_secs: i64,
    /// The speech model in use.
    pub model_spec: crate::transcribe::ModelSpec,
    /// A newer model the manifest offered, until it is installed.
    pub model_update: Option<crate::transcribe::ModelSpec>,
    /// Unix ms of the last manifest check.
    pub model_checked: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            onboarded: false,
            ptt: v(&["Ctrl", "Space"]),
            handsfree: v(&["Ctrl", "Shift", "Space"]),
            paste_last: v(&["Ctrl", "Shift", "V"]),
            history: v(&["Ctrl", "Shift", "H"]),
            mic: String::new(),
            fillers: true,
            textless_copy: true,
            // audio is about 2 MB a minute; a week covers Retry without
            // quietly filling the disk
            keep_audio: "7d".into(),
            keep_history: "forever".into(),
            autostart: false,
            pill: true,
            model_keep_secs: 60,
            model_spec: crate::transcribe::builtin(),
            model_update: None,
            model_checked: 0,
        }
    }
}

fn v(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

pub struct Store(pub Mutex<Settings>);

fn path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("settings.json"))
}

pub fn load(app: &AppHandle) -> Settings {
    path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| {
            // Editors on Windows like to prepend a byte-order mark.
            let s = s.trim_start_matches('\u{feff}');
            match serde_json::from_str(s) {
                Ok(v) => Some(v),
                Err(e) => {
                    log::error!("settings.json unreadable, using defaults: {e}");
                    None
                }
            }
        })
        .unwrap_or_default()
}

pub fn save(app: &AppHandle, s: &Settings) {
    if let Some(p) = path(app) {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(s) {
            let _ = std::fs::write(p, json);
        }
    }
}

/// A snapshot of the current settings.
pub fn get(app: &AppHandle) -> Settings {
    app.state::<Store>().0.lock().unwrap().clone()
}

/// Replace the settings in memory and on disk.
pub fn set(app: &AppHandle, s: Settings) {
    *app.state::<Store>().0.lock().unwrap() = s.clone();
    save(app, &s);
}
