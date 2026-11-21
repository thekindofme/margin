//! iPhone sync inside the app: a background loop that runs a sync pass every few seconds once
//! sync is set up, imports the iPhone's meetings (and mirrors them to Obsidian), and writes the
//! notes an iPhone asks for with the AI chosen here. The format, crypto and merge live in the
//! `milkrice-sync` crate, shared with the `milkrice-sync` command.
use crate::state::AppState;
use chrono::{DateTime, Utc};
use milkrice_sync::desktop::{self, Pairing};
use milkrice_sync::folder::Vault;
use parking_lot::{Condvar, Mutex};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::OnceLock;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const EVERY: Duration = Duration::from_secs(10);

#[derive(Default)]
struct Rt {
    /// The open vault, keyed by vault id (opening reads the key from the keychain).
    vault: Option<(String, Vault)>,
    pairing: Option<Pairing>,
    last_ok: Option<DateTime<Utc>>,
    error: Option<String>,
    /// Meetings whose notes are being written for an iPhone right now.
    writing: HashSet<String>,
    /// Requests that failed this session (no AI chosen…): not retried until relaunch.
    failed: HashSet<String>,
    wake: bool,
}

fn rt() -> &'static (Mutex<Rt>, Condvar) {
    static RT: OnceLock<(Mutex<Rt>, Condvar)> = OnceLock::new();
    RT.get_or_init(Default::default)
}

/// Run a pass now instead of waiting for the next tick.
pub fn wake() {
    let (m, cv) = rt();
    m.lock().wake = true;
    cv.notify_all();
}

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || loop {
        {
            let (m, cv) = rt();
            let mut g = m.lock();
            if !g.wake {
                cv.wait_for(&mut g, EVERY);
            }
            g.wake = false;
        }
        if desktop::config().is_err() {
            continue;
        }
        if let Err(e) = pass(&app) {
            let msg = format!("{e:#}");
            let mut g = rt().0.lock();
            if g.error.as_deref() != Some(&msg) {
                log::warn!("sync: {msg}");
                g.error = Some(msg);
                drop(g);
                let _ = app.emit("sync-status", ());
            }
        }
    });
}

fn with_vault<T>(f: impl FnOnce(&desktop::Config, &Vault) -> anyhow::Result<T>) -> anyhow::Result<T> {
    let cfg = desktop::config()?;
    let cached = rt().0.lock().vault.as_ref().filter(|(id, _)| id == &cfg.vault_id).map(|(_, v)| v.clone());
    let v = match cached {
        Some(v) => v,
        None => {
            let v = desktop::open_vault(&cfg)?;
            rt().0.lock().vault = Some((cfg.vault_id.clone(), v.clone()));
            v
        }
    };
    // The lock isn't held here: a pass can take a while and the UI asks for status meanwhile.
    f(&cfg, &v)
}

fn pass(app: &AppHandle) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    let r = with_vault(|cfg, v| desktop::sync_pass(cfg, v, &state.store))?;
    let first_ok = {
        let mut g = rt().0.lock();
        let changed = g.error.is_some() || g.last_ok.is_none();
        g.error = None;
        g.last_ok = Some(Utc::now());
        changed
    };
    if first_ok || !r.imported.is_empty() {
        let _ = app.emit("sync-status", ());
    }
    if r.exported > 0 || !r.imported.is_empty() {
        log::info!("sync: wrote {} records, updated {} meetings from other devices", r.exported, r.imported.len());
    }
    for p in r.problems.iter().take(3) {
        log::warn!("sync: skipped unreadable file {p}");
    }
    for id in &r.imported {
        crate::export::schedule(app, id);
        let _ = app.emit("meeting-updated", json!({"id": id}));
    }
    for id in r.requests {
        {
            let mut g = rt().0.lock();
            if g.writing.contains(&id) || g.failed.contains(&id) {
                continue;
            }
            g.writing.insert(id.clone());
        }
        let app = app.clone();
        std::thread::spawn(move || {
            log::info!("sync: an iPhone asked for notes on {id}");
            let res = crate::recorder::after_meeting(&app, &id);
            let mut g = rt().0.lock();
            g.writing.remove(&id);
            if let Err(e) = res {
                g.failed.insert(id.clone());
                drop(g);
                let _ = app.emit("warning", format!("Couldn't write notes your iPhone asked for: {e}"));
            } else {
                drop(g);
                crate::export::schedule(&app, &id);
            }
            wake(); // send the notes straight back
        });
    }
    Ok(())
}

pub fn status() -> Value {
    let icloud = desktop::icloud_drive().exists();
    let base = json!({
        "setup": false,
        "icloud_available": icloud,
        "default_folder": desktop::default_folder(),
        "default_has_vault": desktop::folder_has_vault(&desktop::default_folder()),
    });
    let Ok(cfg) = desktop::config() else { return base };
    let (last_ok, error, pairing) = {
        let g = rt().0.lock();
        (g.last_ok, g.error.clone(), g.pairing.is_some())
    };
    let st = with_vault(desktop::status);
    let mut out = json!({
        "setup": true,
        "icloud_available": icloud,
        "folder": cfg.folder,
        "device_name": cfg.device_name,
        "last_ok": last_ok,
        "error": error,
        "pairing": pairing,
    });
    match st {
        Ok(s) => {
            out["in_icloud"] = json!(s.in_icloud);
            out["devices"] = json!(s.devices);
            out["meetings_from_others"] = json!(s.meetings_from_others);
            out["unreadable"] = json!(s.unreadable);
        }
        Err(e) => out["error"] = json!(format!("{e:#}")),
    }
    out
}

pub fn setup(folder: Option<String>) -> anyhow::Result<Value> {
    let folder = folder.map(std::path::PathBuf::from);
    let target = folder.clone().unwrap_or_else(desktop::default_folder);
    if desktop::folder_has_vault(&target) {
        anyhow::bail!(
            "{} already holds a Milkrice vault, probably from another Mac. Choose an empty folder, or delete that folder first if nothing else uses it.",
            target.display()
        );
    }
    desktop::init(folder)?;
    wake();
    Ok(status())
}

pub fn pair_start() -> anyhow::Result<Value> {
    let p = with_vault(|_, v| Ok(Pairing::start(v)))?;
    let svg = desktop::qr_svg(&p.qr)?;
    let qr = p.qr.clone();
    let mut g = rt().0.lock();
    if let Some(old) = g.pairing.take() {
        old.cancel();
    }
    g.pairing = Some(p);
    Ok(json!({"qr": qr, "svg": svg}))
}

pub fn pair_poll() -> anyhow::Result<Option<desktop::Joining>> {
    let mut g = rt().0.lock();
    match g.pairing.as_mut() {
        Some(p) => p.poll(),
        None => anyhow::bail!("pairing was cancelled"),
    }
}

pub fn pair_accept() -> anyhow::Result<String> {
    let p = rt().0.lock().pairing.take();
    let p = p.ok_or_else(|| anyhow::anyhow!("pairing was cancelled"))?;
    let name = with_vault(|_, v| p.accept(v))?;
    wake();
    Ok(name)
}

pub fn pair_cancel() {
    if let Some(p) = rt().0.lock().pairing.take() {
        p.cancel();
    }
}

pub fn leave() -> anyhow::Result<()> {
    pair_cancel();
    desktop::leave()?;
    let mut g = rt().0.lock();
    g.vault = None;
    g.last_ok = None;
    g.error = None;
    Ok(())
}
