//! Lightweight adaptive energy VAD that cuts a continuous stream into utterances.
//! Tracks a noise floor per stream, so it works for quiet rooms and noisy cafés alike.

use crate::SAMPLE_RATE;

const FRAME: usize = 480; // 30 ms at 16 kHz
const MS_PER_FRAME: u64 = 30;

#[derive(Debug, Clone)]
pub struct VadConfig {
    /// How far above the noise floor (in dB) counts as speech.
    pub threshold_db: f32,
    /// Absolute floor; anything quieter is never speech.
    pub min_rms: f32,
    /// Silence needed to end an utterance.
    pub hangover_ms: u64,
    /// Audio kept before speech onset.
    pub preroll_ms: u64,
    pub min_utterance_ms: u64,
    /// Utterances are force-split near this length so live text keeps flowing.
    pub max_utterance_ms: u64,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            threshold_db: 9.0,
            min_rms: 0.004,
            hangover_ms: 700,
            preroll_ms: 240,
            min_utterance_ms: 400,
            max_utterance_ms: 18_000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Utterance {
    /// Offset from the start of the stream.
    pub start_ms: u64,
    pub samples: Vec<f32>,
}

impl Utterance {
    pub fn end_ms(&self) -> u64 {
        self.start_ms + self.samples.len() as u64 * 1000 / SAMPLE_RATE as u64
    }
}

pub struct Vad {
    cfg: VadConfig,
    pending: Vec<f32>,
    noise_floor: f32,
    frames_seen: u64,
    preroll: std::collections::VecDeque<Vec<f32>>,
    current: Option<(u64, Vec<f32>)>,
    silence_frames: u64,
    voiced_frames: u64,
    /// Last frame RMS, handy for level meters.
    pub level: f32,
}

fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

impl Vad {
    pub fn new(cfg: VadConfig) -> Self {
        Self {
            cfg,
            pending: Vec::with_capacity(FRAME * 4),
            noise_floor: 0.002,
            frames_seen: 0,
            preroll: Default::default(),
            current: None,
            silence_frames: 0,
            voiced_frames: 0,
            level: 0.0,
        }
    }

    /// Feed samples; returns any utterances that completed.
    pub fn push(&mut self, samples: &[f32]) -> Vec<Utterance> {
        self.pending.extend_from_slice(samples);
        let mut out = Vec::new();
        let mut off = 0;
        while self.pending.len() - off >= FRAME {
            let frame = self.pending[off..off + FRAME].to_vec();
            off += FRAME;
            if let Some(u) = self.process_frame(frame) {
                out.push(u);
            }
        }
        self.pending.drain(..off);
        out
    }

    /// Flush whatever is in progress (end of recording).
    pub fn flush(&mut self) -> Option<Utterance> {
        let rest = std::mem::take(&mut self.pending);
        if let Some((_, buf)) = self.current.as_mut() {
            buf.extend_from_slice(&rest);
        }
        self.finish()
    }

    fn finish(&mut self) -> Option<Utterance> {
        let (start, samples) = self.current.take()?;
        let voiced_ms = self.voiced_frames * MS_PER_FRAME;
        self.voiced_frames = 0;
        self.silence_frames = 0;
        if voiced_ms < self.cfg.min_utterance_ms {
            return None;
        }
        Some(Utterance { start_ms: start, samples })
    }

    fn process_frame(&mut self, frame: Vec<f32>) -> Option<Utterance> {
        let idx = self.frames_seen;
        self.frames_seen += 1;
        let e = rms(&frame);
        self.level = e;
        let ratio = 10f32.powf(self.cfg.threshold_db / 20.0);
        let is_speech = e > self.cfg.min_rms && e > self.noise_floor * ratio;
        // Noise floor: fall quickly, rise slowly (only on non-speech frames).
        if !is_speech {
            let a = if e < self.noise_floor { 0.2 } else { 0.01 };
            self.noise_floor = (1.0 - a) * self.noise_floor + a * e.max(1e-5);
        }

        let mut done = None;
        match self.current.as_mut() {
            None => {
                if is_speech {
                    let mut buf: Vec<f32> = self.preroll.drain(..).flatten().collect();
                    let pre_ms = buf.len() as u64 * 1000 / SAMPLE_RATE as u64;
                    buf.extend_from_slice(&frame);
                    let start = (idx * MS_PER_FRAME).saturating_sub(pre_ms);
                    self.current = Some((start, buf));
                    self.voiced_frames = 1;
                    self.silence_frames = 0;
                    return None;
                }
            }
            Some((start, buf)) => {
                buf.extend_from_slice(&frame);
                if is_speech {
                    self.voiced_frames += 1;
                    self.silence_frames = 0;
                } else {
                    self.silence_frames += 1;
                }
                let len_ms = buf.len() as u64 * 1000 / SAMPLE_RATE as u64;
                let start = *start;
                if self.silence_frames * MS_PER_FRAME >= self.cfg.hangover_ms {
                    // Trim most of the trailing silence.
                    let keep_tail = (200 / MS_PER_FRAME) as usize * FRAME;
                    let trim = (self.silence_frames as usize * FRAME).saturating_sub(keep_tail);
                    let new_len = buf.len().saturating_sub(trim);
                    buf.truncate(new_len);
                    done = self.finish();
                } else if len_ms >= self.cfg.max_utterance_ms && !is_speech {
                    done = self.finish();
                } else if len_ms >= self.cfg.max_utterance_ms + 4_000 {
                    // Never let one chunk run away.
                    done = self.finish();
                    let _ = start;
                }
            }
        }
        if self.current.is_none() {
            self.preroll.push_back(frame);
            let max = (self.cfg.preroll_ms / MS_PER_FRAME) as usize;
            while self.preroll.len() > max {
                self.preroll.pop_front();
            }
        }
        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(ms: u64, amp: f32) -> Vec<f32> {
        let n = (ms * 16) as usize;
        (0..n).map(|i| amp * (i as f32 * 0.07).sin()).collect()
    }

    #[test]
    fn splits_two_utterances() {
        let mut v = Vad::new(VadConfig::default());
        let mut audio = vec![0.0005f32; 16_000];
        audio.extend(tone(1200, 0.3));
        audio.extend(vec![0.0005; 16_000]);
        audio.extend(tone(900, 0.3));
        audio.extend(vec![0.0005; 16_000]);
        let mut us = v.push(&audio);
        us.extend(v.flush());
        assert_eq!(us.len(), 2, "got {:?}", us.iter().map(|u| (u.start_ms, u.end_ms())).collect::<Vec<_>>());
        assert!(us[0].start_ms >= 700 && us[0].start_ms <= 1000);
    }

    #[test]
    fn ignores_clicks() {
        let mut v = Vad::new(VadConfig::default());
        let mut audio = vec![0.0005f32; 16_000];
        audio.extend(tone(90, 0.5));
        audio.extend(vec![0.0005; 16_000]);
        let mut us = v.push(&audio);
        us.extend(v.flush());
        assert!(us.is_empty());
    }
}
