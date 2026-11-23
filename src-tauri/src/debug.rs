// Debug capture. OFF by default.
//
// Turned on only by BLURT_DEBUG=1 in a debug build. Release builds ignore
// the variable entirely, so nothing a user says can ever be written to disk
// by a shipped binary. Do not change that.
//
// When on, every dictation appends a line to debug/log.txt and saves the raw
// 16 kHz audio as debug/<timestamp>.wav so it can be replayed through the
// model with `cargo run --example transcribe_wav`.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn enabled() -> bool {
    cfg!(debug_assertions) && std::env::var("BLURT_DEBUG").map(|v| v == "1").unwrap_or(false)
}

fn dir() -> PathBuf {
    // <repo>/debug next to src-tauri, or wherever the exe was launched from.
    let d = std::env::var("BLURT_DEBUG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("../debug"));
    let _ = fs::create_dir_all(&d);
    d
}

fn stamp() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

pub fn log(line: &str) {
    if !enabled() {
        return;
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(dir().join("log.txt")) {
        let _ = writeln!(f, "{} {}", stamp(), line);
    }
    log::info!("{line}");
}

/// Saves the dictation audio and returns the file stem used, for the log line.
pub fn save_wav(samples_16k: &[f32]) -> Option<String> {
    if !enabled() {
        return None;
    }
    let name = stamp().to_string();
    let path = dir().join(format!("{name}.wav"));
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(&path, spec).ok()?;
    for s in samples_16k {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        w.write_sample(v).ok()?;
    }
    w.finalize().ok()?;
    Some(name)
}
