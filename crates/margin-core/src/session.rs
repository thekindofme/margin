//! A live recording session: capture → VAD → transcription → speaker labels → sink.
//!
//! Threads: capture callbacks push 16 kHz frames; a segmenter thread runs one VAD per source;
//! a worker thread transcribes utterances in order, labels speakers and removes echo
//! (your speakers leaking into your mic when you're not on headphones).
use crate::audio::{self, CaptureHandle};
use crate::diarize::{SpeakerClusterer, SpeakerEmbedder};
use crate::stt::Transcriber;
use crate::types::{AudioFrame, AudioSource, Segment, SAMPLE_RATE};
use crate::vad::{Utterance, Vad, VadConfig};
use anyhow::Result;
use crossbeam_channel::{bounded, unbounded, Receiver, RecvTimeoutError};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub trait SessionSink: Send + Sync {
    fn segment(&self, seg: Segment);
    /// RMS levels (0..1) roughly every 100 ms.
    fn levels(&self, mic: f32, system: f32);
    fn warning(&self, msg: &str);
}

#[derive(Clone)]
pub struct SessionConfig {
    pub capture_mic: bool,
    pub capture_system: bool,
    pub mic_device: Option<String>,
    /// Added to every timestamp (for resuming a meeting).
    pub offset_ms: u64,
    pub clusterer: Option<SpeakerClusterer>,
    pub vad: VadConfig,
    /// Opt-in: keep a local recording at this path (without extension).
    pub record_to: Option<std::path::PathBuf>,
    /// New remote speakers are numbered after this (s{base+1}, …) when resuming a meeting.
    pub speaker_base: usize,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            capture_mic: true,
            capture_system: true,
            mic_device: None,
            offset_ms: 0,
            clusterer: None,
            vad: VadConfig::default(),
            record_to: None,
            speaker_base: 0,
        }
    }
}

pub struct SessionResult {
    pub duration_ms: u64,
    pub clusterer: SpeakerClusterer,
}

pub struct Session {
    captures: Vec<CaptureHandle>,
    paused: Arc<AtomicBool>,
    elapsed: Arc<AtomicU64>,
    segmenter: Option<JoinHandle<()>>,
    worker: Option<JoinHandle<SpeakerClusterer>>,
    started: Instant,
    pub system_active: bool,
}

struct SourcedUtterance {
    source: AudioSource,
    utt: Utterance,
}

impl Session {
    pub fn start(
        cfg: SessionConfig,
        engine: Arc<Mutex<Transcriber>>,
        embedder: Option<Arc<Mutex<SpeakerEmbedder>>>,
        sink: Arc<dyn SessionSink>,
    ) -> Result<Self> {
        let (frame_tx, frame_rx) = bounded::<AudioFrame>(512);
        let mut captures = Vec::new();
        if cfg.capture_mic {
            captures.push(audio::start_mic(cfg.mic_device.clone(), frame_tx.clone())?);
        }
        let mut system_active = false;
        if cfg.capture_system {
            match audio::start_system(frame_tx.clone()) {
                Ok(h) => {
                    captures.push(h);
                    system_active = true;
                }
                Err(e) => sink.warning(&format!(
                    "Couldn't capture other participants' audio ({e}). Only your microphone is being transcribed."
                )),
            }
        }
        drop(frame_tx);
        anyhow::ensure!(!captures.is_empty(), "no audio source could be started");
        Self::run_pipeline(cfg, frame_rx, captures, system_active, engine, embedder, sink)
    }

    /// Run the pipeline on frames from any source (files, tests). Capture ends when all
    /// senders of `frame_rx` are dropped.
    pub fn from_frames(
        cfg: SessionConfig,
        frame_rx: Receiver<AudioFrame>,
        engine: Arc<Mutex<Transcriber>>,
        embedder: Option<Arc<Mutex<SpeakerEmbedder>>>,
        sink: Arc<dyn SessionSink>,
    ) -> Result<Self> {
        Self::run_pipeline(cfg, frame_rx, Vec::new(), true, engine, embedder, sink)
    }

