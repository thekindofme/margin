//! Record N seconds from mic + system audio into two 16 kHz WAVs.
//! Usage: capture_test <seconds> <out_dir>
use crossbeam_channel::{unbounded, RecvTimeoutError};
use margin_core::audio;
use margin_core::types::{AudioSource, SAMPLE_RATE};
use std::time::{Duration, Instant};

fn writer(path: &std::path::Path) -> anyhow::Result<hound::WavWriter<std::io::BufWriter<std::fs::File>>> {
    Ok(hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?)
}

fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        0.0
    } else {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let mut args = std::env::args().skip(1);
    let secs: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(3);
    let dir = std::path::PathBuf::from(args.next().unwrap_or_else(|| "cap".into()));
    std::fs::create_dir_all(&dir)?;

    println!("input devices: {:?}", audio::list_input_devices());
    println!("system audio permitted: {}", audio::system_audio_permitted());

    let (tx, rx) = unbounded();
    let mic = match audio::start_mic(None, tx.clone()) {
        Ok(h) => Some(h),
        Err(e) => {
            eprintln!("mic failed: {e:#}");
            None
        }
    };
    let sys = match audio::start_system(tx.clone()) {
        Ok(h) => Some(h),
        Err(e) => {
            eprintln!("system failed: {e:#}");
            None
        }
    };
    drop(tx);

    let mut mic_w = writer(&dir.join("mic.wav"))?;
    let mut sys_w = writer(&dir.join("system.wav"))?;
    let (mut mic_n, mut sys_n) = (0usize, 0usize);
    let start = Instant::now();
    let end = start + Duration::from_secs(secs);
    let mut sec_mark = 1u64;
    let (mut mic_acc, mut sys_acc): (Vec<f32>, Vec<f32>) = (Vec::new(), Vec::new());
    while Instant::now() < end {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(f) => {
                let (w, n, acc) = match f.source {
                    AudioSource::Mic => (&mut mic_w, &mut mic_n, &mut mic_acc),
                    AudioSource::System => (&mut sys_w, &mut sys_n, &mut sys_acc),
                };
                for s in &f.samples {
                    w.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
                }
                *n += f.samples.len();
                acc.extend_from_slice(&f.samples);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if start.elapsed().as_secs() >= sec_mark {
            println!(
                "t={sec_mark}s  mic rms={:.4} ({} smp)  system rms={:.4} ({} smp)",
                rms(&mic_acc), mic_acc.len(), rms(&sys_acc), sys_acc.len()
            );
            mic_acc.clear();
            sys_acc.clear();
            sec_mark += 1;
        }
    }
    if let Some(h) = mic {
        h.stop();
    }
    if let Some(h) = sys {
        h.stop();
    }
    mic_w.finalize()?;
    sys_w.finalize()?;
    println!("total mic samples={mic_n} system samples={sys_n}; wrote {}", dir.display());
    Ok(())
}
