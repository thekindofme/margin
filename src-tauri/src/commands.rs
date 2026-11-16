//! Commands invoked from the UI. Long work runs on blocking threads so the UI never stalls.
use crate::state::{AppState, Settings};
use crate::{recorder, scheduler};

use margin_core::store::{ChatMessage, Meeting, MeetingSummary};
use margin_core::{audio, catalog, llm, models, CalEvent, Segment};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

type R<T> = Result<T, String>;

fn e<E: std::fmt::Display>(err: E) -> String {
    err.to_string()
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> R<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(e)?.map_err(e)
}

// ------------------------------------------------------------------ settings & status

/// Settings as seen by the UI: secret calendar addresses are reduced to their host.
fn redacted(mut s: Settings) -> Settings {
    for f in s.calendar_feeds.iter_mut() {
        f.url = crate::ics::host(&f.url);
    }
    s
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Settings {
    redacted(state.settings())
}

#[tauri::command]
pub fn save_settings(app: AppHandle, state: State<AppState>, mut settings: Settings) -> R<Settings> {
    let old = state.settings();
    // Feeds (with their secret URLs) are managed only by add/remove_calendar_feed.
    settings.calendar_feeds = old.calendar_feeds.clone();
    state.save_settings(settings.clone()).map_err(e)?;
    if old.model_id != settings.model_id || old.language != settings.language {
        // Reload lazily on next recording; warm it up now in the background.
        *state.engine.lock() = None;
        let app2 = app.clone();
        std::thread::spawn(move || {
            let st = app2.state::<AppState>();
            let r = recorder::ensure_engine(&st);
            let _ = app2.emit("engine", json!({"ready": r.is_ok(), "error": r.err().map(|x| x.to_string())}));
        });
    }
    Ok(redacted(settings))
}

/// Status of every permission Margin uses, for the guided permissions panel.
#[tauri::command]
pub fn permissions(app: AppHandle) -> Value {
    use margin_core::permissions::{status, Permission};
    use tauri_plugin_notification::{NotificationExt, PermissionState};
    let notif = match app.notification().permission_state() {
        Ok(PermissionState::Granted) => "granted",
        Ok(PermissionState::Denied) => "denied",
        _ => "not_determined",
    };
    json!({
        "microphone": status(Permission::Microphone),
        "system_audio": status(Permission::SystemAudio),
        "calendar": status(Permission::Calendar),
        "notifications": notif,
    })
}

fn bundle_path() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().find(|p| p.extension().map(|e| e == "app").unwrap_or(false)).map(|p| p.to_path_buf())
}

/// Ask for a permission the easiest way macOS allows: the native prompt if it can still be
/// shown, otherwise open the exact System Settings pane and reveal Margin.app in Finder so it
/// can be dragged into the list.
#[tauri::command]
pub async fn grant_permission(app: AppHandle, kind: String) -> R<Value> {
    use margin_core::permissions::{open_settings, request, status, PermStatus, Permission};
    if kind == "notifications" {
        use tauri_plugin_notification::NotificationExt;
        let st = app.notification().request_permission().map_err(e)?;
        return Ok(json!({"status": format!("{st:?}").to_lowercase(), "opened_settings": false}));
    }
    let p = match kind.as_str() {
        "microphone" => Permission::Microphone,
        "system_audio" => Permission::SystemAudio,
        "calendar" => Permission::Calendar,
        _ => return Err("unknown permission".into()),
    };
    let before = status(p);
    let after = if before == PermStatus::NotDetermined {
        blocking(move || Ok(request(p))).await?
    } else {
        before
    };
    let mut opened = false;
    if after != PermStatus::Granted {
        // The prompt can't be shown again (or never shows for screen capture): go to Settings.
        let _ = open_settings(p);
        // Only the Screen & System Audio list accepts "+"/drag-in; the others list an app
        // automatically once it has asked.
        if p == Permission::SystemAudio {
            if let Some(b) = bundle_path() {
                let _ = std::process::Command::new("/usr/bin/open").arg("-R").arg(b).spawn();
            }
        }
        opened = true;
    }
    if p == Permission::Calendar && after == PermStatus::Granted {
        let a = app.clone();
        std::thread::spawn(move || scheduler::refresh_events(&a));
    }
    Ok(json!({"status": after, "opened_settings": opened}))
}

