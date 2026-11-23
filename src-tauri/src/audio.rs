// Microphone capture. Opens the chosen input, resamples to 16 kHz mono f32
// (what every speech model wants), and reports a smoothed level for the pill.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rubato::{FftFixedIn, Resampler};

pub const TARGET_RATE: usize = 16_000;

fn is_bt_handsfree(n: &str) -> bool {
    let n = n.to_lowercase();
    n.contains("hands-free") || n.contains("handsfree") || n.contains("hfp")
}

/// Every input device name, for the settings screen.
pub fn list_inputs() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

/// Which mic to open.
///
/// `prefer` (a substring, from settings) wins, then BLURT_MIC, then the
/// system default unless it looks like a Bluetooth hands-free profile (those
/// record silence unless the headset is in a call), then the first physical
/// microphone, then whatever the default was.
fn pick_device(prefer: &str) -> anyhow::Result<cpal::Device> {
    let host = cpal::default_host();
    let name = |d: &cpal::Device| d.name().unwrap_or_default();

    // The common case first, and fast: no preference, a sane default mic.
    // Enumerating every device is what costs time, so it only happens when
    // there is a preference to match or the default is unusable.
    let default = host.default_input_device();
    let wants: Vec<String> = [prefer.to_string(), std::env::var("BLURT_MIC").unwrap_or_default()].into_iter().filter(|w| !w.is_empty()).collect();
    if wants.is_empty() {
        if let Some(d) = &default {
            if !is_bt_handsfree(&name(d)) {
                return Ok(d.clone());
            }
        }
    }

    let inputs: Vec<cpal::Device> = host.input_devices()?.collect();
    for want in &wants {
        if let Some(d) = inputs.iter().find(|d| name(d).to_lowercase().contains(&want.to_lowercase())) {
            return Ok(d.clone());
        }
    }
    if let Some(d) = &default {
        if !is_bt_handsfree(&name(d)) {
            return Ok(d.clone());
        }
        crate::debug::log(&format!("default mic {:?} looks like Bluetooth hands-free, skipping", name(d)));
    }

    let physical = inputs.iter().find(|d| {
        let n = name(d).to_lowercase();
        !is_bt_handsfree(&n) && (n.contains("microphone array") || n.contains("microphone") || n.contains("mic"))
    });
    if let Some(d) = physical {
        return Ok(d.clone());
    }
    default.ok_or_else(|| anyhow::anyhow!("no microphone"))
}

pub struct Recorder {
    stream: cpal::Stream,
    samples: Arc<Mutex<Vec<f32>>>,
    level: Arc<Mutex<f32>>,
    pub device: String,
}

