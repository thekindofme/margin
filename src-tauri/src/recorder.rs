//! Starting/stopping recordings and the after-meeting AI pass.
use crate::state::{ActiveRecording, AppState};
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use margin_core::diarize::{SpeakerClusterer, SpeakerEmbedder};
use margin_core::llm;
use margin_core::session::{Session, SessionConfig, SessionSink};
use margin_core::store::Meeting;
use margin_core::stt::Transcriber;
use margin_core::{catalog, models, CalEvent, Segment};
use parking_lot::Mutex;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

pub fn notify(app: &AppHandle, title: &str, body: &str) {
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        log::warn!("notification failed: {e}");
    }
}

/// Load (or reuse) the transcription engine for the selected model.
pub fn ensure_engine(state: &AppState) -> Result<Arc<Mutex<Transcriber>>> {
    let s = state.settings();
    let id = s
        .model_id
        .clone()
        .or_else(|| catalog::MODELS.iter().find(|m| models::is_downloaded(m)).map(|m| m.id.to_string()))
        .ok_or_else(|| anyhow!("Download a speech model first (Settings → Models)."))?;
    if let Some(e) = state.engine.lock().as_ref() {
        if e.lock().model_id == id {
            e.lock().set_language(Some(s.language.clone()));
            return Ok(e.clone());
        }
    }
    let spec = catalog::find(&id).ok_or_else(|| anyhow!("unknown model {id}"))?;
    state.engine_loading.store(true, Ordering::Relaxed);
    let res = Transcriber::load(spec);
    state.engine_loading.store(false, Ordering::Relaxed);
    let mut t = res?;
    t.set_language(Some(s.language));
    let arc = Arc::new(Mutex::new(t));
    *state.engine.lock() = Some(arc.clone());
    Ok(arc)
}

/// Speaker model, loaded lazily; downloaded in the background the first time.
pub fn ensure_embedder(app: &AppHandle) -> Option<Arc<Mutex<SpeakerEmbedder>>> {
    let state = app.state::<AppState>();
    if !state.settings().diarize {
        return None;
    }
    if let Some(e) = state.embedder.lock().as_ref() {
        return Some(e.clone());
    }
    if !models::speaker_model_present() {
        let app = app.clone();
        std::thread::spawn(move || {
            let cancel = AtomicBool::new(false);
            let r = models::ensure_speaker_model(&cancel, |d, t| {
                let _ = app.emit("download-progress", json!({"id": "speakers", "done": d, "total": t}));
            });
            let _ = app.emit("download-progress", json!({"id": "speakers", "finished": true, "error": r.as_ref().err().map(|e| e.to_string())}));
        });
        return None;
    }
    let path = margin_core::paths::models_dir().join(margin_core::diarize::EMBED_MODEL_FILE);
    match SpeakerEmbedder::load(&path) {
        Ok(e) => {
            let arc = Arc::new(Mutex::new(e));
            *state.embedder.lock() = Some(arc.clone());
            Some(arc)
        }
        Err(e) => {
            log::warn!("speaker model failed to load: {e}");
            None
        }
    }
}

struct Sink {
    app: AppHandle,
    meeting_id: String,
    last_speech: Arc<Mutex<Instant>>,
}

impl SessionSink for Sink {
    fn segment(&self, mut seg: Segment) {
        *self.last_speech.lock() = Instant::now();
        let state = self.app.state::<AppState>();
        match state.store.add_segment(&self.meeting_id, &seg) {
            Ok(id) => seg.id = id,
            Err(e) => log::error!("saving segment: {e}"),
        }
        let _ = self.app.emit("segment", json!({"meeting_id": self.meeting_id, "segment": seg}));
    }
    fn levels(&self, mic: f32, system: f32) {
        let _ = self.app.emit("levels", json!({"mic": mic, "system": system}));
    }
    fn warning(&self, msg: &str) {
        log::warn!("{msg}");
        let _ = self.app.emit("warning", msg);
    }
}

pub fn new_meeting(state: &AppState, title: &str, event: Option<&CalEvent>) -> Result<Meeting> {
    let s = state.settings();
    let m = Meeting {
        id: uuid::Uuid::new_v4().to_string(),
        title: event.map(|e| e.title.clone()).unwrap_or_else(|| title.to_string()),
        created_at: Utc::now(),
        event_id: event.map(|e| e.id.clone()),
        attendees: event.map(|e| e.attendees.clone()).unwrap_or_default(),
        template: s.default_template.clone(),
        status: "draft".into(),
        meeting_url: event.and_then(|e| e.meeting_url.clone()),
        ..Default::default()
    };
    state.store.create_meeting(&m)?;
    Ok(m)
}