#[tauri::command]
pub fn open_permission_settings(kind: String) -> R<()> {
    use margin_core::permissions::{open_settings, Permission};
    let p = match kind.as_str() {
        "microphone" => Permission::Microphone,
        "system_audio" => Permission::SystemAudio,
        _ => Permission::Calendar,
    };
    open_settings(p).map_err(e)
}

/// Screen & System Audio Recording only takes effect after a relaunch.
#[tauri::command]
pub fn restart_app(app: AppHandle) {
    recorder::stop_and_wait(&app, std::time::Duration::from_secs(20));
    app.restart();
}

#[tauri::command]
pub fn input_devices() -> Vec<String> {
    audio::list_input_devices()
}

/// Inputs, the system defaults, and what's selected — for the recorder's device chip.
#[tauri::command]
pub async fn audio_devices(app: AppHandle) -> R<Value> {
    let selected = app.state::<AppState>().settings().mic_device;
    let capture_system = app.state::<AppState>().settings().capture_system;
    blocking(move || {
        let (default_input, default_output) = audio::default_devices();
        let inputs = audio::list_input_devices();
        let active = selected.clone().filter(|s| inputs.contains(s)).or(default_input.clone());
        Ok(json!({
            "inputs": inputs, "default_input": default_input, "default_output": default_output,
            "selected": selected, "active": active, "capture_system": capture_system,
        }))
    })
    .await
}

/// Listen briefly on every mic and report what each hears (finds the one that hears you).
#[tauri::command]
pub async fn probe_mics(app: AppHandle) -> R<Value> {
    // The picker's preview stream would compete for the device; pause it.
    if let Some(h) = app.state::<AppState>().mic_test.lock().take() {
        h.stop();
    }
    let probes = blocking(|| Ok(audio::probe_inputs(1500))).await?;
    Ok(json!(probes))
}

/// Choose a microphone (None = follow the system default). Hot-swaps if recording.
#[tauri::command]
pub async fn set_mic(app: AppHandle, device: Option<String>) -> R<()> {
    let state = app.state::<AppState>();
    let mut s = state.settings();
    s.mic_device = device.filter(|d| !d.is_empty());
    state.save_settings(s).map_err(e)?;
    let recording = state.recording.lock().is_some();
    if recording {
        let a = app.clone();
        blocking(move || recorder::swap_input(&a)).await?;
    }
    Ok(())
}

/// Live input level for the picker, emitted as "mic-test" events (~12/s) until stopped.
#[tauri::command]
pub fn mic_test(app: AppHandle, device: Option<String>, on: bool) -> R<()> {
    let state = app.state::<AppState>();
    if let Some(h) = state.mic_test.lock().take() {
        h.stop();
    }
    if !on {
        return Ok(());
    }
    let (tx, rx) = margin_core::bounded::<margin_core::AudioFrame>(32);
    let handle = audio::start_mic(device.filter(|d| !d.is_empty()), tx).map_err(e)?;
    *state.mic_test.lock() = Some(handle);
    std::thread::spawn(move || {
        let mut peak = 0f32;
        let mut last = std::time::Instant::now();
        let mut silent = true;
        while let Ok(f) = rx.recv() {
            for v in &f.samples {
                peak = peak.max(v.abs());
                if *v != 0.0 {
                    silent = false;
                }
            }
            if last.elapsed() >= std::time::Duration::from_millis(80) {
                let _ = app.emit("mic-test", json!({"level": peak, "digital_silence": silent}));
                peak = 0.0;
                last = std::time::Instant::now();
            }
        }
    });
    Ok(())
}

// ------------------------------------------------------------------ models

#[tauri::command]
pub fn list_models(state: State<AppState>) -> Value {
    let s = state.settings();
    let downloading: Vec<String> = state.downloads.lock().keys().cloned().collect();
    json!({
        "models": models::statuses(),
        "active": s.model_id,
        "downloading": downloading,
        "speaker_model": models::speaker_model_present(),
    })
}

