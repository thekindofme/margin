//! Kaldi-compatible log-mel filterbank (80 bins, 25 ms / 10 ms, povey window,
//! pre-emphasis 0.97, DC removal, no dither, snip_edges=true).
use rustfft::{num_complex::Complex, FftPlanner};
use std::f32::consts::PI;

pub const SAMPLE_RATE: usize = 16_000;
pub const NUM_BINS: usize = 80;
pub const FRAME_LEN: usize = 400;
pub const FRAME_SHIFT: usize = 160;
const NFFT: usize = 512;
const PREEMPH: f32 = 0.97;
const LOW_FREQ: f32 = 20.0;

fn mel(f: f32) -> f32 {
    1127.0 * (1.0 + f / 700.0).ln()
}

/// Triangular mel filters over NFFT/2 bins (Nyquist bin excluded, as in Kaldi).
fn mel_banks() -> Vec<Vec<f32>> {
    let nyq = SAMPLE_RATE as f32 / 2.0;
    let (lo, hi) = (mel(LOW_FREQ), mel(nyq));
    let delta = (hi - lo) / (NUM_BINS as f32 + 1.0);
    let bin_hz = SAMPLE_RATE as f32 / NFFT as f32;
    (0..NUM_BINS)
        .map(|b| {
            let l = lo + b as f32 * delta;
            let c = lo + (b as f32 + 1.0) * delta;
            let r = lo + (b as f32 + 2.0) * delta;
            (0..NFFT / 2)
                .map(|i| {
                    let m = mel(i as f32 * bin_hz);
                    if m > l && m < r {
                        if m <= c { (m - l) / (c - l) } else { (r - m) / (r - c) }
                    } else {
                        0.0
                    }
                })
                .collect()
        })
        .collect()
}

pub struct Fbank {
    window: Vec<f32>,
    banks: Vec<Vec<f32>>,
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
}

impl Default for Fbank {
    fn default() -> Self {
        Self::new()
    }
}

impl Fbank {
    pub fn new() -> Self {
        let window = (0..FRAME_LEN)
            .map(|i| (0.5 - 0.5 * (2.0 * PI * i as f32 / (FRAME_LEN as f32 - 1.0)).cos()).powf(0.85))
            .collect();
        Self { window, banks: mel_banks(), fft: FftPlanner::new().plan_fft_forward(NFFT) }
    }

    pub fn num_frames(n_samples: usize) -> usize {
        if n_samples < FRAME_LEN { 0 } else { 1 + (n_samples - FRAME_LEN) / FRAME_SHIFT }
    }

    /// `samples` in [-1, 1]. Returns frames x 80 log-mel energies (no mean normalisation).
    pub fn compute(&self, samples: &[f32]) -> Vec<[f32; NUM_BINS]> {
        let n = Self::num_frames(samples.len());
        let mut out = Vec::with_capacity(n);
        let mut buf = vec![Complex::new(0.0f32, 0.0); NFFT];
        let mut frame = [0.0f32; FRAME_LEN];
        for f in 0..n {
            frame.copy_from_slice(&samples[f * FRAME_SHIFT..f * FRAME_SHIFT + FRAME_LEN]);
            let mean = frame.iter().sum::<f32>() / FRAME_LEN as f32;
            frame.iter_mut().for_each(|x| *x -= mean);
            for i in (1..FRAME_LEN).rev() {
                frame[i] -= PREEMPH * frame[i - 1];
            }
            frame[0] -= PREEMPH * frame[0];
            for (i, c) in buf.iter_mut().enumerate() {
                *c = if i < FRAME_LEN { Complex::new(frame[i] * self.window[i], 0.0) } else { Complex::new(0.0, 0.0) };
            }
            self.fft.process(&mut buf);
            let power: Vec<f32> = buf[..NFFT / 2].iter().map(|c| c.norm_sqr()).collect();
            let mut row = [0.0f32; NUM_BINS];
            for (b, w) in self.banks.iter().enumerate() {
                let e: f32 = w.iter().zip(&power).map(|(a, p)| a * p).sum();
                row[b] = e.max(f32::EPSILON).ln();
            }
            out.push(row);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_frames_and_finite() {
        let n = 16_000;
        let s: Vec<f32> = (0..n).map(|i| (2.0 * PI * 440.0 * i as f32 / 16_000.0).sin() * 0.5).collect();
        let f = Fbank::new().compute(&s);
        assert_eq!(f.len(), 1 + (n - 400) / 160);
        assert!(f.iter().all(|r| r.iter().all(|v| v.is_finite())));
        // 440 Hz energy should exceed high-frequency energy
        assert!(f[10][10] > f[10][70]);
    }

    #[test]
    fn too_short_gives_no_frames() {
        assert!(Fbank::new().compute(&[0.0; 100]).is_empty());
    }
}
