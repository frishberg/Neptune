// Dev check: how much memory does Parakeet take, how fast is it, and does
// the memory come back? Prints a stage, then holds a few seconds so an
// outside sampler can read working set and private bytes at each stage.
//   BLURT_MODELS=../models cargo run --release --example mem_probe -- path/to/16k.wav

use std::path::PathBuf;
use std::time::{Duration, Instant};

use transcribe_rs::onnx::parakeet::{ParakeetModel, ParakeetParams};
use transcribe_rs::onnx::Quantization;

fn stage(name: &str) {
    println!("STAGE {name} pid={}", std::process::id());
    std::thread::sleep(Duration::from_secs(3));
}

fn main() -> anyhow::Result<()> {
    let wav = std::env::args().nth(1).expect("usage: mem_probe <file.wav>");
    let models = std::env::var("BLURT_MODELS").unwrap_or_else(|_| "../models".into());
    let dir = PathBuf::from(models).join("parakeet-tdt-0.6b-v3-int8");
    let samples = transcribe_rs::audio::read_wav_samples(&PathBuf::from(&wav))?;
    stage("start");

    let t = Instant::now();
    let mut model = ParakeetModel::load(&dir, &Quantization::Int8)?;
    println!("load {:.2}s", t.elapsed().as_secs_f32());
    stage("loaded");

    for secs in [20usize, 35] {
        let n = (secs * 16_000).min(samples.len());
        // warm once, then time
        model.transcribe_with(&samples[..n], &ParakeetParams::default())?;
        let t = Instant::now();
        let r = model.transcribe_with(&samples[..n], &ParakeetParams::default())?;
        println!("{secs}s audio in {:.2}s, text {:x}", t.elapsed().as_secs_f32(), fxhash(&r.text));
        stage(&format!("after_{secs}s"));
    }
    drop(model);
    stage("dropped");
    Ok(())
}

/// A short fingerprint of the transcript, to see that variants agree.
fn fxhash(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}