#[tauri::command]
pub fn download_model(app: AppHandle, state: State<AppState>, id: String) -> R<()> {
    let spec = catalog::find(&id).ok_or("unknown model")?;
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut d = state.downloads.lock();
        if d.contains_key(&id) {
            return Ok(());
        }
        d.insert(id.clone(), cancel.clone());
    }
    std::thread::spawn(move || {
        let app2 = app.clone();
        let id2 = id.clone();
        let res = models::download(spec, &cancel, move |done, total| {
            let _ = app2.emit("download-progress", json!({"id": id2, "done": done, "total": total}));
        });
        let state = app.state::<AppState>();
        state.downloads.lock().remove(&id);
        let err = res.as_ref().err().map(|x| x.to_string());
        if res.is_ok() {
            // First model becomes the active one.
            let mut s = state.settings();
            if s.model_id.is_none() {
                s.model_id = Some(id.clone());
                let _ = state.save_settings(s);
            }
            // Speaker model piggybacks on the first download.
            if !models::speaker_model_present() {
                let _ = recorder::ensure_embedder(&app);
            }
        }
        let _ = app.emit("download-progress", json!({"id": id, "finished": true, "error": err}));
    });
    Ok(())
}

#[tauri::command]
pub fn cancel_download(state: State<AppState>, id: String) {
    if let Some(c) = state.downloads.lock().get(&id) {
        c.store(true, Ordering::Relaxed);
    }
}

#[tauri::command]
pub fn delete_model(state: State<AppState>, id: String) -> R<()> {
    let spec = catalog::find(&id).ok_or("unknown model")?;
    let mut s = state.settings();
    if s.model_id.as_deref() == Some(id.as_str()) {
        if state.recording.lock().is_some() {
            return Err("Stop recording before deleting the active model.".into());
        }
        *state.engine.lock() = None;
        s.model_id = None;
        state.save_settings(s).map_err(e)?;
    }
    models::delete(spec).map_err(e)
}

// ------------------------------------------------------------------ AI providers

#[tauri::command]
pub async fn providers(app: AppHandle, refresh: bool) -> R<Value> {
    let state = app.state::<AppState>();
    let cached = state.providers.lock().clone();
    let list = match (cached, refresh) {
        (Some(l), false) => l,
        _ => {
            let l = blocking(|| Ok(llm::detect())).await?;
            *state.providers.lock() = Some(l.clone());
            l
        }
    };
    let templates: Vec<Value> =
        llm::TEMPLATES.iter().map(|(id, name, desc)| json!({"id": id, "name": name, "desc": desc})).collect();
    Ok(json!({"providers": list, "templates": templates}))
}

// ------------------------------------------------------------------ calendar

#[tauri::command]
pub fn upcoming_events(state: State<AppState>) -> Vec<Value> {
    let now = chrono::Utc::now();
    state
        .events
        .lock()
        .iter()
        .filter(|ev| ev.end > now - chrono::Duration::hours(2) && !ev.all_day)
        .take(40)
        .map(|ev| {
            json!({
                "event": ev,
                "real": scheduler::is_real_meeting(ev),
                "meeting_id": state.store.find_by_event(&ev.id).ok().flatten(),
            })
        })
        .collect()
}

/// Subscribe to a read-only iCal feed (e.g. Google Calendar's secret address). Validated first.
#[tauri::command]
pub async fn add_calendar_feed(app: AppHandle, name: String, url: String, owner_email: Option<String>) -> R<Value> {
    let feed = crate::ics::Feed {
        id: uuid::Uuid::new_v4().to_string(),
        name: if name.trim().is_empty() { "Google Calendar".into() } else { name.trim().to_string() },
        url: url.trim().to_string(),
        owner_email: owner_email.map(|e| e.trim().to_string()).filter(|e| e.contains('@')),
    };
    let f2 = feed.clone();
    let upcoming = blocking(move || crate::ics::test_feed(&f2)).await?;
    let state = app.state::<AppState>();
    let mut s = state.settings();
    s.calendar_feeds.retain(|f| f.url != feed.url);
    s.calendar_feeds.push(feed.clone());
    state.save_settings(s).map_err(e)?;
    log::info!("calendar feed added: '{}' ({}), {upcoming} events in the next 30 days", feed.name, crate::ics::host(&feed.url));
    let a = app.clone();
    std::thread::spawn(move || scheduler::refresh_events(&a));
    Ok(json!({"id": feed.id, "upcoming": upcoming}))
}

#[tauri::command]
pub fn remove_calendar_feed(app: AppHandle, state: State<AppState>, id: String) -> R<()> {
    let mut s = state.settings();
    s.calendar_feeds.retain(|f| f.id != id);
    state.save_settings(s).map_err(e)?;
    crate::ics::forget(&id);
    let a = app.clone();
    std::thread::spawn(move || scheduler::refresh_events(&a));
    Ok(())
}

