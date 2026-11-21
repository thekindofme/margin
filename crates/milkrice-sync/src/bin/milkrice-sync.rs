//! `milkrice-sync`: the Mac side of iPhone sync.
//!
//!   milkrice-sync init [--folder PATH]   create the encrypted vault (default: iCloud Drive/Milkrice Sync)
//!   milkrice-sync pair                   show a QR code, confirm the 6-digit code, hand the key to the phone
//!   milkrice-sync sync                   one pass: export Mac meetings, import iPhone meetings
//!   milkrice-sync serve [--every SECS]   keep syncing, and write notes the iPhone asks for with your AI
//!   milkrice-sync status                 folder, devices, meetings
//!
//! Data lives in the Milkrice data folder (`MILKRICE_HOME` overrides it, as for the app).
//! The vault key is kept in the macOS login keychain, never in the sync folder.
use anyhow::{bail, Context, Result};
use chrono::{SecondsFormat, Utc};
use milkrice_core::{llm, paths, store::Store, types::Segment};
use milkrice_sync::{
    crypto::{self, Key},
    folder::{DeviceCard, Vault},
    merge::{self, Clock, MeetingState, Record},
    pair,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    folder: PathBuf,
    device_id: String,
    vault_id: String,
    device_name: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SyncState {
    clock: Clock,
    meetings: BTreeMap<String, MeetingState>,
    /// Hash of the record last written per meeting, so unchanged records aren't rewritten
    /// (each rewrite is an upload for iCloud/Drive).
    #[serde(default)]
    written: BTreeMap<String, String>,
    /// When this Mac's device card was last refreshed (unix seconds).
    #[serde(default)]
    device_written_at: i64,
}

fn sync_dir() -> PathBuf {
    let d = paths::data_dir().join("sync");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn load<T: for<'a> Deserialize<'a> + Default>(name: &str) -> T {
    std::fs::read(sync_dir().join(name)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save<T: Serialize>(name: &str, v: &T) -> Result<()> {
    let p = sync_dir().join(name);
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(v)?)?;
    std::fs::rename(tmp, p)?;
    Ok(())
}

fn config() -> Result<Config> {
    let b = std::fs::read(sync_dir().join("config.json")).context("sync isn't set up on this Mac yet; run `milkrice-sync init`")?;
    Ok(serde_json::from_slice(&b)?)
}

// ---------- keychain

const SERVICE: &str = "Milkrice Sync";

fn store_key(vault_id: &str, key: &Key) -> Result<()> {
    if let Ok(path) = std::env::var("MILKRICE_SYNC_KEYFILE") {
        return Ok(std::fs::write(path, hex::encode(key))?); // tests only
    }
    security_framework::passwords::set_generic_password(SERVICE, vault_id, hex::encode(key).as_bytes())
        .context("couldn't save the sync key to the keychain")
}

fn load_key(vault_id: &str) -> Result<Key> {
    let hex_key = if let Ok(path) = std::env::var("MILKRICE_SYNC_KEYFILE") {
        std::fs::read_to_string(path)?.into_bytes()
    } else {
        security_framework::passwords::get_generic_password(SERVICE, vault_id).context("the sync key isn't in this Mac's keychain")?
    };
    hex::decode(String::from_utf8(hex_key)?.trim())?.try_into().ok().context("bad key in keychain")
}

fn open_vault(cfg: &Config) -> Result<Vault> {
    Vault::open(&cfg.folder, load_key(&cfg.vault_id)?)
}

fn default_folder() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join("Library/Mobile Documents/com~apple~CloudDocs/Milkrice Sync")
}

fn host_name() -> String {
    std::process::Command::new("scutil")
        .args(["--get", "ComputerName"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Mac".into())
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn wall_ms() -> u64 {
    Utc::now().timestamp_millis() as u64
}

// ---------- commands

fn init(folder: Option<PathBuf>) -> Result<()> {
    if let Ok(c) = config() {
        bail!("sync is already set up here (folder: {}). Delete {} to start over.", c.folder.display(), sync_dir().display());
    }
    let folder = folder.unwrap_or_else(default_folder);
    if folder.starts_with(dirs::home_dir().unwrap_or_default().join("Library/Mobile Documents")) {
        let icloud = folder.parent().unwrap_or(&folder);
        if !icloud.exists() {
            bail!("iCloud Drive isn't on for this Mac ({} is missing). Turn it on, or pass --folder.", icloud.display());
        }
    }
    std::fs::create_dir_all(&folder)?;
    let vault = Vault::create(&folder)?;
    let cfg = Config {
        folder: folder.clone(),
        device_id: hex::encode(crypto::random_bytes::<16>()),
        vault_id: crypto::b64(&vault.vault_id),
        device_name: host_name(),
    };
    store_key(&cfg.vault_id, &vault.key)?;
    save("config.json", &cfg)?;
    vault.write_device(&cfg.device_id, &DeviceCard { name: cfg.device_name.clone(), kind: "mac".into(), last_seen: now_rfc3339() })?;
    println!("Created an encrypted Milkrice vault in\n  {}\nThe key is in this Mac's keychain (\"{SERVICE}\"). Next: `milkrice-sync pair`.", folder.display());
    Ok(())
}

fn pair_cmd(timeout_secs: u64) -> Result<()> {
    let cfg = config()?;
    let vault = open_vault(&cfg)?;
    let secret: [u8; 32] = crypto::random_bytes();
    let invite = pair::Invite { vault_id: vault.vault_id, session: crypto::random_bytes(), mac_pub: pair::public_key(&secret) };
    let qr = invite.to_qr();
    let code = qrcode::QrCode::new(qr.as_bytes())?;
    let art = code.render::<qrcode::render::unicode::Dense1x2>().quiet_zone(true).build();
    println!("{art}\nOn the iPhone: Milkrice › Settings › Sync with Mac › Pair, then scan this code.");
    println!("(No camera? Paste this instead.)\nPAIRING CODE: {qr}\n");
    std::io::stdout().flush()?;

    let req_path = vault.pair_dir().join(format!("{}.req", invite.session_hex()));
    let start = Instant::now();
    let req: pair::Request = loop {
        if let Ok(b) = std::fs::read(&req_path) {
            if let Ok(r) = serde_json::from_slice(&b) {
                break r;
            }
        }
        if start.elapsed() > Duration::from_secs(timeout_secs) {
            bail!("no iPhone answered within {timeout_secs} s; run `milkrice-sync pair` again");
        }
        std::thread::sleep(Duration::from_millis(700));
    };
    let phone_pub: [u8; 32] = crypto::unb64(&req.public)?.try_into().ok().context("bad request")?;
    let d = pair::derive(&secret, &phone_pub, &invite, &phone_pub)?;
    let card: DeviceCard = serde_json::from_slice(&crypto::box_open(&d.pair_key, &invite.session, &crypto::unb64(&req.card)?)?)?;
    println!("\"{}\" wants to join.\n\n   Code:  {} {}\n", card.name, &d.code[..3], &d.code[3..]);
    print!("Does the iPhone show the same code? Type y to give it the key: ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    let _ = std::fs::remove_file(&req_path);
    if !answer.trim().eq_ignore_ascii_case("y") {
        bail!("not paired; nothing was shared");
    }
    let resp = pair::seal_grant(&d, &invite, vault.key_id, &vault.key, &crypto::random_bytes())?;
    vault.write_plain(&format!("pair/{}.resp", invite.session_hex()), &serde_json::to_vec(&resp)?)?;
    println!("Paired with \"{}\". Run `milkrice-sync serve` to keep both in step.", card.name);
    Ok(())
}

/// The Mac's view of one meeting, as sync fields.
fn local_fields(store: &Store, id: &str, known: Option<&MeetingState>) -> Result<Option<BTreeMap<String, Value>>> {
    let Some(m) = store.get_meeting(id)? else { return Ok(None) };
    let segs = store.segments(id)?;
    let transcript: Vec<Value> =
        segs.iter().map(|s| json!([s.start_ms, s.end_ms, llm::speaker_name(&s.speaker, &m.speakers), s.text])).collect();
    let mut f = BTreeMap::new();
    f.insert("title".into(), json!(m.title));
    f.insert("notes".into(), json!(m.notes));
    f.insert("enhanced".into(), json!(m.enhanced));
    f.insert("enhanced_by".into(), json!(m.enhanced_by));
    f.insert("transcript".into(), Value::Array(transcript));
    f.insert("transcribed_by".into(), json!(m.stt_model));
    f.insert("created_at".into(), json!(m.created_at.to_rfc3339_opts(SecondsFormat::Secs, true)));
    f.insert("duration_ms".into(), json!(m.duration_ms));
    f.insert("deleted".into(), json!(false));
    if known.map(|s| !s.contains_key("origin")).unwrap_or(true) {
        f.insert("origin".into(), json!("mac"));
        f.insert("mode".into(), json!("mac"));
    }
    Ok(Some(f))
}

fn db_conn() -> Result<rusqlite::Connection> {
    Ok(rusqlite::Connection::open(paths::db_path())?)
}

/// Apply another device's changes to the Mac's store.
fn apply(store: &Store, id: &str, changes: &BTreeMap<String, Value>, merged: &Record) -> Result<()> {
    let s = |k: &str| merged.fields.get(k).and_then(|x| x.v.as_str()).unwrap_or("").to_string();
    if merged.fields.get("deleted").and_then(|x| x.v.as_bool()) == Some(true) {
        if store.get_meeting(id)?.is_some() {
            store.delete_meeting(id)?;
        }
        return Ok(());
    }
    if store.get_meeting(id)?.is_none() {
        let created = chrono::DateTime::parse_from_rfc3339(&s("created_at")).map(|d| d.with_timezone(&Utc)).unwrap_or_else(|_| Utc::now());
        let m = milkrice_core::store::Meeting {
            id: id.into(),
            title: s("title"),
            created_at: created,
            started_at: Some(created),
            ended_at: Some(created),
            duration_ms: merged.fields.get("duration_ms").and_then(|x| x.v.as_u64()).unwrap_or(0),
            notes: s("notes"),
            enhanced: s("enhanced"),
            template: "auto".into(),
            status: "done".into(),
            ..Default::default()
        };
        store.create_meeting(&m)?;
        store.set_field(id, "enhanced_by", &s("enhanced_by"))?;
        store.set_field(id, "stt_model", &s("transcribed_by"))?;
        replace_transcript(id, merged.fields.get("transcript").map(|x| &x.v))?;
        return Ok(());
    }
    for (k, v) in changes {
        match k.as_str() {
            "title" | "notes" | "enhanced" | "enhanced_by" => store.set_field(id, k, v.as_str().unwrap_or(""))?,
            "transcribed_by" => store.set_field(id, "stt_model", v.as_str().unwrap_or(""))?,
            "transcript" => replace_transcript(id, Some(v))?,
            "duration_ms" => {
                db_conn()?.execute("UPDATE meetings SET duration_ms=?1 WHERE id=?2", rusqlite::params![v.as_i64().unwrap_or(0), id])?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn replace_transcript(id: &str, v: Option<&Value>) -> Result<()> {
    let mut c = db_conn()?;
    let tx = c.transaction()?;
    tx.execute("DELETE FROM segments WHERE meeting_id=?1", [id])?;
    for row in v.and_then(|v| v.as_array()).into_iter().flatten() {
        let Some(r) = row.as_array() else { continue };
        tx.execute(
            "INSERT INTO segments (meeting_id,start_ms,end_ms,source,speaker,text) VALUES (?1,?2,?3,'mic',?4,?5)",
            rusqlite::params![
                id,
                r.first().and_then(|x| x.as_i64()).unwrap_or(0),
                r.get(1).and_then(|x| x.as_i64()).unwrap_or(0),
                r.get(2).and_then(|x| x.as_str()).unwrap_or("Speaker"),
                r.get(3).and_then(|x| x.as_str()).unwrap_or("")
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

struct PassReport {
    exported: usize,
    imported: Vec<String>,
    requests: Vec<String>,
    problems: Vec<String>,
}

fn sync_pass(cfg: &Config, vault: &Vault, store: &Store) -> Result<PassReport> {
    let mut st: SyncState = load("state.json");
    if st.clock.device.is_empty() {
        st.clock = Clock::new(&cfg.device_id);
    }
    let (remote, problems) = vault.read_records(&cfg.device_id)?;
    let mut ids: Vec<String> = store
        .list_meetings(None, 100_000)?
        .into_iter()
        .filter(|m| m.status != "recording")
        .map(|m| m.id)
        .collect();
    ids.extend(remote.keys().cloned());
    ids.extend(st.meetings.keys().cloned());
    ids.sort();
    ids.dedup();

    let mut report = PassReport { exported: 0, imported: vec![], requests: vec![], problems };
    for id in ids {
        let known = st.meetings.get(&id).cloned();
        let local = match local_fields(store, &id, known.as_ref())? {
            Some(f) => f,
            // Known before but gone now: deleted on this Mac (unless it was already a tombstone).
            None if known.as_ref().map(|k| k.contains_key("title")).unwrap_or(false)
                && !remote.get(&id).map(|rs| rs.iter().any(|r| r.fields.get("deleted").map(|d| d.v == json!(true)).unwrap_or(false))).unwrap_or(false)
                && !tombstoned(known.as_ref()) =>
            {
                BTreeMap::from([("deleted".to_string(), json!(true))])
            }
            None => BTreeMap::new(),
        };
        let mut state = known.unwrap_or_default();
        let remotes = remote.get(&id).cloned().unwrap_or_default();
        let out = merge::merge(&id, &local, &mut state, &remotes, &mut st.clock, wall_ms());
        if !out.apply.is_empty() {
            apply(store, &id, &out.apply, &merged_view(&out.record, &remotes, &state))?;
            report.imported.push(id.clone());
        }
        let request = remotes.iter().filter_map(|r| r.fields.get("notes_request")).max_by(|a, b| a.hlc.cmp(&b.hlc)).map(|s| s.v.clone());
        if merge::notes_requested(&state, request.as_ref()) && store.get_meeting(&id)?.is_some() {
            report.requests.push(id.clone());
        }
        if !out.record.fields.is_empty() {
            let h = hex::encode(&<sha2::Sha256 as sha2::Digest>::digest(serde_json::to_vec(&out.record)?)[..8]);
            if st.written.get(&id) != Some(&h) || !vault.own_record_exists(&id, &cfg.device_id) {
                vault.write_record(&out.record)?;
                st.written.insert(id.clone(), h);
                report.exported += 1;
            }
        }
        st.meetings.insert(id, state);
    }
    if Utc::now().timestamp() - st.device_written_at > 600 {
        vault.write_device(&cfg.device_id, &DeviceCard { name: cfg.device_name.clone(), kind: "mac".into(), last_seen: now_rfc3339() })?;
        st.device_written_at = Utc::now().timestamp();
    }
    save("state.json", &st)?;
    Ok(report)
}

fn tombstoned(state: Option<&MeetingState>) -> bool {
    state.map(|s| s.get("deleted").map(|d| d.hash == merge::value_hash(&json!(true))).unwrap_or(false)).unwrap_or(false)
}

/// Every field's winning value: our record plus newer remote values (for creating a meeting).
fn merged_view(own: &Record, remotes: &[Record], state: &MeetingState) -> Record {
    let mut r = own.clone();
    for rem in remotes {
        for (k, s) in &rem.fields {
            if state.get(k).map(|st| st.hlc == s.hlc).unwrap_or(false) {
                r.fields.insert(k.clone(), s.clone());
            }
        }
    }
    r
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
    let cfg = config()?;
    let vault = open_vault(&cfg)?;
    let store = Store::open(&paths::db_path())?;
    loop {
        let r = sync_pass(&cfg, &vault, &store)?;
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
            sync_pass(&cfg, &vault, &store)?; // send them straight back
        }
        if !serve {
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(every));
    }
}

fn status() -> Result<()> {
    let cfg = config()?;
    let vault = open_vault(&cfg)?;
    println!("Folder:  {}\nThis Mac: {} ({})", cfg.folder.display(), cfg.device_name, &cfg.device_id[..8]);
    for (id, c) in vault.devices() {
        println!("Device:  {} · {} · last seen {} ({})", c.name, c.kind, c.last_seen, &id[..8.min(id.len())]);
    }
    let (recs, problems) = vault.read_records(&cfg.device_id)?;
    println!("Meetings from other devices: {}", recs.len());
    if !problems.is_empty() {
        println!("Unreadable files: {}", problems.len());
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