pub fn emit_rec_state(app: &AppHandle) {
    let state = app.state::<AppState>();
    let rec = state.recording.lock();
    let v = match rec.as_ref() {
        Some(r) => json!({
            "recording": true,
            "meeting_id": r.meeting_id,
            "paused": r.session.is_paused(),
            "elapsed_ms": r.offset_ms + r.session.elapsed_ms(),
            "system_audio": r.session.system_active,
            "event_end": r.event_end,
            "auto": state.auto_recording.lock().as_deref() == Some(r.meeting_id.as_str()),
        }),
        None => json!({"recording": false}),
    };
    drop(rec);
    let _ = app.emit("rec-state", v);
    crate::tray::refresh(app);
}

pub fn start(app: &AppHandle, meeting_id: &str, event_end: Option<DateTime<Utc>>) -> Result<()> {
    let state = app.state::<AppState>();
    if let Some(r) = state.recording.lock().as_ref() {
        if r.meeting_id == meeting_id {
            return Ok(());
        }
    }
    // Starting a new meeting while another records (back-to-back): finish the previous one
    // properly, including its notes.
    stop(app, true)?;
    let meeting = state.store.get_meeting(meeting_id)?.ok_or_else(|| anyhow!("meeting not found"))?;
    let engine = ensure_engine(&state)?;
    let embedder = ensure_embedder(app);
    let s = state.settings();
    // Voice prints stay in memory only (never written to disk — biometric data).
    let clusterer: Option<SpeakerClusterer> = state.voiceprints.lock().get(meeting_id).cloned();
    // Without this meeting's voice prints (e.g. after a restart) new speakers must not reuse
    // existing labels like s1, or they'd inherit someone else's name.
    let speaker_base = if clusterer.is_some() {
        0
    } else {
        state.store.segments(meeting_id)?.iter().filter_map(|s| s.speaker.strip_prefix('s')?.parse::<usize>().ok()).max().unwrap_or(0)
    };
    let offset_ms = state.store.segments(meeting_id)?.last().map(|s| s.end_ms + 1000).unwrap_or(0).max(meeting.duration_ms);
    let last_speech = Arc::new(Mutex::new(Instant::now()));
    let sink = Arc::new(Sink { app: app.clone(), meeting_id: meeting_id.to_string(), last_speech: last_speech.clone() });
    let cfg = SessionConfig {
        record_to: (s.keep_audio && margin_core::audio_rec::valid_id(meeting_id))
            .then(|| margin_core::paths::audio_dir().join(meeting_id).join(offset_ms.to_string())),
        capture_system: s.capture_system,
        mic_device: s.mic_device.clone(),
        offset_ms,
        clusterer,
        speaker_base,
        ..Default::default()
    };
    // MARGIN_TEST_AUDIO=<dir>: play <dir>/mic.wav + system.wav in real time instead of the
    // live devices (QA of the scheduler/recorder path without OS audio permissions).
    let session = match std::env::var("MARGIN_TEST_AUDIO") {
        Ok(dir) => {
            let (rx, handle) = test_audio_source(dir.into())?;
            Session::from_frames_with(cfg, rx, vec![handle], engine, embedder, sink)?
        }
        Err(_) => Session::start(cfg, engine, embedder, sink)?,
    };
    state.store.mark_started(meeting_id)?;
    if let Some(e) = state.engine.lock().as_ref() {
        let _ = state.store.set_field(meeting_id, "stt_model", &e.lock().model_id);
    }
    *state.recording.lock() = Some(ActiveRecording {
        meeting_id: meeting_id.to_string(),
        session,
        event_end,
        last_speech,
        offset_ms,
    });
    *state.prompt.lock() = None;
    let _ = app.emit("prompt", serde_json::Value::Null);
    emit_rec_state(app);
    Ok(())
}