/// Calendar sources for Settings: the macOS calendars (via permission) and subscribed feeds
/// (URLs never sent to the UI in full).
#[tauri::command]
pub fn calendar_sources(state: State<AppState>) -> Value {
    let feeds: Vec<Value> = state
        .settings()
        .calendar_feeds
        .iter()
        .map(|f| json!({"id": f.id, "name": f.name, "host": crate::ics::host(&f.url), "owner_email": f.owner_email}))
        .collect();
    let mut cals: Vec<String> = state.events.lock().iter().map(|e| e.calendar.clone()).collect();
    cals.sort();
    cals.dedup();
    json!({"feeds": feeds, "calendars_with_events": cals, "event_count": state.events.lock().len()})
}

#[tauri::command]
pub fn current_prompt(state: State<AppState>) -> Value {
    serde_json::to_value(state.prompt.lock().clone()).unwrap_or(Value::Null)
}

#[tauri::command]
pub fn dismiss_prompt(app: AppHandle, state: State<AppState>) {
    *state.prompt.lock() = None;
    let _ = app.emit("prompt", Value::Null);
    crate::tray::refresh(&app);
}

/// Start (or open) notes for a calendar event.
#[tauri::command]
pub async fn start_event(app: AppHandle, event: CalEvent, record: bool) -> R<String> {
    blocking(move || {
        let state = app.state::<AppState>();
        if record {
            scheduler::start_for_event(&app, &event)?;
            Ok(state.store.find_by_event(&event.id)?.unwrap_or_default())
        } else {
            Ok(match state.store.find_by_event(&event.id)? {
                Some(id) => id,
                None => recorder::new_meeting(&state, &event.title, Some(&event))?.id,
            })
        }
    })
    .await
}

// ------------------------------------------------------------------ meetings

#[tauri::command]
pub fn list_meetings(state: State<AppState>, query: Option<String>) -> R<Vec<MeetingSummary>> {
    state.store.list_meetings(query.as_deref(), 500).map_err(e)
}

#[derive(Serialize)]
pub struct MeetingDetail {
    meeting: Meeting,
    segments: Vec<Segment>,
    chats: Vec<ChatMessage>,
    busy: Option<String>,
}

#[tauri::command]
pub fn get_meeting(state: State<AppState>, id: String) -> R<MeetingDetail> {
    let meeting = state.store.get_meeting(&id).map_err(e)?.ok_or("not found")?;
    Ok(MeetingDetail {
        segments: state.store.segments(&id).map_err(e)?,
        chats: state.store.chats(&id).map_err(e)?,
        busy: state.busy.lock().get(&id).cloned(),
        meeting,
    })
}

#[tauri::command]
pub fn create_meeting(state: State<AppState>, title: Option<String>) -> R<Meeting> {
    recorder::new_meeting(&state, title.as_deref().unwrap_or(""), None).map_err(e)
}

#[tauri::command]
pub fn update_meeting(app: AppHandle, state: State<AppState>, id: String, field: String, value: String) -> R<()> {
    state.store.set_field(&id, &field, &value).map_err(e)?;
    crate::export::schedule(&app, &id);
    Ok(())
}

#[tauri::command]
pub fn update_segment(state: State<AppState>, id: i64, text: String) -> R<()> {
    state.store.update_segment_text(id, &text).map_err(e)
}

#[tauri::command]
pub fn delete_meeting(app: AppHandle, state: State<AppState>, id: String) -> R<()> {
    if state.recording.lock().as_ref().map(|r| r.meeting_id == id).unwrap_or(false) {
        recorder::stop_and_wait(&app, std::time::Duration::from_secs(20));
    }
    let _ = margin_core::audio_rec::delete_recordings(&id);
    state.store.delete_meeting(&id).map_err(e)
}

/// Kept recordings for a meeting (opt-in setting), each with the transcript offset it starts at.
#[tauri::command]
pub fn meeting_audio(id: String) -> Value {
    json!(margin_core::audio_rec::recordings(&id)
        .into_iter()
        .map(|(offset_ms, p)| json!({"offset_ms": offset_ms, "path": p.to_string_lossy()}))
        .collect::<Vec<_>>())
}

#[tauri::command]
pub fn delete_meeting_audio(id: String) -> R<()> {
    margin_core::audio_rec::delete_recordings(&id).map_err(e)
}

