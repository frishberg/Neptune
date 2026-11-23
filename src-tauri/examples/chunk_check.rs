// Dev check: does transcribing a long take in pause-split chunks change the
// text, and how does the timing compare?
//   BLURT_MODELS=../models cargo run --release --example chunk_check -- path/to/16k.wav [target_secs]

use std::path::PathBuf;
use std::time::Instant;

use transcribe_rs::onnx::parakeet::{ParakeetModel, ParakeetParams};
use transcribe_rs::onnx::Quantization;

const RATE: usize = 16_000;

/// Split points at pauses: frames of 20 ms below an energy floor, a pause is
/// >= min_pause of them in a row. A chunk closes at the first pause after
/// `target` seconds, or hard at `cap` seconds.
fn split_at_pauses(s: &[f32], target: f32, cap: f32) -> Vec<(usize, usize)> {
    let frame = RATE / 50;
    let rms: Vec<f32> = s.chunks(frame).map(|f| (f.iter().map(|x| x * x).sum::<f32>() / f.len().max(1) as f32).sqrt()).collect();
    let mut sorted = rms.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let floor = sorted[sorted.len() / 5];
    let peak = *sorted.last().unwrap_or(&0.0);
    let thr = (floor * 4.0).max(peak * 0.08).max(0.002);
    let min_pause = 15; // 300 ms
    let mut out = vec![];
    let mut start = 0usize;
    let mut quiet = 0usize;
    for (i, r) in rms.iter().enumerate() {
        quiet = if *r < thr { quiet + 1 } else { 0 };
        let secs = (i * frame - start) as f32 / RATE as f32;
        if (secs >= target && quiet >= min_pause) || secs >= cap {
            // cut in the middle of the pause
            let cut = if quiet >= min_pause { (i - quiet / 2) * frame } else { i * frame };
            out.push((start, cut));
            start = cut;
            quiet = 0;
        }
    }
    if start < s.len() {
        out.push((start, s.len()));
    }
    out
}

/// Energy trim, same rule as src/trim.rs.
fn trim(s: &[f32]) -> Vec<f32> {
    let frame = RATE / 50;
    let pad = RATE * 3 / 10;
    let rms: Vec<f32> = s.chunks(frame).map(|f| (f.iter().map(|x| x * x).sum::<f32>() / f.len().max(1) as f32).sqrt()).collect();
    if rms.is_empty() { return s.to_vec(); }
    let mut sorted = rms.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let thr = (sorted[sorted.len() / 5] * 4.0).max(sorted.last().unwrap() * 0.08).max(0.002);
    let (Some(f), Some(l)) = (rms.iter().position(|&r| r > thr), rms.iter().rposition(|&r| r > thr)) else { return s.to_vec() };
    let start = (f * frame).saturating_sub(pad);
    let end = ((l + 1) * frame + pad).min(s.len());
    s[start..end].to_vec()
}

fn main() -> anyhow::Result<()> {
    let wav = std::env::args().nth(1).expect("wav path");
    let target: f32 = std::env::args().nth(2).and_then(|v| v.parse().ok()).unwrap_or(15.0);
    let models = std::env::var("BLURT_MODELS").unwrap_or_else(|_| "../models".into());
    let dir = PathBuf::from(models).join("parakeet-tdt-0.6b-v3-int8");
    let samples = transcribe_rs::audio::read_wav_samples(&PathBuf::from(&wav))?;
    println!("audio {:.1}s", samples.len() as f32 / RATE as f32);
    let mut model = ParakeetModel::load(&dir, &Quantization::Int8)?;

    let t = Instant::now();
    let whole = model.transcribe_with(&samples, &ParakeetParams::default())?.text.trim().to_string();
    let t_whole = t.elapsed().as_secs_f32();
    println!("whole: {t_whole:.2}s, {} words", whole.split_whitespace().count());

    let chunks = split_at_pauses(&samples, target, target * 1.8);
    let mut parts = vec![];
    let mut total = 0.0;
    let mut longest = 0.0f32;
    for (a, b) in &chunks {
        let t = Instant::now();
        // Same treatment a whole take gets: silence cut off both ends, 300 ms kept.
        let piece = trim(&samples[*a..*b]);
        let txt = model.transcribe_with(&piece, &ParakeetParams::default())?.text.trim().to_string();
        let dt = t.elapsed().as_secs_f32();
        total += dt;
        longest = longest.max(dt);
        println!("  chunk {:5.1}s..{:5.1}s  {:.2}s  {:?}", *a as f32 / RATE as f32, *b as f32 / RATE as f32, dt, &txt[..txt.len().min(60)]);
        parts.push(txt);
    }
    let joined = parts.join(" ");
    println!("chunked: {} chunks, total {total:.2}s, longest chunk {longest:.2}s, {} words", chunks.len(), joined.split_whitespace().count());

    // Word-level agreement: longest common subsequence, ignoring case and punctuation.
    let norm = |s: &str| s.split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()).filter(|w| !w.is_empty()).collect::<Vec<_>>();
    let (a, b) = (norm(&whole), norm(&joined));
    let mut dp = vec![vec![0u16; b.len() + 1]; a.len() + 1];
    for i in 1..=a.len() { for j in 1..=b.len() { dp[i][j] = if a[i-1] == b[j-1] { dp[i-1][j-1] + 1 } else { dp[i-1][j].max(dp[i][j-1]) }; } }
    println!("words in common: {} of {} (whole) / {} (chunked)", dp[a.len()][b.len()], a.len(), b.len());
    println!("\n--- whole ---\n{whole}\n\n--- chunked ---\n{joined}");
    Ok(())
}