/// Stop the active recording (if any). Runs the AI pass in the background when enabled.
pub fn stop(app: &AppHandle, run_ai: bool) -> Result<()> {
    let state = app.state::<AppState>();
    let Some(rec) = state.recording.lock().take() else { return Ok(()) };
    *state.auto_recording.lock() = None;
    emit_rec_state(app);
    let _ = app.emit("rec-state", json!({"recording": false, "finishing": rec.meeting_id}));
    let app2 = app.clone();
    std::thread::spawn(move || {
        let state = app2.state::<AppState>();
        let id = rec.meeting_id.clone();
        let offset = rec.offset_ms;
        let result = rec.session.stop();
        let _ = state.store.mark_stopped(&id, offset + result.duration_ms);
        state.voiceprints.lock().insert(id.clone(), result.clusterer);
        let _ = app2.emit("meeting-updated", json!({"id": id}));
        emit_rec_state(&app2);
        crate::export::schedule(&app2, &id);
        // No AI provider chosen yet: never pick one implicitly. The note asks the user instead.
        if run_ai && state.settings().auto_enhance && state.settings().provider.is_some() {
            if let Err(e) = after_meeting(&app2, &id) {
                let _ = app2.emit("warning", format!("AI notes: {e}"));
            }
        }
    });
    Ok(())
}

/// Stop and wait (bounded) so quitting, restarting or deleting never loses the end of a meeting.
pub fn stop_and_wait(app: &AppHandle, timeout: std::time::Duration) {
    let state = app.state::<AppState>();
    let Some(rec) = state.recording.lock().take() else { return };
    let (tx, rx) = std::sync::mpsc::channel();
    let app2 = app.clone();
    std::thread::spawn(move || {
        let state = app2.state::<AppState>();
        let result = rec.session.stop();
        let _ = state.store.mark_stopped(&rec.meeting_id, rec.offset_ms + result.duration_ms);
        state.voiceprints.lock().insert(rec.meeting_id.clone(), result.clusterer);
        let _ = tx.send(());
    });
    if rx.recv_timeout(timeout).is_err() {
        log::warn!("recording did not finish within {timeout:?}; the startup sweep will close it");
    }
}

/// The AI the user chose to write notes. Never falls back to whatever CLI happens to be signed
/// in: nothing leaves the Mac until the user has picked a provider.
pub fn provider(state: &AppState) -> Result<(llm::Provider, String)> {
    let s = state.settings();
    s.provider
        .map(|p| (p, s.provider_model))
        .ok_or_else(|| anyhow!("Choose who writes your notes first (Settings → AI notes)."))
}

/// Auto-record started a meeting: say so loudly (notification + in-app banner).
pub fn auto_started(app: &AppHandle, ev: &CalEvent) {
    let state = app.state::<AppState>();
    let id = state.recording.lock().as_ref().map(|r| r.meeting_id.clone());
    *state.auto_recording.lock() = id.clone();
    let title = if ev.title.trim().is_empty() { "Untitled meeting" } else { ev.title.trim() };
    notify(app, &format!("Recording automatically: {title}"), "Auto-record is on. Stop or turn it off from the menu bar or the Margin window.");
    let _ = app.emit("auto-recording", json!({"meeting_id": id, "title": title}));
    emit_rec_state(app);
}

/// Pin the selftests to a signed-in provider (the app itself never picks one implicitly).
fn selftest_provider(s: &mut crate::state::Settings) {
    if s.provider.is_none() {
        s.provider = llm::detect().into_iter().find(|p| p.available && p.signed_in).map(|p| p.id);
    }
}

fn set_busy(app: &AppHandle, id: &str, what: Option<&str>) {
    let state = app.state::<AppState>();
    match what {
        Some(w) => state.busy.lock().insert(id.to_string(), w.to_string()),
        None => state.busy.lock().remove(id),
    };
    let _ = app.emit("busy", json!({"id": id, "what": what}));
}