#[tauri::command]
pub fn audio_storage() -> Value {
    json!({"bytes": margin_core::audio_rec::usage_bytes()})
}

/// Rename a speaker; naming two speakers the same merges them.
#[tauri::command]
pub fn rename_speaker(app: AppHandle, state: State<AppState>, id: String, key: String, name: String) -> R<()> {
    crate::export::schedule(&app, &id);
    let mut m = state.store.get_meeting(&id).map_err(e)?.ok_or("not found")?;
    let name = name.trim().to_string();
    if let Some((other, _)) = m.speakers.iter().find(|(k, v)| **k != key && v.eq_ignore_ascii_case(&name) && !name.is_empty()) {
        let other = other.clone();
        state.store.reassign_speaker(&id, &key, &other).map_err(e)?;
        m.speakers.remove(&key);
    } else if name.is_empty() {
        m.speakers.remove(&key);
    } else {
        m.speakers.insert(key, name);
    }
    state.store.set_field(&id, "speakers", &serde_json::to_string(&m.speakers).map_err(e)?).map_err(e)
}

// ------------------------------------------------------------------ recording

#[tauri::command]
pub async fn start_recording(app: AppHandle, id: String) -> R<()> {
    blocking(move || recorder::start(&app, &id, None)).await
}

#[tauri::command]
pub fn stop_recording(app: AppHandle) -> R<()> {
    recorder::stop(&app, true).map_err(e)
}

#[tauri::command]
pub fn pause_recording(app: AppHandle, state: State<AppState>, paused: bool) {
    if let Some(r) = state.recording.lock().as_ref() {
        if paused {
            r.session.pause()
        } else {
            r.session.resume()
        }
    }
    recorder::emit_rec_state(&app);
}

#[tauri::command]
pub fn rec_state(app: AppHandle) {
    recorder::emit_rec_state(&app);
}

// ------------------------------------------------------------------ AI

#[tauri::command]
pub async fn enhance(app: AppHandle, id: String) -> R<String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let m = state.store.get_meeting(&id)?.ok_or_else(|| anyhow::anyhow!("not found"))?;
        let segs = state.store.segments(&id)?;
        anyhow::ensure!(!segs.is_empty() || !m.notes.trim().is_empty(), "Nothing to work with yet — record or write something first.");
        let (p, model) = recorder::provider(&state)?;
        state.busy.lock().insert(id.clone(), "Writing notes".into());
        let _ = app.emit("busy", json!({"id": id, "what": "Writing notes"}));
        let r = recorder::enhance(&app, &m, &segs, p, &model);
        state.busy.lock().remove(&id);
        let _ = app.emit("busy", json!({"id": id, "what": null}));
        r
    })
    .await
}

#[tauri::command]
pub async fn identify_speakers(app: AppHandle, id: String) -> R<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        let mut m = state.store.get_meeting(&id)?.ok_or_else(|| anyhow::anyhow!("not found"))?;
        let segs = state.store.segments(&id)?;
        let (p, model) = recorder::provider(&state)?;
        let (sys, prompt) = llm::speaker_names_prompt(&m, &segs)
            .ok_or_else(|| anyhow::anyhow!("No other speakers in this meeting."))?;
        let map = llm::parse_speaker_map(&llm::complete(p, &model, &sys, &prompt)?);
        anyhow::ensure!(!map.is_empty(), "Couldn't tell who's who from the conversation — name speakers by clicking them.");
        m.speakers.extend(map);
        state.store.set_field(&id, "speakers", &serde_json::to_string(&m.speakers)?)?;
        let _ = app.emit("meeting-updated", json!({"id": id}));
        crate::export::schedule(&app, &id);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn ask(app: AppHandle, id: String, question: String) -> R<String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let m = recorder::with_user_name(&state, state.store.get_meeting(&id)?.ok_or_else(|| anyhow::anyhow!("not found"))?);
        let segs = state.store.segments(&id)?;
        let chats = state.store.chats(&id)?;
        let mut history = Vec::new();
        let mut q: Option<String> = None;
        for c in chats {
            if c.role == "user" {
                q = Some(c.content);
            } else if let Some(qq) = q.take() {
                history.push((qq, c.content));
            }
        }
        let (p, model) = recorder::provider(&state)?;
        let (sys, prompt) = llm::ask_prompt(&m, &segs, &history, &question);
        state.store.add_chat(&id, "user", &question)?;
        let answer = llm::complete(p, &model, &sys, &prompt)?;
        state.store.add_chat(&id, "assistant", &answer)?;
        Ok(answer)
    })
    .await
}

