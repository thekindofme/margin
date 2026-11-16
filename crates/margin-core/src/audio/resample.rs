//! Downmix + resample helpers shared by the mic and system capture paths.
//! Everything is turned into 16 kHz mono f32 blocks of ~100 ms.
use crate::types::SAMPLE_RATE;
use anyhow::{anyhow, Result};
use rubato::{FftFixedIn, Resampler};

/// Samples per emitted block (100 ms at 16 kHz).
pub const BLOCK: usize = (SAMPLE_RATE as usize) / 10;

/// Average interleaved `channels`-wide frames into mono, appending to `out`.
pub fn downmix_interleaved(data: &[f32], channels: usize, out: &mut Vec<f32>) {
    if channels <= 1 {
        out.extend_from_slice(data);
        return;
    }
    let inv = 1.0 / channels as f32;
    out.extend(data.chunks_exact(channels).map(|f| f.iter().sum::<f32>() * inv));
}

/// Average planar channels (equal length) into mono, appending to `out`.
pub fn downmix_planar(planes: &[&[f32]], out: &mut Vec<f32>) {
    let Some(len) = planes.iter().map(|p| p.len()).min() else { return };
    if planes.len() == 1 {
        out.extend_from_slice(&planes[0][..len]);
        return;
    }
    let inv = 1.0 / planes.len() as f32;
    out.extend((0..len).map(|i| planes.iter().map(|p| p[i]).sum::<f32>() * inv));
}

/// Streaming mono resampler to 16 kHz that emits fixed-size blocks.
pub struct Mono16k {
    rate: u32,
    resampler: Option<FftFixedIn<f32>>,
    in_buf: Vec<f32>,
    out_chunk: Vec<Vec<f32>>,
    pending: Vec<f32>,
}

impl Mono16k {
    pub fn new(input_rate: u32) -> Result<Self> {
        if input_rate == 0 {
            return Err(anyhow!("invalid input sample rate 0"));
        }
        let resampler = if input_rate == SAMPLE_RATE {
            None
        } else {
            // ~20 ms input chunks.
            let chunk = (input_rate as usize / 50).max(64);
            Some(
                FftFixedIn::<f32>::new(input_rate as usize, SAMPLE_RATE as usize, chunk, 2, 1)
                    .map_err(|e| anyhow!("resampler init ({input_rate} Hz): {e}"))?,
            )
        };
        let out_chunk = match &resampler {
            Some(r) => r.output_buffer_allocate(true),
            None => Vec::new(),
        };
        Ok(Self {
            rate: input_rate,
            resampler,
            in_buf: Vec::new(),
            out_chunk,
            pending: Vec::with_capacity(BLOCK * 2),
        })
    }

    pub fn input_rate(&self) -> u32 {
        self.rate
    }

    /// Feed mono samples at the input rate; `emit` is called for each full 100 ms block.
    pub fn push(&mut self, mono: &[f32], mut emit: impl FnMut(Vec<f32>)) {
        match self.resampler.as_mut() {
            None => self.pending.extend_from_slice(mono),
            Some(r) => {
                self.in_buf.extend_from_slice(mono);
                loop {
                    let need = r.input_frames_next();
                    if self.in_buf.len() < need {
                        break;
                    }
                    let input = [&self.in_buf[..need]];
                    match r.process_into_buffer(&input, &mut self.out_chunk, None) {
                        Ok((_, produced)) => {
                            self.pending.extend_from_slice(&self.out_chunk[0][..produced])
                        }
                        Err(e) => {
                            log::error!("resample error: {e}");
                        }
                    }
                    self.in_buf.drain(..need);
                }
            }
        }
        let mut start = 0;
        while self.pending.len() - start >= BLOCK {
            emit(self.pending[start..start + BLOCK].to_vec());
            start += BLOCK;
        }
        if start > 0 {
            self.pending.drain(..start);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    #[test]
    fn stereo_48k_sine_to_16k_mono() {
        let secs = 3usize;
        let n = 48_000 * secs;
        let mut inter = Vec::with_capacity(n * 2);
        for i in 0..n {
            let s = (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48_000.0).sin() * 0.5;
            inter.push(s);
            inter.push(s);
        }
        let mut mono = Vec::new();
        downmix_interleaved(&inter, 2, &mut mono);
        assert_eq!(mono.len(), n);

        let mut rs = Mono16k::new(48_000).unwrap();
        let mut out: Vec<f32> = Vec::new();
        // feed in odd-sized pieces to exercise buffering
        for piece in mono.chunks(777) {
            rs.push(piece, |b| {
                assert_eq!(b.len(), BLOCK);
                out.extend(b)
            });
        }
        let expected = 16_000 * secs;
        // within the resampler delay + one partial block
        assert!(out.len() <= expected && out.len() + 2 * BLOCK >= expected, "len {}", out.len());
        let tail = &out[out.len() / 2..];
        let r = rms(tail);
        let want = 0.5 / 2f32.sqrt();
        assert!((r - want).abs() < 0.02, "rms {r} vs {want}");
    }

    #[test]
    fn passthrough_16k_and_planar() {
        let a = vec![1.0f32; 3200];
        let b = vec![0.0f32; 3200];
        let mut mono = Vec::new();
        downmix_planar(&[&a, &b], &mut mono);
        assert!(mono.iter().all(|v| (*v - 0.5).abs() < 1e-6));
        let mut rs = Mono16k::new(16_000).unwrap();
        let mut blocks = 0;
        rs.push(&mono, |_| blocks += 1);
        assert_eq!(blocks, 2);
    }
}