/// Name speakers, title the meeting and write enhanced notes.
pub fn after_meeting(app: &AppHandle, id: &str) -> Result<()> {
    let state = app.state::<AppState>();
    let segments = state.store.segments(id)?;
    if segments.iter().all(|s| s.text.trim().is_empty()) {
        return Ok(());
    }
    let (p, model) = provider(&state)?;
    let mut m = with_user_name(&state, state.store.get_meeting(id)?.ok_or_else(|| anyhow!("meeting not found"))?);

    // 1. Who's who.
    let unnamed = segments.iter().any(|s| s.speaker.starts_with('s') && !m.speakers.contains_key(&s.speaker));
    if unnamed {
        set_busy(app, id, Some("Recognising speakers"));
        if let Some((sys, prompt)) = llm::speaker_names_prompt(&m, &segments) {
            if let Ok(out) = llm::complete(p, &model, &sys, &prompt) {
                let cur = state.store.get_meeting(id)?.unwrap_or_default();
                let (mut saved, mut guessed) = (cur.speakers, cur.speakers_guessed);
                for (k, v) in llm::parse_speaker_map(&out) {
                    if !saved.contains_key(&k) {
                        saved.insert(k.clone(), v.clone());
                        if !guessed.contains(&k) {
                            guessed.push(k.clone());
                        }
                    }
                    m.speakers.entry(k).or_insert(v);
                }
                state.store.set_field(id, "speakers", &serde_json::to_string(&saved)?)?;
                state.store.set_field(id, "speakers_guessed", &serde_json::to_string(&guessed)?)?;
                let _ = app.emit("meeting-updated", json!({"id": id}));
            }
        }
    }

    // 2. Title.
    if m.title.trim().is_empty() || m.title == "New note" {
        set_busy(app, id, Some("Naming"));
        let (sys, prompt) = llm::title_prompt(&m, &segments);
        if let Ok(t) = llm::complete(p, &model, &sys, &prompt) {
            let t = t.lines().next().unwrap_or("").trim().trim_matches('"').trim_end_matches('.').to_string();
            if !t.is_empty() && t.len() < 90 {
                m.title = t;
                state.store.set_field(id, "title", &m.title)?;
                let _ = app.emit("meeting-updated", json!({"id": id}));
            }
        }
    }

    // 3. Notes.
    set_busy(app, id, Some("Writing notes"));
    let res = enhance(app, &m, &segments, p, &model);
    set_busy(app, id, None);
    res?;
    notify(app, "Your notes are ready", if m.title.is_empty() { "Margin" } else { &m.title });
    Ok(())
}

/// Use the user's name for their own lines in AI prompts (not persisted).
pub fn with_user_name(state: &AppState, mut m: Meeting) -> Meeting {
    let name = state.settings().user_name;
    if !name.trim().is_empty() {
        m.speakers.entry("me".into()).or_insert(name);
    }
    m
}

pub fn enhance(app: &AppHandle, m: &Meeting, segments: &[Segment], p: llm::Provider, model: &str) -> Result<String> {
    let state = app.state::<AppState>();
    let m = &with_user_name(&state, m.clone());
    let (sys, prompt) = llm::enhance_prompt(m, segments);
    let out = llm::complete(p, model, &sys, &prompt)?;
    state.store.set_field(&m.id, "enhanced", &out)?;
    let by = match p {
        llm::Provider::Claude => format!("claude:{}", if model.is_empty() { "haiku" } else { model }),
        llm::Provider::Codex => format!("codex:{}", if model.is_empty() { llm::codex_default_model() } else { model.to_string() }),
        llm::Provider::Ollama => format!("ollama:{model}"),
    };
    state.store.set_field(&m.id, "enhanced_by", &by)?;
    state.store.set_field(&m.id, "enhanced_edited", "0")?;
    crate::export::schedule(app, &m.id);
    let _ = app.emit("meeting-updated", json!({"id": m.id}));
    Ok(out)
}