#[tauri::command]
pub async fn ask_all(app: AppHandle, question: String) -> R<String> {
    blocking(move || {
        let state = app.state::<AppState>();
        // Most relevant first: search hits, then recent meetings.
        let words: Vec<&str> = question.split_whitespace().filter(|w| w.len() > 4).collect();
        let mut ids: Vec<String> = Vec::new();
        for w in &words {
            for m in state.store.list_meetings(Some(w.trim_matches(|c: char| !c.is_alphanumeric())), 6)? {
                if !ids.contains(&m.id) {
                    ids.push(m.id);
                }
            }
        }
        for m in state.store.list_meetings(None, 12)? {
            if !ids.contains(&m.id) {
                ids.push(m.id);
            }
        }
        ids.truncate(12);
        let mut data = Vec::new();
        for id in ids {
            if let Some(m) = state.store.get_meeting(&id)? {
                let segs = state.store.segments(&id)?;
                data.push((m, segs));
            }
        }
        anyhow::ensure!(!data.is_empty(), "No meetings yet.");
        let (p, model) = recorder::provider(&state)?;
        let (sys, prompt) = llm::ask_all_prompt(&data, &question);
        llm::complete(p, &model, &sys, &prompt)
    })
    .await
}

// ------------------------------------------------------------------ agents (CLI, MCP, skill)

fn cli_path() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let p = exe.parent()?.join("margin-cli");
    p.is_file().then_some(p)
}

fn home() -> std::path::PathBuf {
    dirs::home_dir().unwrap_or_default()
}

