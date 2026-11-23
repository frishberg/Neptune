// Cut the silence off both ends of a dictation before it reaches the model.
//
// Parakeet returns nothing for a short utterance buried in a long quiet
// lead-in (people pause after pressing the key). Trimming to the speech,
// with a little padding, fixes that and also makes inference faster.
//
// Energy only, no model: 20 ms frames, a frame counts as speech when its RMS
// is above a floor derived from the quietest frames and above a fraction of
// the loudest frame.

const RATE: usize = 16_000;
const FRAME: usize = RATE / 50; // 20 ms
const PAD: usize = RATE * 3 / 10; // 300 ms kept on each side

#[allow(dead_code)]
pub struct Trimmed {
    pub samples: Vec<f32>,
    pub start_secs: f32,
    pub end_secs: f32,
}

pub fn trim(samples: &[f32]) -> Trimmed {
    let n = samples.len();
    let rms: Vec<f32> = samples
        .chunks(FRAME)
        .map(|f| (f.iter().map(|x| x * x).sum::<f32>() / f.len().max(1) as f32).sqrt())
        .collect();
    if rms.is_empty() {
        return Trimmed { samples: samples.to_vec(), start_secs: 0.0, end_secs: 0.0 };
    }

    // a bad driver can hand over NaN or infinity; that is silence, not a crash
    let rms: Vec<f32> = rms.into_iter().map(|r| if r.is_finite() { r } else { 0.0 }).collect();
    let mut sorted = rms.clone();
    sorted.sort_by(f32::total_cmp);
    let floor = sorted[sorted.len() / 5]; // 20th percentile = the room
    let peak = *sorted.last().unwrap();
    let threshold = (floor * 4.0).max(peak * 0.08).max(0.002);

    let first = rms.iter().position(|&r| r > threshold);
    let last = rms.iter().rposition(|&r| r > threshold);
    let (Some(first), Some(last)) = (first, last) else {
        return Trimmed { samples: samples.to_vec(), start_secs: 0.0, end_secs: n as f32 / RATE as f32 };
    };

    let start = (first * FRAME).saturating_sub(PAD);
    let end = ((last + 1) * FRAME + PAD).min(n);
    Trimmed {
        samples: samples[start..end].to_vec(),
        start_secs: start as f32 / RATE as f32,
        end_secs: end as f32 / RATE as f32,
    }
}
