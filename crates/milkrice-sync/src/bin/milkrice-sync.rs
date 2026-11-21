//! `milkrice-sync`: the Mac side of iPhone sync.
//!
//!   milkrice-sync init [--folder PATH]   create the encrypted vault (default: iCloud Drive/Milkrice Sync)
//!   milkrice-sync pair                   show a QR code, confirm the 6-digit code, hand the key to the phone
//!   milkrice-sync sync                   one pass: export Mac meetings, import iPhone meetings
//!   milkrice-sync serve [--every SECS]   keep syncing, and write notes the iPhone asks for with your AI
//!   milkrice-sync status                 folder, devices, meetings
//!
//! Data lives in the Milkrice data folder (`MILKRICE_HOME` overrides it, as for the app).
//! The vault key is kept in the macOS login keychain, never in the sync folder. The Milkrice app
//! does all of this itself (Settings › iPhone sync); quit it before running `serve` here.
use anyhow::{Context, Result};
use chrono::Utc;
use milkrice_core::{llm, paths, store::Store, types::Segment};
use milkrice_sync::desktop::{self, Pairing};
use serde_json::Value;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn init(folder: Option<PathBuf>) -> Result<()> {
    let cfg = desktop::init(folder)?;
    println!("Created an encrypted Milkrice vault in\n  {}\nThe key is in this Mac's keychain (\"{}\"). Next: `milkrice-sync pair`.", cfg.folder.display(), desktop::KEYCHAIN_SERVICE);
    Ok(())
}

fn pair_cmd(timeout_secs: u64) -> Result<()> {
    let cfg = desktop::config()?;
    let vault = desktop::open_vault(&cfg)?;
    let mut p = Pairing::start(&vault);
    println!("{}\nOn the iPhone: Milkrice › Settings › Sync with Mac › Pair with my Mac, then scan this code.", desktop::qr_text(&p.qr)?);
    println!("(No camera? Paste this instead.)\nPAIRING CODE: {}\n", p.qr);
    std::io::stdout().flush()?;
    let start = Instant::now();
    let joining = loop {
        if let Some(j) = p.poll()? {
            break j;
        }
        if start.elapsed() > Duration::from_secs(timeout_secs) {
            p.cancel();
            anyhow::bail!("no iPhone answered within {timeout_secs} s; run `milkrice-sync pair` again");
        }
        std::thread::sleep(Duration::from_millis(700));
    };
    println!("\"{}\" wants to join.\n\n   Code:  {} {}\n", joining.name, &joining.code[..3], &joining.code[3..]);
    print!("Does the iPhone show the same code? Type y to give it the key: ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    if !answer.trim().eq_ignore_ascii_case("y") {
        p.cancel();
        anyhow::bail!("not paired; nothing was shared");
    }
    let name = p.accept(&vault)?;
    println!("Paired with \"{name}\". Run `milkrice-sync serve` to keep both in step.");
    Ok(())
}

fn provider() -> Option<(llm::Provider, String)> {
    let store = Store::open(&paths::db_path()).ok()?;
    let s: Value = serde_json::from_str(&store.get_setting("settings")?).ok()?;
    let p: llm::Provider = serde_json::from_value(s.get("provider")?.clone()).ok()?;
    Some((p, s.get("provider_model").and_then(|m| m.as_str()).unwrap_or("").to_string()))
}

/// Write notes for a meeting the iPhone asked about, with the AI chosen in the Mac app.
fn write_notes(store: &Store, id: &str) -> Result<String> {
    let (p, model) = provider().context("no AI is chosen in Milkrice for Mac yet (Settings › AI), so notes stay unwritten")?;
    let m = store.get_meeting(id)?.context("meeting vanished")?;
    let segs: Vec<Segment> = store.segments(id)?;
    if m.title.trim().is_empty() {
        let (sys, prompt) = llm::title_prompt(&m, &segs);
        if let Ok(t) = llm::complete(p, &model, &sys, &prompt) {
            store.set_field(id, "title", t.trim().trim_matches('"'))?;
        }
    }
    let (sys, prompt) = llm::enhance_prompt(&m, &segs);
    let out = llm::complete(p, &model, &sys, &prompt)?;
    let by = match p {
        llm::Provider::Claude => format!("claude:{}", if model.is_empty() { "haiku" } else { &model }),
        llm::Provider::Codex => format!("codex:{}", if model.is_empty() { llm::codex_default_model() } else { model.clone() }),
        llm::Provider::Ollama => format!("ollama:{model}"),
    };
    store.set_field(id, "enhanced", &out)?;
    store.set_field(id, "enhanced_by", &by)?;
    store.set_field(id, "enhanced_edited", "0")?;
    Ok(by)
}

fn run(serve: bool, every: u64) -> Result<()> {
    let cfg = desktop::config()?;
    let vault = desktop::open_vault(&cfg)?;
    let store = Store::open(&paths::db_path())?;
    loop {
        let r = desktop::sync_pass(&cfg, &vault, &store)?;
        let stamp = Utc::now().format("%H:%M:%S");
        println!("{stamp} synced: {} records changed, {} meetings updated from other devices{}", r.exported, r.imported.len(),
            if r.requests.is_empty() { String::new() } else { format!(", {} waiting for notes", r.requests.len()) });
        for p in &r.problems {
            eprintln!("  skipped unreadable file: {p}");
        }
        if serve && !r.requests.is_empty() {
            for id in &r.requests {
                match write_notes(&store, id) {
                    Ok(by) => println!("  wrote notes for {id} with {by}"),
                    Err(e) => eprintln!("  couldn't write notes for {id}: {e:#}"),
                }
            }
            desktop::sync_pass(&cfg, &vault, &store)?; // send them straight back
        }
        if !serve {
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(every));
    }
}

fn status() -> Result<()> {
    let cfg = desktop::config()?;
    let vault = desktop::open_vault(&cfg)?;
    let s = desktop::status(&cfg, &vault)?;
    println!("Folder:  {}\nThis Mac: {} ({})", s.folder.display(), s.device_name, &cfg.device_id[..8]);
    for d in &s.devices {
        println!("Device:  {} · {} · last seen {} ({})", d.name, d.kind, d.last_seen, &d.id[..8.min(d.id.len())]);
    }
    println!("Meetings from other devices: {}", s.meetings_from_others);
    if s.unreadable > 0 {
        println!("Unreadable files: {}", s.unreadable);
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    match args.first().map(|s| s.as_str()) {
        Some("init") => init(flag("--folder").map(PathBuf::from)),
        Some("pair") => pair_cmd(flag("--timeout").and_then(|s| s.parse().ok()).unwrap_or(600)),
        Some("sync") => run(false, 0),
        Some("serve") => run(true, flag("--every").and_then(|s| s.parse().ok()).unwrap_or(5)),
        Some("status") => status(),
        _ => {
            println!("{}", HELP);
            Ok(())
        }
    }
}

const HELP: &str = "milkrice-sync: end-to-end encrypted sync between Milkrice for Mac and iPhone

  milkrice-sync init [--folder PATH]   create the vault (default: iCloud Drive/Milkrice Sync)
  milkrice-sync pair [--timeout SECS]  pair an iPhone: QR code + matching 6-digit code
  milkrice-sync sync                   one sync pass
  milkrice-sync serve [--every SECS]   keep syncing; write notes the iPhone asks for with your AI
  milkrice-sync status                 folder, devices and what's synced

The folder only ever holds sealed files. The key stays in this Mac's keychain.";