/// `MARGIN_SELFTEST=<dir>`: play <dir>/mic.wav + <dir>/system.wav through the real recorder,
/// store and after-meeting AI pass, print the result, then quit. Used for headless QA.
pub fn selftest(app: AppHandle, dir: std::path::PathBuf) {
    std::thread::spawn(move || {
        let run = || -> Result<()> {
            let state = app.state::<AppState>();
            let mut s = state.settings();
            s.model_id = Some(std::env::var("MARGIN_SELFTEST_MODEL").unwrap_or_else(|_| "parakeet-v2".into()));
            s.user_name = "Alex".into();
            selftest_provider(&mut s);
            state.save_settings(s)?;
            let engine = ensure_engine(&state)?;
            let embedder = ensure_embedder(&app);
            let ev = CalEvent {
                id: "selftest".into(),
                title: String::new(),
                start: Utc::now(),
                end: Utc::now(),
                attendees: vec![
                    margin_core::Attendee { name: "Daniel Brooks".into(), email: None, is_self: false },
                    margin_core::Attendee { name: "Rishi Patel".into(), email: None, is_self: false },
                ],
                location: None,
                meeting_url: None,
                calendar: "Test".into(),
                calendar_color: None,
                all_day: false,
            };
            let m = new_meeting(&state, "", Some(&ev))?;
            state.store.set_field(&m.id, "notes", "beta date\nrelease notes owner?")?;
            let read = |n: &str| -> Result<Vec<f32>> {
                let mut r = margin_core::transcribe_rs::audio::read_wav_samples(&dir.join(n)).map_err(|e| anyhow!("{e}"))?;
                r.shrink_to_fit();
                Ok(r)
            };
            let (mic, sys) = (read("mic.wav")?, read("system.wav")?);
            let (tx, rx) = crossbeam_channel_bounded();
            let last_speech = Arc::new(Mutex::new(Instant::now()));
            let sink = Arc::new(Sink { app: app.clone(), meeting_id: m.id.clone(), last_speech });
            let session = Session::from_frames(SessionConfig::default(), rx, engine, embedder, sink)?;
            state.store.mark_started(&m.id)?;
            for i in (0..mic.len().max(sys.len())).step_by(1600) {
                for (src, b) in [(margin_core::AudioSource::Mic, &mic), (margin_core::AudioSource::System, &sys)] {
                    if i < b.len() {
                        tx.send(margin_core::AudioFrame { source: src, samples: b[i..(i + 1600).min(b.len())].to_vec() })?;
                    }
                }
            }
            drop(tx);
            let res = session.stop();
            state.store.mark_stopped(&m.id, res.duration_ms)?;
            after_meeting(&app, &m.id)?;
            let m = state.store.get_meeting(&m.id)?.ok_or_else(|| anyhow!("gone"))?;
            println!("SELFTEST title: {}", m.title);
            println!("SELFTEST speakers: {:?}", m.speakers);
            for s in state.store.segments(&m.id)? {
                println!("SELFTEST seg [{}] {}: {}", s.start_ms, s.speaker, s.text);
            }
            println!("SELFTEST enhanced:\n{}", m.enhanced);
            println!("SELFTEST list: {}", state.store.list_meetings(Some("release notes"), 5)?.len());
            Ok(())
        };
        match run() {
            Ok(()) => println!("SELFTEST OK"),
            Err(e) => println!("SELFTEST FAILED: {e}"),
        }
        app.exit(0);
    });
}

fn crossbeam_channel_bounded() -> (margin_core::Sender<margin_core::AudioFrame>, margin_core::Receiver<margin_core::AudioFrame>) {
    margin_core::bounded(64)
}

/// Real-time player for MARGIN_TEST_AUDIO; after the files end it sends silence until stopped.
fn test_audio_source(
    dir: std::path::PathBuf,
) -> Result<(margin_core::Receiver<margin_core::AudioFrame>, margin_core::audio::CaptureHandle)> {
    let read = |n: &str| margin_core::transcribe_rs::audio::read_wav_samples(&dir.join(n)).map_err(|e| anyhow!("{e}"));
    let (mic, sys) = (read("mic.wav")?, read("system.wav")?);
    let (tx, rx) = margin_core::bounded(64);
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    std::thread::spawn(move || {
        let t0 = Instant::now();
        let mut i = 0usize;
        while !stop2.load(Ordering::Relaxed) {
            for (src, b) in [(margin_core::AudioSource::Mic, &mic), (margin_core::AudioSource::System, &sys)] {
                let chunk = if i < b.len() { b[i..(i + 1600).min(b.len())].to_vec() } else { vec![0.0; 1600] };
                if tx.send(margin_core::AudioFrame { source: src, samples: chunk }).is_err() {
                    return;
                }
            }
            i += 1600;
            let due = t0 + std::time::Duration::from_millis((i / 16) as u64);
            if let Some(d) = due.checked_duration_since(Instant::now()) {
                std::thread::sleep(d);
            }
        }
    });
    Ok((rx, margin_core::audio::CaptureHandle::new(move || stop.store(true, Ordering::Relaxed))))
}

