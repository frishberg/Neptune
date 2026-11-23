// Dev check: list input devices, record 3 s raw from the chosen one (no
// resampling), print peak and RMS per quarter second, save raw.wav.
//   cargo run --example mic_check [name-substring]

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn main() -> anyhow::Result<()> {
    let host = cpal::default_host();
    println!("default input: {:?}", host.default_input_device().map(|d| d.name().unwrap_or_default()));
    for d in host.input_devices()? {
        println!("  input: {:?}", d.name().unwrap_or_default());
    }
    let want = std::env::args().nth(1).unwrap_or_default().to_lowercase();
    let device = host
        .input_devices()?
        .find(|d| d.name().unwrap_or_default().to_lowercase().contains(&want))
        .or_else(|| host.default_input_device())
        .expect("no input device");
    let config = device.default_input_config()?;
    println!("using {:?} {:?}", device.name()?, config);
    let rate = config.sample_rate().0 as usize;
    let ch = config.channels() as usize;

    let buf = Arc::new(Mutex::new(Vec::<f32>::new()));
    let b = buf.clone();
    let stream = device.build_input_stream(
        &config.into(),
        move |data: &[f32], _| b.lock().unwrap().extend_from_slice(data),
        |e| eprintln!("err {e}"),
        None,
    )?;
    let secs: u64 = std::env::var("SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(3);
    stream.play()?;
    println!("recording {secs} s... speak now");
    std::thread::sleep(Duration::from_secs(secs));
    drop(stream);

    let data = buf.lock().unwrap().clone();
    // Per-channel stats and L/R correlation: opposite-polarity channels
    // cancel when averaged, which would explain a "deaf" array.
    if ch >= 2 {
        let l: Vec<f32> = data.chunks(ch).map(|c| c[0]).collect();
        let r: Vec<f32> = data.chunks(ch).map(|c| c[1]).collect();
        let pk = |v: &[f32]| v.iter().fold(0f32, |m, x| m.max(x.abs()));
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
        let (ml, mr) = (mean(&l), mean(&r));
        let num: f32 = l.iter().zip(&r).map(|(a, b)| (a - ml) * (b - mr)).sum();
        let den = (l.iter().map(|a| (a - ml).powi(2)).sum::<f32>() * r.iter().map(|b| (b - mr).powi(2)).sum::<f32>()).sqrt();
        println!("L peak {:.3}  R peak {:.3}  L/R correlation {:.3}", pk(&l), pk(&r), num / den.max(1e-9));
        let pl: Vec<String> = l.chunks(rate / 2).map(|c| format!("{:.3}", pk(c))).collect();
        println!("L peak per 0.5s: {}", pl.join(" "));
        let spec = hound::WavSpec { channels: 2, sample_rate: rate as u32, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create("../debug/raw_stereo.wav", spec)?;
        for c in data.chunks(ch) { w.write_sample((c[0].clamp(-1.0, 1.0) * 32767.0) as i16)?; w.write_sample((c[1].clamp(-1.0, 1.0) * 32767.0) as i16)?; }
        w.finalize()?;
    }
    let mono: Vec<f32> = data.chunks(ch).map(|c| c[0]).collect();
    println!("got {} frames = {:.2}s", mono.len(), mono.len() as f32 / rate as f32);
    let step = rate / 4;
    let peaks: Vec<String> = mono
        .chunks(step)
        .map(|c| format!("{:.3}", c.iter().fold(0f32, |m, v| m.max(v.abs()))))
        .collect();
    println!("peak per 0.25s: {}", peaks.join(" "));

    let spec = hound::WavSpec { channels: 1, sample_rate: rate as u32, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create("../debug/raw.wav", spec)?;
    for s in &mono {
        w.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16)?;
    }
    w.finalize()?;
    println!("saved ../debug/raw.wav");
    Ok(())
}