    /// Like `from_frames`, with handles that are stopped (so their senders close) on `stop()`.
    pub fn from_frames_with(
        cfg: SessionConfig,
        frame_rx: Receiver<AudioFrame>,
        handles: Vec<CaptureHandle>,
        engine: Arc<Mutex<Transcriber>>,
        embedder: Option<Arc<Mutex<SpeakerEmbedder>>>,
        sink: Arc<dyn SessionSink>,
    ) -> Result<Self> {
        Self::run_pipeline(cfg, frame_rx, handles, true, engine, embedder, sink)
    }

    fn run_pipeline(
        cfg: SessionConfig,
        frame_rx: Receiver<AudioFrame>,
        captures: Vec<CaptureHandle>,
        system_active: bool,
        engine: Arc<Mutex<Transcriber>>,
        embedder: Option<Arc<Mutex<SpeakerEmbedder>>>,
        sink: Arc<dyn SessionSink>,
    ) -> Result<Self> {
        let paused = Arc::new(AtomicBool::new(false));
        let elapsed = Arc::new(AtomicU64::new(0));
        let (utt_tx, utt_rx) = unbounded::<SourcedUtterance>();

        let seg_sink = sink.clone();
        let seg_paused = paused.clone();
        let seg_elapsed = elapsed.clone();
        let vad_cfg = cfg.vad.clone();
        let offset = cfg.offset_ms;
        let mut recorder = cfg.record_to.as_deref().and_then(|p| match crate::audio_rec::AudioRecorder::create(p) {
            Ok(r) => Some(r),
            Err(e) => {
                log::warn!("not keeping audio: {e}");
                None
            }
        });
        let segmenter = std::thread::Builder::new().name("margin-vad".into()).spawn(move || {
            let mut mic_vad = Vad::new(vad_cfg.clone());
            // Remote audio is cleaner (no room noise): a slightly lower threshold catches quiet talkers.
            let mut sys_vad = Vad::new(VadConfig { threshold_db: 7.0, min_rms: 0.002, ..vad_cfg });
            let mut mic_samples = 0u64;
            // macOS delivers exact digital silence when the app lacks microphone permission.
            let mut mic_zero_run = 0u64;
            let mut warned_silent = false;
            // Some mics / voice processing emit exact zeros during quiet moments, so only a mic
            // that has never produced any sound counts as blocked.
            let mut mic_ever_live = false;
            // A mic that never gets above a whisper is probably the wrong device.
            let mut mic_max_peak = 0f32;
            let mut warned_quiet = false;
            let mut last_level = Instant::now();
            let (mut mic_peak, mut sys_peak) = (0f32, 0f32);
            let send = |source, mut u: Utterance| {
                u.start_ms += offset;
                let _ = utt_tx.send(SourcedUtterance { source, utt: u });
            };
            while let Ok(frame) = frame_rx.recv() {
                if seg_paused.load(Ordering::Relaxed) {
                    continue;
                }
                if let Some(r) = recorder.as_mut() {
                    r.write(frame.source, &frame.samples);
                }
                let lvl = frame.samples.iter().fold(0f32, |m, v| m.max(v.abs()));
                match frame.source {
                    AudioSource::Mic => {
                        mic_samples += frame.samples.len() as u64;
                        if frame.samples.iter().all(|v| *v == 0.0) {
                            mic_zero_run += frame.samples.len() as u64;
                            if !warned_silent && !mic_ever_live && mic_zero_run > 4 * SAMPLE_RATE as u64 {
                                warned_silent = true;
                                seg_sink.warning("MIC_SILENT: Your microphone is sending no sound at all.");
                            }
                        } else {
                            mic_zero_run = 0;
                            mic_ever_live = true;
                        }
                        mic_max_peak = mic_max_peak.max(lvl);
                        if !warned_quiet && !warned_silent && mic_ever_live && mic_max_peak < 0.02
                            && mic_samples > 20 * SAMPLE_RATE as u64
                        {
                            warned_quiet = true;
                            seg_sink.warning("MIC_QUIET: Your microphone has been nearly silent since recording started.");
                        }
                        seg_elapsed.store(mic_samples * 1000 / SAMPLE_RATE as u64, Ordering::Relaxed);
                        mic_peak = mic_peak.max(lvl);
                        for u in mic_vad.push(&frame.samples) {
                            send(AudioSource::Mic, u);
                        }
                    }
                    AudioSource::System => {
                        sys_peak = sys_peak.max(lvl);
                        for u in sys_vad.push(&frame.samples) {
                            send(AudioSource::System, u);
                        }
                    }
                }
                if last_level.elapsed() >= Duration::from_millis(100) {
                    seg_sink.levels(mic_peak.min(1.0), sys_peak.min(1.0));
                    mic_peak = 0.0;
                    sys_peak = 0.0;
                    last_level = Instant::now();
                }
            }
            if let Some(u) = mic_vad.flush() {
                send(AudioSource::Mic, u);
            }
            if let Some(u) = sys_vad.flush() {
                send(AudioSource::System, u);
            }
            if let Some(r) = recorder.take() {
                match r.finish() {
                    Ok(p) => log::info!("kept meeting audio: {}", p.display()),
                    Err(e) => log::warn!("saving meeting audio failed: {e}"),
                }
            }
        })?;

        let clusterer = cfg
            .clusterer
            .clone()
            .unwrap_or_else(|| SpeakerClusterer::new(crate::diarize::DEFAULT_THRESHOLD));
        let worker_sink = sink;
        let speaker_base = cfg.speaker_base;
        let worker = std::thread::Builder::new()
            .name("margin-stt".into())
            .spawn(move || worker_loop(utt_rx, engine, embedder, clusterer, speaker_base, worker_sink))?;

        Ok(Self {
            captures,
            paused,
            elapsed,
            segmenter: Some(segmenter),
            worker: Some(worker),
            started: Instant::now(),
            system_active,
        })
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::Relaxed);
    }
    pub fn resume(&self) {
        self.paused.store(false, Ordering::Relaxed);
    }
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }
    /// Recorded (unpaused) audio length in ms.
    pub fn elapsed_ms(&self) -> u64 {
        let e = self.elapsed.load(Ordering::Relaxed);
        if e == 0 && self.captures.len() == 1 && !self.is_paused() {
            // System-only capture: fall back to wall clock.
            return self.started.elapsed().as_millis() as u64;
        }
        e
    }

    /// Stop capture and wait for the transcription backlog to drain.
    pub fn stop(mut self) -> SessionResult {
        let duration_ms = self.elapsed_ms();
        for c in self.captures.drain(..) {
            c.stop();
        }
        if let Some(h) = self.segmenter.take() {
            let _ = h.join();
        }
        let clusterer = self
            .worker
            .take()
            .and_then(|h| h.join().ok())
            .unwrap_or_else(|| SpeakerClusterer::new(crate::diarize::DEFAULT_THRESHOLD));
        SessionResult { duration_ms, clusterer }
    }
}

