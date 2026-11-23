// Dev check: transcribe a 16 kHz mono WAV with the bundled Parakeet setup.
//   BLURT_MODELS=../models cargo run --example transcribe_wav -- path/to/file.wav

use std::path::PathBuf;
use std::time::Instant;

use transcribe_rs::onnx::parakeet::{ParakeetModel, ParakeetParams};
use transcribe_rs::onnx::Quantization;

fn main() -> anyhow::Result<()> {
    let wav = std::env::args().nth(1).expect("usage: transcribe_wav <file.wav>");
    let models = std::env::var("BLURT_MODELS").unwrap_or_else(|_| "../models".into());
    let dir = PathBuf::from(models).join("parakeet-tdt-0.6b-v3-int8");

    let t = Instant::now();
    let mut model = ParakeetModel::load(&dir, &Quantization::Int8)?;
    println!("model loaded in {:.2}s", t.elapsed().as_secs_f32());

    let samples = transcribe_rs::audio::read_wav_samples(&PathBuf::from(&wav))?;
    println!("audio: {:.2}s", samples.len() as f32 / 16_000.0);

    let t = Instant::now();
    let result = model.transcribe_with(&samples, &ParakeetParams::default())?;
    println!("transcribed in {:.2}s", t.elapsed().as_secs_f32());
    println!("---\n{}\n---", result.text.trim());
    Ok(())
}