/// `MARGIN_TEST_EVENT=<seconds>`: inject a calendar meeting that started just now and ends in
/// <seconds>, switch to auto-record, and let the real scheduler start, stop and summarise it.
/// Prints the result and quits. Combine with MARGIN_TEST_AUDIO and MARGIN_QUIET_SECS.
pub fn calendar_selftest(app: AppHandle, secs: i64) {
    let state = app.state::<AppState>();
    let mut s = state.settings();
    s.calendar_mode = "auto".into();
    s.auto_stop = true;
    s.auto_enhance = true;
    s.user_name = "Alex".into();
    if std::env::var("MARGIN_TEST_KEEP_AUDIO").is_ok() {
        s.keep_audio = true;
    }
    if let Ok(dir) = std::env::var("MARGIN_TEST_OBSIDIAN") {
        s.obsidian = Some(margin_core::obsidian::ExportOptions { root: dir, ..Default::default() });
    }
    if s.model_id.is_none() {
        s.model_id = Some("parakeet-v2".into());
    }
    selftest_provider(&mut s);
    let _ = state.save_settings(s);
    let now = Utc::now();
    let ev = CalEvent {
        id: format!("qa-{}", now.timestamp()),
        title: "QA: beta launch sync".into(),
        start: now - chrono::Duration::seconds(5),
        end: now + chrono::Duration::seconds(secs),
        attendees: vec![
            margin_core::Attendee { name: "Daniel Brooks".into(), email: Some("daniel@example.com".into()), is_self: false },
            margin_core::Attendee { name: "Rishi Patel".into(), email: Some("rishi@example.com".into()), is_self: false },
            margin_core::Attendee { name: "Alex".into(), email: None, is_self: true },
        ],
        location: None,
        meeting_url: Some("https://meet.google.com/qa-test".into()),
        calendar: "QA".into(),
        calendar_color: None,
        all_day: false,
    };
    *state.events.lock() = vec![ev.clone()];
    std::thread::spawn(move || {
        let t0 = Instant::now();
        let state = app.state::<AppState>();
        let mut started_at = None;
        let mut stopped_at = None;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let rec = state.recording.lock().as_ref().map(|r| r.meeting_id.clone());
            if rec.is_some() && started_at.is_none() {
                started_at = Some(t0.elapsed().as_secs_f32());
                println!("CALTEST auto-started recording after {:.1}s", started_at.unwrap_or(0.0));
            }
            if rec.is_none() && started_at.is_some() && stopped_at.is_none() {
                stopped_at = Some(t0.elapsed().as_secs_f32());
                println!("CALTEST auto-stopped after {:.1}s (event ended at {}s)", stopped_at.unwrap_or(0.0), secs + 5);
            }
            if let Ok(Some(id)) = state.store.find_by_event(&ev.id) {
                if let Ok(Some(m)) = state.store.get_meeting(&id) {
                    if stopped_at.is_some() && !m.enhanced.is_empty() && state.busy.lock().is_empty() {
                        println!("CALTEST meeting: {} | status {} | {} ms | speakers {:?}", m.title, m.status, m.duration_ms, m.speakers);
                        for seg in state.store.segments(&id).unwrap_or_default() {
                            println!("CALTEST seg [{}] {}: {}", seg.start_ms, seg.speaker, seg.text);
                        }
                        println!("CALTEST notes:\n{}", m.enhanced);
                        for (off, p) in margin_core::audio_rec::recordings(&id) {
                            let len = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                            println!("CALTEST audio: offset {off} ms -> {} ({} KB)", p.display(), len / 1024);
                        }
                        // Give the debounced Obsidian export a moment, then report it.
                        std::thread::sleep(std::time::Duration::from_secs(3));
                        if let Ok(Some(m2)) = state.store.get_meeting(&id) {
                            if !m2.export_path.is_empty() {
                                println!("CALTEST obsidian: {}", m2.export_path);
                            }
                        }
                        println!("CALTEST OK in {:.0}s", t0.elapsed().as_secs_f32());
                        app.exit(0);
                        return;
                    }
                }
            }
            if t0.elapsed().as_secs() > 600 {
                println!("CALTEST FAILED: timed out (started={started_at:?} stopped={stopped_at:?})");
                app.exit(1);
                return;
            }
        }
    });
}

/// Switch to the newly selected microphone mid-recording without losing anything:
/// stop the current session (keeping its transcript and speakers) and resume the same note.
pub fn swap_input(app: &AppHandle) -> Result<()> {
    let state = app.state::<AppState>();
    let Some(rec) = state.recording.lock().take() else { return Ok(()) };
    let id = rec.meeting_id.clone();
    let res = rec.session.stop();
    state.store.mark_stopped(&id, rec.offset_ms + res.duration_ms)?;
    state.voiceprints.lock().insert(id.clone(), res.clusterer);
    start(app, &id, rec.event_end)
}
