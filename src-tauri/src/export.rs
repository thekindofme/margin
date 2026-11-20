//! Keeps the Obsidian/folder mirror up to date. Calls are debounced per meeting so a burst of
//! edits produces one write.
use crate::state::AppState;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

static PENDING: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);
const DEBOUNCE: Duration = Duration::from_millis(1500);

/// Schedule an export of one meeting (no-op when export is off or it's still recording).
pub fn schedule(app: &AppHandle, id: &str) {
    if app.state::<AppState>().settings().obsidian.is_none() {
        return;
    }
    let first = {
        let mut p = PENDING.lock();
        let map = p.get_or_insert_with(HashMap::new);
        let first = !map.contains_key(id);
        map.insert(id.to_string(), Instant::now());
        first
    };
    if !first {
        return; // the waiting thread will pick up the latest state
    }
    let app = app.clone();
    let id = id.to_string();
    std::thread::spawn(move || loop {
        std::thread::sleep(DEBOUNCE);
        let ready = {
            let mut p = PENDING.lock();
            let map = p.get_or_insert_with(HashMap::new);
            match map.get(&id) {
                Some(t) if t.elapsed() >= DEBOUNCE => {
                    map.remove(&id);
                    true
                }
                Some(_) => false,
                None => return,
            }
        };
        if ready {
            if let Err(e) = export_now(&app, &id) {
                log::warn!("export to Obsidian failed: {e}");
                let _ = app.emit("warning", format!("Couldn't update your Obsidian note: {e}"));
            }
            return;
        }
    });
}

pub fn export_now(app: &AppHandle, id: &str) -> anyhow::Result<Option<std::path::PathBuf>> {
    let state = app.state::<AppState>();
    let Some(opts) = state.settings().obsidian else { return Ok(None) };
    let Some(m) = state.store.get_meeting(id)? else { return Ok(None) };
    if m.status == "recording" {
        return Ok(None);
    }
    let segs = state.store.segments(id)?;
    if segs.is_empty() && m.notes.trim().is_empty() && m.enhanced.trim().is_empty() {
        return Ok(None); // nothing worth a note yet
    }
    let prev = (!m.export_path.is_empty()).then(|| std::path::PathBuf::from(&m.export_path));
    let path = milkrice_core::obsidian::export(&m, &segs, &opts, prev.as_deref())?;
    let p = path.to_string_lossy().to_string();
    if p != m.export_path {
        state.store.set_field(id, "export_path", &p)?;
    }
    Ok(Some(path))
}
