//! Audio capture. OWNER: audio worker.
//! Mic via cpal, system audio (everyone else on the call) via ScreenCaptureKit on macOS.
//! Every frame sent downstream is 16 kHz mono f32 (see `crate::SAMPLE_RATE`).
use crate::types::{AudioFrame, AudioSource};
use anyhow::Result;

mod mic;
mod resample;
#[cfg(target_os = "macos")]
mod system;
use crossbeam_channel::Sender;

/// Stops capture when dropped or when `stop()` is called.
pub struct CaptureHandle {
    stop: Option<Box<dyn FnOnce() + Send>>,
}

impl CaptureHandle {
    pub fn new(stop: impl FnOnce() + Send + 'static) -> Self {
        Self { stop: Some(Box::new(stop)) }
    }
    pub fn stop(mut self) {
        if let Some(f) = self.stop.take() {
            f()
        }
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        if let Some(f) = self.stop.take() {
            f()
        }
    }
}

/// Names of available input devices (default first, deduped).
pub fn list_input_devices() -> Vec<String> {
    mic::list_input_devices()
}

/// Names of the current system default input and output devices.
pub fn default_devices() -> (Option<String>, Option<String>) {
    use cpal::traits::{DeviceTrait, HostTrait};
    // CoreAudio can block while a permission prompt is pending: never hang the caller.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let host = cpal::default_host();
        let i = host.default_input_device().and_then(|d| d.name().ok());
        let o = host.default_output_device().and_then(|d| d.name().ok());
        let _ = tx.send((i, o));
    });
    rx.recv_timeout(std::time::Duration::from_secs(3)).unwrap_or((None, None))
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MicProbe {
    pub name: String,
    /// Peak absolute sample over the probe window (0..1).
    pub peak: f32,
    /// Exact zeros only: the device is muted, disconnected or blocked.
    pub digital_silence: bool,
    pub error: Option<String>,
}

/// Listen on every input device at once for `ms` milliseconds and report what each one hears.
/// Used to find the mic that actually picks up the user when the chosen one is silent.
pub fn probe_inputs(ms: u64) -> Vec<MicProbe> {
    let names = list_input_devices();
    let mut probes = Vec::new();
    for name in names {
        let (tx, rx) = crossbeam_channel::bounded::<AudioFrame>(256);
        match start_mic(Some(name.clone()), tx) {
            Ok(h) => probes.push((name, Some((h, rx)), None)),
            Err(e) => probes.push((name, None, Some(e.to_string()))),
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(ms));
    probes
        .into_iter()
        .map(|(name, cap, error)| match cap {
            Some((h, rx)) => {
                h.stop();
                let mut peak = 0f32;
                let mut any = false;
                let mut nonzero = false;
                while let Ok(f) = rx.try_recv() {
                    any = true;
                    for v in &f.samples {
                        peak = peak.max(v.abs());
                        nonzero |= *v != 0.0;
                    }
                }
                MicProbe { name, peak, digital_silence: any && !nonzero, error: if any { None } else { Some("no audio".into()) } }
            }
            None => MicProbe { name, peak: 0.0, digital_silence: false, error },
        })
        .collect()
}

/// Where a running capture reports that it failed (device unplugged, stream stopped by the OS).
pub type CaptureErrors = Sender<(AudioSource, String)>;

/// Start capturing the microphone. `device` = None uses the system default input.
pub fn start_mic(device: Option<String>, tx: Sender<AudioFrame>) -> Result<CaptureHandle> {
    mic::start_mic(device, tx, None)
}

/// Like `start_mic`, reporting stream errors (e.g. the device went away) to `errors`.
pub fn start_mic_watched(device: Option<String>, tx: Sender<AudioFrame>, errors: CaptureErrors) -> Result<CaptureHandle> {
    mic::start_mic(device, tx, Some(errors))
}

/// Start capturing system output audio (other meeting participants). macOS only.
pub fn start_system(tx: Sender<AudioFrame>) -> Result<CaptureHandle> {
    start_system_inner(tx, None)
}

/// Like `start_system`, reporting an unexpected stream stop to `errors`.
pub fn start_system_watched(tx: Sender<AudioFrame>, errors: CaptureErrors) -> Result<CaptureHandle> {
    start_system_inner(tx, Some(errors))
}

fn start_system_inner(tx: Sender<AudioFrame>, errors: Option<CaptureErrors>) -> Result<CaptureHandle> {
    #[cfg(target_os = "macos")]
    {
        system::start(tx, errors)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (tx, errors);
        Err(anyhow::anyhow!("system audio capture is only supported on macOS"))
    }
}

/// Whether the app may capture system audio (Screen Recording permission on macOS).
pub fn system_audio_permitted() -> bool {
    #[cfg(target_os = "macos")]
    {
        system::permitted()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Ask macOS for system audio permission (opens the prompt / Settings once).
pub fn request_system_audio_permission() -> bool {
    #[cfg(target_os = "macos")]
    {
        system::request_permission()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}