fn agent_cli_ok(bin: &str, args: &[&str]) -> bool {
    std::process::Command::new(bin)
        .args(args)
        .env("PATH", llm::shell_path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[tauri::command]
pub async fn agents_status() -> R<Value> {
    blocking(|| {
        let link = home().join(".local/bin/margin");
        let cli = cli_path();
        let installed = std::fs::read_link(&link).ok().zip(cli.clone()).map(|(a, b)| a == b).unwrap_or(false);
        let which = |n: &str| which::which_in(n, Some(llm::shell_path()), "/").is_ok();
        Ok(json!({
            "cli_bundled": cli.is_some(),
            "cli_installed": installed,
            "cli_path": link.to_string_lossy(),
            "claude_available": which("claude"),
            "claude_connected": agent_cli_ok("claude", &["mcp", "get", "margin"]) && home().join(".claude/skills/margin/SKILL.md").exists(),
            "codex_available": which("codex"),
            "codex_connected": agent_cli_ok("codex", &["mcp", "get", "margin"]) && home().join(".codex/skills/margin/SKILL.md").exists(),
        }))
    })
    .await
}

/// One-click agent setup. target: "cli" | "claude" | "codex".
#[tauri::command]
pub async fn agent_connect(app: AppHandle, target: String) -> R<String> {
    let skill_src = app
        .path()
        .resource_dir()
        .map_err(e)?
        .join("skill/margin/SKILL.md");
    blocking(move || {
        let cli = cli_path().ok_or_else(|| anyhow::anyhow!("margin-cli isn't bundled in this build"))?;
        let install_skill = |dir: std::path::PathBuf| -> anyhow::Result<()> {
            std::fs::create_dir_all(&dir)?;
            std::fs::copy(&skill_src, dir.join("SKILL.md"))?;
            Ok(())
        };
        let run = |bin: &str, args: Vec<String>| -> anyhow::Result<()> {
            let out = std::process::Command::new(bin).args(&args).env("PATH", llm::shell_path()).output()?;
            anyhow::ensure!(
                out.status.success() || String::from_utf8_lossy(&out.stderr).contains("already exists"),
                "{bin} failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
            Ok(())
        };
        let cli_s = cli.to_string_lossy().to_string();
        match target.as_str() {
            "cli" => {
                let dir = home().join(".local/bin");
                std::fs::create_dir_all(&dir)?;
                let link = dir.join("margin");
                let _ = std::fs::remove_file(&link);
                std::os::unix::fs::symlink(&cli, &link)?;
                Ok(format!("Installed {}", link.display()))
            }
            "claude" => {
                let _ = run("claude", vec!["mcp".into(), "remove".into(), "-s".into(), "user".into(), "margin".into()]);
                run("claude", vec!["mcp".into(), "add".into(), "-s".into(), "user".into(), "margin".into(), "--".into(), cli_s, "mcp".into()])?;
                install_skill(home().join(".claude/skills/margin"))?;
                Ok("Claude Code can now read your meetings (MCP + skill)".into())
            }
            "codex" => {
                let _ = run("codex", vec!["mcp".into(), "remove".into(), "margin".into()]);
                run("codex", vec!["mcp".into(), "add".into(), "margin".into(), "--".into(), cli_s, "mcp".into()])?;
                install_skill(home().join(".codex/skills/margin"))?;
                Ok("Codex can now read your meetings (MCP + skill)".into())
            }
            _ => anyhow::bail!("unknown target"),
        }
    })
    .await
}

// ------------------------------------------------------------------ Obsidian / second brain

#[tauri::command]
pub fn obsidian_vaults() -> Value {
    json!(margin_core::obsidian::detect_vaults())
}

/// Write every past meeting into the chosen vault/folder. Returns how many notes were written.
#[tauri::command]
pub async fn obsidian_export_all(app: AppHandle) -> R<usize> {
    blocking(move || {
        let state = app.state::<AppState>();
        anyhow::ensure!(state.settings().obsidian.is_some(), "Choose an Obsidian vault or folder first.");
        let mut n = 0;
        for m in state.store.list_meetings(None, 100_000)? {
            if crate::export::export_now(&app, &m.id)?.is_some() {
                n += 1;
            }
        }
        log::info!("exported {n} meetings to Obsidian");
        Ok(n)
    })
    .await
}

/// Open a meeting's note in Obsidian (falls back to the default Markdown app).
#[tauri::command]
pub async fn open_in_obsidian(app: AppHandle, id: String) -> R<()> {
    let path = {
        let a = app.clone();
        let id2 = id.clone();
        blocking(move || crate::export::export_now(&a, &id2)).await?
    }
    .ok_or("Turn on Obsidian export in Settings first.")?;
    let url = format!(
        "obsidian://open?path={}",
        path.to_string_lossy().bytes().map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        }).collect::<String>()
    );
    let ok = std::process::Command::new("/usr/bin/open").arg(&url).status().map(|s| s.success()).unwrap_or(false);
    if !ok {
        let _ = std::process::Command::new("/usr/bin/open").arg(&path).status();
    }
    Ok(())
}

// ------------------------------------------------------------------ export

pub fn to_markdown(m: &Meeting, segs: &[Segment]) -> String {
    let mut out = format!("# {}\n\n", if m.title.is_empty() { "Untitled meeting" } else { &m.title });
    out.push_str(&format!("*{}*", m.created_at.with_timezone(&chrono::Local).format("%A %e %B %Y, %H:%M")));
    if !m.attendees.is_empty() {
        out.push_str(&format!(" · {}", m.attendees.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")));
    }
    out.push_str("\n\n");
    if !m.enhanced.trim().is_empty() {
        out.push_str(m.enhanced.trim());
        out.push_str("\n\n");
    }
    if !m.notes.trim().is_empty() {
        out.push_str("## My notes\n\n");
        out.push_str(m.notes.trim());
        out.push_str("\n\n");
    }
    if !segs.is_empty() {
        out.push_str("## Transcript\n\n");
        out.push_str(&llm::transcript_text(segs, &m.speakers, usize::MAX));
        out.push('\n');
    }
    out
}

#[tauri::command]
pub fn export_markdown(state: State<AppState>, id: String, save: bool) -> R<String> {
    let m = state.store.get_meeting(&id).map_err(e)?.ok_or("not found")?;
    let segs = state.store.segments(&id).map_err(e)?;
    let md = to_markdown(&m, &segs);
    if !save {
        return Ok(md);
    }
    let dir = dirs::document_dir().unwrap_or_else(margin_core::paths::data_dir).join("Margin");
    std::fs::create_dir_all(&dir).map_err(e)?;
    let safe: String = m
        .title
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' })
        .collect();
    let name = format!("{} {}.md", m.created_at.with_timezone(&chrono::Local).format("%Y-%m-%d"), safe.trim());
    let path = dir.join(name);
    std::fs::write(&path, md).map_err(e)?;
    Ok(path.to_string_lossy().to_string())
}