impl Recorder {
    /// Opens the mic and starts capturing. With `keep` false nothing is
    /// buffered, only the level is tracked (for the meter in settings).
    pub fn start(prefer: &str, keep: bool) -> anyhow::Result<Self> {
        let device = pick_device(prefer)?;
        let config = device.default_input_config()?;
        let in_rate = config.sample_rate().0 as usize;
        let channels = config.channels() as usize;
        let device_name = device.name().unwrap_or_default();
        crate::debug::log(&format!(
            "mic: {:?} {} Hz {} ch {:?}",
            device_name,
            in_rate,
            channels,
            config.sample_format()
        ));

        let samples = Arc::new(Mutex::new(Vec::<f32>::with_capacity(if keep { TARGET_RATE * 60 } else { 0 })));
        let level = Arc::new(Mutex::new(0.0f32));
        let keep = Arc::new(AtomicBool::new(keep));

        let chunk = 1024;
        let mut resampler = if in_rate != TARGET_RATE {
            Some(FftFixedIn::<f32>::new(in_rate, TARGET_RATE, chunk, 2, 1)?)
        } else {
            None
        };
        let mut pending: Vec<f32> = Vec::with_capacity(chunk * 2);

        let s = samples.clone();
        let l = level.clone();
        let k = keep.clone();
        // One callback body for every sample format the device might use;
        // each format is converted to f32 on the way in.
        // The level the pill draws. Measured on real speech: RMS over the
        // last 30 ms (one callback alone jumps with every pitch period),
        // a perceptual curve so quiet and loud read in proportion, then an
        // envelope with a 15 ms attack and a 100 ms release. The pill adds
        // a spring on top. Together that is about half the jerk of a raw
        // per-callback level with 50 ms of lag, which the eye does not see.
        let mut recent: std::collections::VecDeque<f32> = std::collections::VecDeque::new();
        let mut on_data = move |data: Vec<f32>| {
            let frames = (data.len() / channels).max(1);
            let dt_ms = frames as f32 * 1000.0 / in_rate as f32;
            let window = ((30.0 / dt_ms).round() as usize).max(1);
            recent.push_back(data.iter().map(|x| x * x).sum::<f32>() / data.len().max(1) as f32);
            while recent.len() > window {
                recent.pop_front();
            }
            let rms = (recent.iter().sum::<f32>() / recent.len() as f32).sqrt();
            let target = (rms * 6.0).powf(0.75).min(1.0);
            let attack = 1.0 - (-dt_ms / 15.0).exp();
            let release = 1.0 - (-dt_ms / 100.0).exp();
            let mut lv = l.lock().unwrap();
            *lv += (target - *lv) * if target > *lv { attack } else { release };
            drop(lv);
            if !k.load(Ordering::Relaxed) {
                return;
            }

            // Fold channels to mono.
            let mono = data.chunks(channels).map(|c| c.iter().sum::<f32>() / channels as f32);
            pending.extend(mono);

            let mut out = s.lock().unwrap();
            match resampler.as_mut() {
                None => out.extend(pending.drain(..)),
                Some(r) => {
                    while pending.len() >= chunk {
                        let block: Vec<f32> = pending.drain(..chunk).collect();
                        if let Ok(res) = r.process(&[block], None) {
                            out.extend_from_slice(&res[0]);
                        }
                    }
                }
            }
        };
        let err = |e| log::error!("mic error: {e}");
        let cfg: cpal::StreamConfig = config.clone().into();
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(&cfg, move |d: &[f32], _| on_data(d.to_vec()), err, None)?,
            cpal::SampleFormat::I16 => device.build_input_stream(&cfg, move |d: &[i16], _| on_data(d.iter().map(|&x| x as f32 / 32768.0).collect()), err, None)?,
            cpal::SampleFormat::U16 => device.build_input_stream(&cfg, move |d: &[u16], _| on_data(d.iter().map(|&x| (x as f32 - 32768.0) / 32768.0).collect()), err, None)?,
            cpal::SampleFormat::I32 => device.build_input_stream(&cfg, move |d: &[i32], _| on_data(d.iter().map(|&x| x as f32 / 2147483648.0).collect()), err, None)?,
            cpal::SampleFormat::I8 => device.build_input_stream(&cfg, move |d: &[i8], _| on_data(d.iter().map(|&x| x as f32 / 128.0).collect()), err, None)?,
            other => anyhow::bail!("unsupported microphone sample format {other:?}"),
        };
        stream.play()?;

        Ok(Self { stream, samples, level, device: device_name })
    }

    pub fn level(&self) -> f32 {
        *self.level.lock().unwrap()
    }

    /// Samples captured so far.
    pub fn len(&self) -> usize {
        self.samples.lock().unwrap().len()
    }

    /// A copy of samples[a..b], clamped to what exists.
    pub fn copy_range(&self, a: usize, b: usize) -> Vec<f32> {
        let s = self.samples.lock().unwrap();
        let b = b.min(s.len());
        let a = a.min(b);
        s[a..b].to_vec()
    }

    /// Stops the stream and returns everything captured, 16 kHz mono.
    pub fn finish(self) -> Vec<f32> {
        drop(self.stream);
        std::mem::take(&mut *self.samples.lock().unwrap())
    }
}