fn words(t: &str) -> Vec<String> {
    t.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 1)
        .map(|w| w.to_lowercase())
        .collect()
}

/// Share of the mic text's words that also appear in the system text.
fn overlap(mic: &str, sys: &str) -> f32 {
    let m = words(mic);
    if m.is_empty() {
        return 0.0;
    }
    let s: std::collections::HashSet<String> = words(sys).into_iter().collect();
    m.iter().filter(|w| s.contains(*w)).count() as f32 / m.len() as f32
}

fn is_echo(mic: &Segment, sys: &Segment) -> bool {
    let overlaps_in_time = mic.start_ms < sys.end_ms + 1500 && sys.start_ms < mic.end_ms + 1500;
    overlaps_in_time && overlap(&mic.text, &sys.text) >= 0.6
}

/// Utterances shorter than this can't create a new speaker (see worker_loop).
const MIN_NEW_SPEAKER_SECS: f32 = 2.5;

fn worker_loop(
    rx: Receiver<SourcedUtterance>,
    engine: Arc<Mutex<Transcriber>>,
    embedder: Option<Arc<Mutex<SpeakerEmbedder>>>,
    mut clusterer: SpeakerClusterer,
    speaker_base: usize,
    sink: Arc<dyn SessionSink>,
) -> SpeakerClusterer {
    // Mic segments wait briefly so overlapping system speech can mark them as echo.
    let mut pending_mic: Vec<(Segment, Instant)> = Vec::new();
    let mut recent_sys: std::collections::VecDeque<Segment> = Default::default();
    let mut last_sys_speaker = "s1".to_string();
    const HOLD: Duration = Duration::from_millis(2500);

    let flush = |pending: &mut Vec<(Segment, Instant)>, recent: &std::collections::VecDeque<Segment>, all: bool| {
        let mut keep = Vec::new();
        for (seg, t) in pending.drain(..) {
            if recent.iter().any(|s| is_echo(&seg, s)) {
                log::debug!("dropping echoed mic segment: {}", seg.text);
                continue;
            }
            if all || t.elapsed() >= HOLD {
                sink.segment(seg);
            } else {
                keep.push((seg, t));
            }
        }
        *pending = keep;
    };

    loop {
        let item = match rx.recv_timeout(Duration::from_millis(400)) {
            Ok(i) => Some(i),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        if let Some(SourcedUtterance { source, utt }) = item {
            let text = match engine.lock().transcribe(&utt.samples) {
                Ok(t) => t,
                Err(e) => {
                    sink.warning(&format!("Transcription error: {e}"));
                    String::new()
                }
            };
            if !text.is_empty() {
                let speaker = match source {
                    AudioSource::Mic => "me".to_string(),
                    AudioSource::System => {
                        let dur = utt.samples.len() as f32 / SAMPLE_RATE as f32;
                        match &embedder {
                            // Short clips give noisy voice prints: they may only match someone already
                            // heard. Only a longer utterance can introduce a new speaker.
                            Some(emb) if dur >= 0.8 => match emb.lock().embed(&utt.samples) {
                                Ok(v) if dur < MIN_NEW_SPEAKER_SECS => match clusterer.nearest(&v) {
                                    Some(i) => format!("s{}", i + 1 + speaker_base),
                                    None => format!("s{}", clusterer.assign(&v) + 1 + speaker_base),
                                },
                                Ok(v) => format!("s{}", clusterer.assign(&v) + 1 + speaker_base),
                                Err(_) => last_sys_speaker.clone(),
                            },
                            Some(_) => last_sys_speaker.clone(),
                            None => "them".into(),
                        }
                    }
                };
                let seg = Segment {
                    id: 0,
                    start_ms: utt.start_ms,
                    end_ms: utt.end_ms(),
                    source,
                    speaker: speaker.clone(),
                    text,
                };
                match source {
                    AudioSource::Mic => pending_mic.push((seg, Instant::now())),
                    AudioSource::System => {
                        last_sys_speaker = speaker;
                        recent_sys.push_back(seg.clone());
                        while recent_sys.len() > 20 {
                            recent_sys.pop_front();
                        }
                        sink.segment(seg);
                    }
                }
            }
        }
        flush(&mut pending_mic, &recent_sys, false);
    }
    flush(&mut pending_mic, &recent_sys, true);
    clusterer
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(src: AudioSource, a: u64, b: u64, t: &str) -> Segment {
        Segment { id: 0, start_ms: a, end_ms: b, source: src, speaker: String::new(), text: t.into() }
    }

    #[test]
    fn echo_detection() {
        let sys = seg(AudioSource::System, 1000, 5000, "We should ship the beta on Friday after QA signs off");
        let echo = seg(AudioSource::Mic, 1200, 5100, "we should ship the beta on friday after qa signs");
        let real = seg(AudioSource::Mic, 5200, 7000, "Sounds good, I'll tell the design team");
        assert!(is_echo(&echo, &sys));
        assert!(!is_echo(&real, &sys));
    }
}
