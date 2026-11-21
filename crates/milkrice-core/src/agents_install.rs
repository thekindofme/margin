//! Install, inspect and remove Milkrice's agent integrations: the `milkrice` command on PATH, the
//! read-only MCP server registered with each agent, and the skill file.
//!
//! Shared by `milkrice agents status|install|uninstall` and the app's Settings → Agents, so both
//! behave the same. Rules every write follows:
//! - Config files are merged, never replaced: only the `milkrice` entry is added or removed.
//! - Before an existing file is changed, a copy is saved next to it as
//!   `<file>.milkrice-backup-<timestamp>`.
//! - Installing twice changes nothing the second time (no write, no backup).
//! - Uninstall removes only entries that point at a Milkrice binary, and skill folders that hold
//!   Milkrice's own SKILL.md.
use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The skill, embedded so the CLI can install it without the app's resources.
pub const SKILL_MD: &str = include_str!("../../../skills/milkrice/SKILL.md");
/// Name of the MCP server in every agent's config.
pub const SERVER: &str = "milkrice";
/// Lists which targets installed a shared skill folder, so uninstalling one keeps it for the rest.
const OWNERS_FILE: &str = ".installed-by";
const BREW_INSTALL: &str = "brew install --cask thekindofme/tap/milkrice";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    Cli,
    Claude,
    Codex,
    Cursor,
    Opencode,
    Vscode,
    Skill,
}

impl Target {
    pub const AGENTS: [Target; 5] = [Target::Claude, Target::Codex, Target::Cursor, Target::Opencode, Target::Vscode];
    pub const ALL: [Target; 7] =
        [Target::Cli, Target::Claude, Target::Codex, Target::Cursor, Target::Opencode, Target::Vscode, Target::Skill];

    pub fn parse(s: &str) -> Result<Target> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "cli" => Target::Cli,
            "claude" | "claude-code" => Target::Claude,
            "codex" => Target::Codex,
            "cursor" => Target::Cursor,
            "opencode" => Target::Opencode,
            "vscode" | "code" | "copilot" => Target::Vscode,
            "skill" => Target::Skill,
            _ => bail!("unknown agent '{s}' (expected claude, codex, cursor, opencode, vscode, skill, cli or all)"),
        })
    }

    pub fn id(self) -> &'static str {
        match self {
            Target::Cli => "cli",
            Target::Claude => "claude",
            Target::Codex => "codex",
            Target::Cursor => "cursor",
            Target::Opencode => "opencode",
            Target::Vscode => "vscode",
            Target::Skill => "skill",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Target::Cli => "milkrice command",
            Target::Claude => "Claude Code",
            Target::Codex => "Codex",
            Target::Cursor => "Cursor",
            Target::Opencode => "OpenCode",
            Target::Vscode => "VS Code (Copilot)",
            Target::Skill => "Skill for other agents",
        }
    }
}

/// Where things live. `from_system` reads the real machine; tests build one around a temp dir.
#[derive(Clone, Debug)]
pub struct Env {
    pub home: PathBuf,
    /// `$CLAUDE_CONFIG_DIR` or `~/.claude`.
    pub claude_dir: PathBuf,
    /// Claude Code's user config holding user-scope MCP servers (`~/.claude.json`).
    pub claude_json: PathBuf,
    /// `$CODEX_HOME` or `~/.codex`.
    pub codex_dir: PathBuf,
    /// `$COPILOT_HOME` or `~/.copilot` (VS Code's "Copilot Global" config and skills).
    pub copilot_dir: PathBuf,
    /// `$XDG_CONFIG_HOME/opencode` or `~/.config/opencode`.
    pub opencode_dir: PathBuf,
    /// Directories a Homebrew cask links `milkrice` into.
    pub brew_bins: Vec<PathBuf>,
    /// Folders that may hold Milkrice.app / Cursor.app / Visual Studio Code.app.
    pub app_dirs: Vec<PathBuf>,
    /// The milkrice-cli this process can link `~/.local/bin/milkrice` to.
    pub bundled_cli: Option<PathBuf>,
    /// PATH used to find agent CLIs such as `claude`.
    pub search_path: String,
    /// The PATH a new terminal gets (None if it couldn't be read).
    pub login_path: Option<String>,
    /// Allow running `claude mcp add/remove`. Off in tests.
    pub run_agent_clis: bool,
    /// Report what would change without writing anything.
    pub dry_run: bool,
}

impl Env {
    /// The real machine. `bundled_cli` is the milkrice-cli binary shipped with this build.
    pub fn from_system(bundled_cli: Option<PathBuf>) -> Env {
        let home = dirs::home_dir().unwrap_or_default();
        let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let claude_cfg = var("CLAUDE_CONFIG_DIR");
        Env {
            claude_dir: claude_cfg.clone().unwrap_or_else(|| home.join(".claude")),
            claude_json: claude_cfg.map(|d| d.join(".claude.json")).unwrap_or_else(|| home.join(".claude.json")),
            codex_dir: var("CODEX_HOME").unwrap_or_else(|| home.join(".codex")),
            copilot_dir: var("COPILOT_HOME").unwrap_or_else(|| home.join(".copilot")),
            opencode_dir: var("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config")).join("opencode"),
            brew_bins: vec!["/opt/homebrew/bin".into(), "/usr/local/bin".into()],
            app_dirs: vec!["/Applications".into(), home.join("Applications")],
            bundled_cli,
            search_path: crate::llm::shell_path().to_string(),
            login_path: crate::llm::login_shell_path().map(str::to_string),
            run_agent_clis: true,
            dry_run: false,
            home,
        }
    }

    /// Everything under `home`, nothing from the real system. For tests.
    pub fn sandbox(home: &Path) -> Env {
        Env {
            home: home.to_path_buf(),
            claude_dir: home.join(".claude"),
            claude_json: home.join(".claude.json"),
            codex_dir: home.join(".codex"),
            copilot_dir: home.join(".copilot"),
            opencode_dir: home.join(".config/opencode"),
            brew_bins: vec![home.join("brew/bin")],
            app_dirs: vec![home.join("Applications")],
            bundled_cli: None,
            search_path: String::new(),
            login_path: None,
            run_agent_clis: false,
            dry_run: false,
        }
    }

    fn local_bin(&self) -> PathBuf {
        self.home.join(".local/bin")
    }

    fn agents_skill(&self) -> PathBuf {
        self.home.join(".agents/skills/milkrice")
    }

    /// Shorten paths under home to `~/…` for messages.
    pub fn show(&self, p: &Path) -> String {
        match p.strip_prefix(&self.home) {
            Ok(rest) if !self.home.as_os_str().is_empty() => format!("~/{}", rest.display()),
            _ => p.display().to_string(),
        }
    }

    fn which(&self, bin: &str) -> Option<PathBuf> {
        if self.search_path.is_empty() {
            return None;
        }
        which::which_in(bin, Some(&self.search_path), "/").ok()
    }

    fn app_exists(&self, name: &str) -> bool {
        self.app_dirs.iter().any(|d| d.join(name).exists())
    }
}

// ------------------------------------------------------------------ the milkrice command

/// Where the `milkrice` command comes from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CliSource {
    Homebrew,
    Local,
    App,
}

/// The most stable absolute path to run Milkrice by, in order: the Homebrew link, the
/// `~/.local/bin` link, then the binary inside the app bundle (which breaks if the app moves).
pub fn resolve_command(env: &Env) -> Option<(PathBuf, CliSource)> {
    for dir in &env.brew_bins {
        let p = dir.join("milkrice");
        if p.is_file() {
            return Some((p, CliSource::Homebrew));
        }
    }
    let local = env.local_bin().join("milkrice");
    if local.is_file() {
        return Some((local, CliSource::Local));
    }
    if let Some(b) = env.bundled_cli.as_ref().filter(|b| b.is_file()) {
        return Some((b.clone(), CliSource::App));
    }
    env.app_dirs
        .iter()
        .map(|d| d.join("Milkrice.app/Contents/MacOS/milkrice-cli"))
        .find(|p| p.is_file())
        .map(|p| (p, CliSource::App))
}

fn on_login_path(env: &Env, dir: &Path) -> Option<bool> {
    let lp = env.login_path.as_ref()?;
    let want = dir.to_string_lossy();
    let want = want.trim_end_matches('/');
    let home = env.home.to_string_lossy();
    Some(lp.split(':').any(|p| {
        let p = p.trim_end_matches('/');
        p == want || p.replacen("$HOME", &home, 1) == want || p.replacen('~', &home, 1) == want
    }))
}

/// Advice when the command's folder isn't on the user's PATH (None when it is, or unknown).
pub fn path_hint(env: &Env) -> Option<String> {
    let (cmd, src) = resolve_command(env)?;
    if src == CliSource::App {
        return None;
    }
    let dir = cmd.parent()?;
    if on_login_path(env, dir)? {
        return None;
    }
    let shown = env.show(dir).replacen('~', "$HOME", 1);
    Some(format!(
        "{} isn't on your shell PATH, so typing `milkrice` in a terminal won't find it. MCP still works (it's registered with the full path). To fix it, run: echo 'export PATH=\"{shown}:$PATH\"' >> ~/.zprofile  then open a new terminal.",
        env.show(dir)
    ))
}

/// Put `milkrice` on PATH: nothing to do when Homebrew linked it, otherwise symlink
/// `~/.local/bin/milkrice` to the bundled milkrice-cli.
fn ensure_cli(env: &Env, log: &mut Log) -> Result<()> {
    if let Some(p) = env.brew_bins.iter().map(|d| d.join("milkrice")).find(|p| p.is_file()) {
        log.same(format!("`milkrice` is on PATH via Homebrew ({})", p.display()));
        return Ok(());
    }
    let dir = env.local_bin();
    let link = dir.join("milkrice");
    if link.is_file() {
        log.same(format!("`milkrice` is installed at {}", env.show(&link)));
        return Ok(());
    }
    let Some(target) = env.bundled_cli.as_ref().filter(|b| b.is_file()) else {
        bail!("can't find the milkrice-cli binary to link. Install Milkrice first ({BREW_INSTALL}) or open the app once.");
    };
    if env.dry_run {
        log.did(format!("would link {} → {}", env.show(&link), target.display()));
        return Ok(());
    }
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    remove_legacy_cli_link(&dir);
    if std::fs::symlink_metadata(&link).is_ok() {
        // A dangling link (the app moved or was deleted): replace it.
        std::fs::remove_file(&link)?;
    }
    std::os::unix::fs::symlink(target, &link).with_context(|| format!("link {}", link.display()))?;
    log.did(format!("Linked {} → {}", env.show(&link), target.display()));
    Ok(())
}

fn remove_cli(env: &Env, log: &mut Log) -> Result<()> {
    let link = env.local_bin().join("milkrice");
    match std::fs::read_link(&link) {
        Ok(t) if t.file_name().is_some_and(|n| n == "milkrice-cli") => {
            if env.dry_run {
                log.did(format!("would remove {}", env.show(&link)));
            } else {
                std::fs::remove_file(&link)?;
                log.did(format!("Removed {}", env.show(&link)));
            }
        }
        Ok(_) => log.warn(format!("{} doesn't point at milkrice-cli; left it alone", env.show(&link))),
        Err(_) if link.exists() => log.warn(format!("{} isn't a link Milkrice made; left it alone", env.show(&link))),
        Err(_) => log.same(format!("{} isn't installed", env.show(&link))),
    }
    if let Some(p) = env.brew_bins.iter().map(|d| d.join("milkrice")).find(|p| p.is_file()) {
        log.same(format!("{} belongs to Homebrew; remove it with `brew uninstall --cask milkrice`", p.display()));
    }
    Ok(())
}

/// When `~/.local/bin/milkrice` is a link into an app bundle that no longer exists (the app was
/// moved), point it at this build's milkrice-cli. Returns true if it was repaired.
pub fn repair_cli_link(home: &Path, bundled: Option<&Path>) -> bool {
    let link = home.join(".local/bin/milkrice");
    let (Ok(target), Some(bundled)) = (std::fs::read_link(&link), bundled) else { return false };
    if target.exists() || !bundled.is_file() || target.file_name().is_none_or(|n| n != "milkrice-cli") {
        return false;
    }
    std::fs::remove_file(&link).is_ok() && std::os::unix::fs::symlink(bundled, &link).is_ok()
}

// ------------------------------------------------------------------ result log

/// What one install/uninstall did, for the CLI to print and the app to toast.
#[derive(Debug, Default, Serialize)]
pub struct Log {
    /// Things that changed (or would change, in a dry run).
    pub changed: Vec<String>,
    /// Things already in the wanted state.
    pub unchanged: Vec<String>,
    pub warnings: Vec<String>,
}

impl Log {
    fn did(&mut self, s: impl Into<String>) {
        self.changed.push(s.into());
    }
    fn same(&mut self, s: impl Into<String>) {
        self.unchanged.push(s.into());
    }
    fn warn(&mut self, s: impl Into<String>) {
        self.warnings.push(s.into());
    }
}

// ------------------------------------------------------------------ files: backup, write, remove

fn backup(env: &Env, file: &Path, log: &mut Log) -> Result<()> {
    if !file.exists() || env.dry_run {
        return Ok(());
    }
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let name = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mut dest = file.with_file_name(format!("{name}.milkrice-backup-{stamp}"));
    let mut i = 1;
    while dest.exists() {
        dest = file.with_file_name(format!("{name}.milkrice-backup-{stamp}-{i}"));
        i += 1;
    }
    std::fs::copy(file, &dest).with_context(|| format!("back up {}", file.display()))?;
    log.did(format!("Backed up {} to {}", env.show(file), env.show(&dest)));
    Ok(())
}

/// Write via a temp file and rename, keeping the original file's permissions.
fn write_atomic(file: &Path, text: &str) -> Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    let tmp = file.with_file_name(format!(
        ".{}.milkrice-tmp",
        file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
    ));
    std::fs::write(&tmp, text).with_context(|| format!("write {}", tmp.display()))?;
    if let Ok(meta) = std::fs::metadata(file) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    std::fs::rename(&tmp, file).with_context(|| format!("replace {}", file.display()))?;
    Ok(())
}

// ------------------------------------------------------------------ JSON configs

/// A JSON config with MCP servers under `keys` (e.g. ["mcpServers"] or ["mcp"]).
struct JsonConfig<'a> {
    file: &'a Path,
    keys: &'a [&'a str],
}

fn read_json(file: &Path) -> Result<Option<Value>> {
    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("read {}", file.display())),
    };
    if text.trim().is_empty() {
        return Ok(Some(json!({})));
    }
    let v: Value = serde_json::from_str(&text).map_err(|e| {
        anyhow!("can't parse {} as plain JSON ({e}). If it has comments, add the milkrice entry by hand.", file.display())
    })?;
    anyhow::ensure!(v.is_object(), "{} isn't a JSON object", file.display());
    Ok(Some(v))
}

fn json_servers<'v>(root: &'v Value, keys: &[&str]) -> Option<&'v Map<String, Value>> {
    keys.iter().try_fold(root, |v, k| v.get(*k))?.as_object()
}

impl JsonConfig<'_> {
    fn get(&self) -> Option<Value> {
        let root = read_json(self.file).ok()??;
        json_servers(&root, self.keys)?.get(SERVER).cloned()
    }

    fn set(&self, env: &Env, entry: Value, log: &mut Log) -> Result<()> {
        let existing = read_json(self.file)?;
        let mut root = existing.clone().unwrap_or_else(|| json!({}));
        if json_servers(&root, self.keys).and_then(|m| m.get(SERVER)) == Some(&entry) {
            log.same(format!("MCP server already registered in {}", env.show(self.file)));
            return Ok(());
        }
        let mut cur = &mut root;
        for k in self.keys {
            let obj = cur.as_object_mut().ok_or_else(|| anyhow!("{}: '{k}' isn't an object", self.file.display()))?;
            cur = obj.entry(k.to_string()).or_insert_with(|| json!({}));
        }
        let servers = cur.as_object_mut().ok_or_else(|| anyhow!("{}: servers isn't an object", self.file.display()))?;
        if env.dry_run {
            log.did(format!("would register the MCP server in {}", env.show(self.file)));
            return Ok(());
        }
        servers.insert(SERVER.into(), entry);
        backup(env, self.file, log)?;
        write_atomic(self.file, &(serde_json::to_string_pretty(&root)? + "\n"))?;
        log.did(format!("Registered the MCP server in {}", env.show(self.file)));
        Ok(())
    }

    fn remove(&self, env: &Env, log: &mut Log) -> Result<()> {
        self.remove_where(env, SERVER, is_ours, log)
    }

    /// Remove server `name` if `ours(entry)` says Milkrice put it there.
    fn remove_where(&self, env: &Env, name: &str, ours: impl Fn(&Value) -> bool, log: &mut Log) -> Result<()> {
        let Some(mut root) = read_json(self.file)? else {
            log.same(format!("{} doesn't exist", env.show(self.file)));
            return Ok(());
        };
        let Some(entry) = json_servers(&root, self.keys).and_then(|m| m.get(name)) else {
            log.same(format!("No {name} MCP server in {}", env.show(self.file)));
            return Ok(());
        };
        if !ours(entry) {
            log.warn(format!("The '{name}' entry in {} doesn't run a Milkrice binary; left it alone", env.show(self.file)));
            return Ok(());
        }
        if env.dry_run {
            log.did(format!("would remove the '{name}' MCP server from {}", env.show(self.file)));
            return Ok(());
        }
        let mut cur = &mut root;
        for k in self.keys {
            cur = cur.get_mut(*k).expect("checked above");
        }
        cur.as_object_mut().expect("checked above").remove(name);
        backup(env, self.file, log)?;
        if only_empty_objects(&root) {
            std::fs::remove_file(self.file)?;
            log.did(format!("Removed {} (it held only the {name} entry)", env.show(self.file)));
        } else {
            write_atomic(self.file, &(serde_json::to_string_pretty(&root)? + "\n"))?;
            log.did(format!("Removed the '{name}' MCP server from {}", env.show(self.file)));
        }
        Ok(())
    }
}

fn only_empty_objects(v: &Value) -> bool {
    v.as_object().is_some_and(|m| m.values().all(only_empty_objects))
}

/// The command an MCP entry runs (a string, or the first item of an array for OpenCode).
fn entry_command(entry: &Value) -> Option<String> {
    match entry.get("command")? {
        Value::String(s) => Some(s.clone()),
        Value::Array(a) => a.first()?.as_str().map(str::to_string),
        _ => None,
    }
}

/// True if an MCP entry runs a Milkrice binary (milkrice, milkrice-cli, or the plugin shim).
fn is_ours(entry: &Value) -> bool {
    entry_command(entry).is_some_and(|c| is_milkrice_bin(&c))
}

fn is_milkrice_bin(cmd: &str) -> bool {
    Path::new(cmd).file_name().is_some_and(|n| n == "milkrice" || n == "milkrice-cli" || n == "milkrice-mcp")
}

// ------------------------------------------------------------------ Codex TOML

fn codex_config(env: &Env) -> PathBuf {
    env.codex_dir.join("config.toml")
}

fn read_toml(file: &Path) -> Result<Option<toml_edit::DocumentMut>> {
    match std::fs::read_to_string(file) {
        Ok(t) => Ok(Some(t.parse().map_err(|e| anyhow!("can't parse {}: {e}", file.display()))?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("read {}", file.display())),
    }
}

fn codex_entry(doc: &toml_edit::DocumentMut, name: &str) -> Option<(String, Vec<String>)> {
    let t = doc.get("mcp_servers")?.as_table_like()?.get(name)?.as_table_like()?;
    let cmd = t.get("command")?.as_str()?.to_string();
    let args = t
        .get("args")
        .and_then(|a| a.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    Some((cmd, args))
}

fn codex_get(env: &Env) -> Option<(String, Vec<String>)> {
    codex_entry(&read_toml(&codex_config(env)).ok()??, SERVER)
}

fn codex_set(env: &Env, cmd: &str, log: &mut Log) -> Result<()> {
    let file = codex_config(env);
    let mut doc = read_toml(&file)?.unwrap_or_default();
    if codex_entry(&doc, SERVER) == Some((cmd.to_string(), vec!["mcp".into()])) {
        log.same(format!("MCP server already registered in {}", env.show(&file)));
        return Ok(());
    }
    if env.dry_run {
        log.did(format!("would register the MCP server in {}", env.show(&file)));
        return Ok(());
    }
    let servers = doc.entry("mcp_servers").or_insert_with(|| {
        let mut t = toml_edit::Table::new();
        t.set_implicit(true);
        toml_edit::Item::Table(t)
    });
    let servers = servers.as_table_like_mut().ok_or_else(|| anyhow!("{}: mcp_servers isn't a table", file.display()))?;
    let mut t = toml_edit::Table::new();
    t.insert("command", toml_edit::value(cmd));
    let mut args = toml_edit::Array::new();
    args.push("mcp");
    t.insert("args", toml_edit::value(args));
    servers.insert(SERVER, toml_edit::Item::Table(t));
    backup(env, &file, log)?;
    write_atomic(&file, &doc.to_string())?;
    log.did(format!("Registered the MCP server in {}", env.show(&file)));
    Ok(())
}

fn codex_remove(env: &Env, name: &str, only_if: impl Fn(&str) -> bool, log: &mut Log) -> Result<()> {
    let file = codex_config(env);
    let Some(mut doc) = read_toml(&file)? else { return Ok(()) };
    let Some((cmd, _)) = codex_entry(&doc, name) else {
        if name == SERVER {
            log.same(format!("No milkrice MCP server in {}", env.show(&file)));
        }
        return Ok(());
    };
    if !only_if(&cmd) {
        log.warn(format!("The '{name}' entry in {} doesn't run a Milkrice binary; left it alone", env.show(&file)));
        return Ok(());
    }
    if env.dry_run {
        log.did(format!("would remove the '{name}' MCP server from {}", env.show(&file)));
        return Ok(());
    }
    let servers = doc["mcp_servers"].as_table_like_mut().expect("checked above");
    servers.remove(name);
    if servers.is_empty() {
        doc.remove("mcp_servers");
    }
    backup(env, &file, log)?;
    let text = doc.to_string();
    if text.trim().is_empty() {
        std::fs::remove_file(&file)?;
        log.did(format!("Removed {} (it held only the {name} entry)", env.show(&file)));
    } else {
        write_atomic(&file, &text)?;
        log.did(format!("Removed the '{name}' MCP server from {}", env.show(&file)));
    }
    Ok(())
}

// ------------------------------------------------------------------ Claude Code

/// The Milkrice Claude Code plugin's id (e.g. "milkrice@milkrice") if it's installed and enabled.
pub fn claude_plugin(env: &Env) -> Option<String> {
    let installed = read_json(&env.claude_dir.join("plugins/installed_plugins.json")).ok()??;
    let id = installed.get("plugins")?.as_object()?.keys().find(|k| k.starts_with("milkrice@"))?.clone();
    let settings = read_json(&env.claude_dir.join("settings.json")).ok().flatten();
    let disabled = settings
        .as_ref()
        .and_then(|s| s.get("enabledPlugins")?.get(&id)?.as_bool())
        .is_some_and(|on| !on);
    (!disabled).then_some(id)
}

fn claude_json(env: &Env) -> JsonConfig<'_> {
    JsonConfig { file: &env.claude_json, keys: &["mcpServers"] }
}

fn run_cli(env: &Env, bin: &Path, args: &[&str]) -> Result<()> {
    let out = Command::new(bin)
        .args(args)
        .env("PATH", &env.search_path)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("run {}", bin.display()))?;
    anyhow::ensure!(
        out.status.success(),
        "{} {} failed: {}",
        bin.display(),
        args.join(" "),
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(())
}

fn claude_set(env: &Env, cmd: &str, log: &mut Log) -> Result<()> {
    let want = json!({"type": "stdio", "command": cmd, "args": ["mcp"], "env": {}});
    let cfg = claude_json(env);
    let current = cfg.get();
    if current.as_ref().is_some_and(|e| entry_command(e).as_deref() == Some(cmd) && e.get("args") == Some(&json!(["mcp"]))) {
        log.same(format!("MCP server already registered in {}", env.show(&env.claude_json)));
        return Ok(());
    }
    match env.which("claude").filter(|_| env.run_agent_clis) {
        Some(claude) => {
            if env.dry_run {
                log.did(format!("would run: claude mcp add -s user {SERVER} -- {cmd} mcp"));
                return Ok(());
            }
            if current.is_some() {
                run_cli(env, &claude, &["mcp", "remove", "-s", "user", SERVER])?;
            }
            run_cli(env, &claude, &["mcp", "add", "-s", "user", SERVER, "--", cmd, "mcp"])?;
            log.did(format!("Ran: claude mcp add -s user {SERVER} -- {cmd} mcp"));
            Ok(())
        }
        None => cfg.set(env, want, log),
    }
}

/// Remove Claude Code's user-scope server `name` if `ours` matches it: with `claude mcp remove`
/// when the CLI is available, otherwise by editing ~/.claude.json.
fn claude_remove(env: &Env, name: &str, ours: impl Fn(&Value) -> bool, log: &mut Log) -> Result<()> {
    let cfg = claude_json(env);
    let entry = read_json(cfg.file)?.and_then(|r| json_servers(&r, cfg.keys)?.get(name).cloned());
    match (entry, env.which("claude").filter(|_| env.run_agent_clis && !env.dry_run)) {
        (Some(e), Some(claude)) if ours(&e) => {
            run_cli(env, &claude, &["mcp", "remove", "-s", "user", name])?;
            log.did(format!("Ran: claude mcp remove -s user {name}"));
            Ok(())
        }
        _ => cfg.remove_where(env, name, ours, log),
    }
}

// ------------------------------------------------------------------ skills

fn is_our_skill(text: &str) -> bool {
    text.lines().take(4).any(|l| l.trim() == "name: milkrice")
}

fn read_owners(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join(OWNERS_FILE))
        .map(|t| t.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect())
        .unwrap_or_default()
}

fn install_skill(env: &Env, dir: &Path, owner: Target, log: &mut Log) -> Result<()> {
    let file = dir.join("SKILL.md");
    let current = std::fs::read_to_string(&file).ok();
    if let Some(t) = &current {
        if !is_our_skill(t) {
            bail!("{} exists and isn't Milkrice's skill; not overwriting it", env.show(&file));
        }
    }
    let mut owners = read_owners(dir);
    let new_owner = !owners.iter().any(|o| o == owner.id());
    if current.as_deref() == Some(SKILL_MD) && !new_owner {
        log.same(format!("Skill up to date at {}", env.show(&file)));
        return Ok(());
    }
    if env.dry_run {
        log.did(format!("would write the skill to {}", env.show(&file)));
        return Ok(());
    }
    std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    if current.as_deref() != Some(SKILL_MD) {
        std::fs::write(&file, SKILL_MD).with_context(|| format!("write {}", file.display()))?;
        log.did(format!("{} the skill at {}", if current.is_some() { "Updated" } else { "Installed" }, env.show(&file)));
    } else {
        log.same(format!("Skill up to date at {}", env.show(&file)));
    }
    if new_owner {
        owners.push(owner.id().into());
        std::fs::write(dir.join(OWNERS_FILE), owners.join("\n") + "\n")?;
    }
    Ok(())
}

fn uninstall_skill(env: &Env, dir: &Path, owner: Target, log: &mut Log) -> Result<()> {
    let file = dir.join("SKILL.md");
    let Ok(text) = std::fs::read_to_string(&file) else {
        log.same(format!("No skill at {}", env.show(dir)));
        return Ok(());
    };
    if !is_our_skill(&text) {
        log.warn(format!("{} isn't Milkrice's skill; left it alone", env.show(&file)));
        return Ok(());
    }
    let owners: Vec<String> = read_owners(dir).into_iter().filter(|o| o != owner.id()).collect();
    if !owners.is_empty() {
        if !env.dry_run {
            std::fs::write(dir.join(OWNERS_FILE), owners.join("\n") + "\n")?;
        }
        log.same(format!("Kept the skill at {} (still used by: {})", env.show(dir), owners.join(", ")));
        return Ok(());
    }
    if env.dry_run {
        log.did(format!("would remove the skill at {}", env.show(dir)));
        return Ok(());
    }
    std::fs::remove_file(&file)?;
    let _ = std::fs::remove_file(dir.join(OWNERS_FILE));
    let _ = std::fs::remove_dir(dir); // only if now empty
    log.did(format!("Removed the skill at {}", env.show(dir)));
    Ok(())
}

/// Skill folders each target installs into. `~/.agents/skills` is shared (Codex, Cursor and
/// OpenCode all read it); the owners file keeps it until the last of them is uninstalled.
fn skill_dirs(env: &Env, t: Target) -> Vec<PathBuf> {
    match t {
        Target::Cli => vec![],
        Target::Claude => vec![env.claude_dir.join("skills/milkrice")],
        Target::Codex => vec![env.agents_skill(), env.codex_dir.join("skills/milkrice")],
        Target::Cursor | Target::Opencode => vec![env.agents_skill()],
        Target::Vscode => vec![env.copilot_dir.join("skills/milkrice")],
        Target::Skill => {
            let mut v = vec![env.agents_skill()];
            if claude_plugin(env).is_none() {
                v.push(env.claude_dir.join("skills/milkrice"));
            }
            v
        }
    }
}

// ------------------------------------------------------------------ per-agent MCP configs

fn cursor_cfg(env: &Env) -> PathBuf {
    env.home.join(".cursor/mcp.json")
}

fn copilot_cfg(env: &Env) -> PathBuf {
    env.copilot_dir.join("mcp-config.json")
}

/// OpenCode's global config: opencode.json, or opencode.jsonc if that's the only one.
fn opencode_cfg(env: &Env) -> PathBuf {
    let json = env.opencode_dir.join("opencode.json");
    let jsonc = env.opencode_dir.join("opencode.jsonc");
    if !json.exists() && jsonc.exists() {
        jsonc
    } else {
        json
    }
}

/// OpenCode keeps servers under `mcp.servers` in newer configs and directly under `mcp` in older
/// ones. Use whichever the file already uses; default to the older form, which both read.
fn opencode_keys(file: &Path) -> &'static [&'static str] {
    let nested = read_json(file).ok().flatten().is_some_and(|r| r.get("mcp").and_then(|m| m.get("servers")).is_some_and(Value::is_object));
    if nested {
        &["mcp", "servers"]
    } else {
        &["mcp"]
    }
}

fn opencode_entry(cmd: &str, nested: bool) -> Value {
    if nested {
        json!({"type": "local", "command": [cmd, "mcp"]})
    } else {
        json!({"type": "local", "command": [cmd, "mcp"], "enabled": true})
    }
}

/// The MCP entry registered for a target, and the file it lives in (None if not registered).
fn mcp_entry(env: &Env, t: Target) -> Option<(PathBuf, Value)> {
    let pick = |file: PathBuf, keys: &[&str]| {
        let e = JsonConfig { file: &file, keys }.get()?;
        Some((file, e))
    };
    match t {
        Target::Claude => pick(env.claude_json.clone(), &["mcpServers"]),
        Target::Codex => {
            let (cmd, args) = codex_get(env)?;
            Some((codex_config(env), json!({"command": cmd, "args": args})))
        }
        Target::Cursor => pick(cursor_cfg(env), &["mcpServers"]),
        Target::Vscode => pick(copilot_cfg(env), &["mcpServers"]),
        Target::Opencode => {
            let f = opencode_cfg(env);
            let keys = opencode_keys(&f);
            pick(f, keys)
        }
        Target::Cli | Target::Skill => None,
    }
}

// ------------------------------------------------------------------ status

#[derive(Debug, Serialize)]
pub struct McpStatus {
    pub file: String,
    pub command: Option<String>,
    /// The command it runs exists.
    pub works: bool,
}

#[derive(Debug, Serialize)]
pub struct AgentStatus {
    pub target: Target,
    pub name: &'static str,
    /// The agent seems to be installed on this Mac.
    pub detected: bool,
    pub mcp: Option<McpStatus>,
    /// Skill files Milkrice installed for this agent that are present.
    pub skills: Vec<String>,
    /// Everything for this target is in place and working.
    pub connected: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Status {
    pub version: &'static str,
    /// Absolute path agents are registered with (Homebrew link, ~/.local/bin link or app bundle).
    pub command: Option<String>,
    pub command_source: Option<CliSource>,
    /// `milkrice` is installed as a command (Homebrew or ~/.local/bin), not just inside the app.
    pub cli_installed: bool,
    /// Its folder is on the user's shell PATH (None if unknown).
    pub on_path: Option<bool>,
    pub path_hint: Option<String>,
    /// This build ships milkrice-cli, so the CLI can be linked.
    pub cli_bundled: bool,
    /// Where `install cli` links to.
    pub cli_link: String,
    /// The Claude Code plugin id if it's installed.
    pub claude_plugin: Option<String>,
    pub agents: Vec<AgentStatus>,
}

fn detected(env: &Env, t: Target) -> bool {
    match t {
        Target::Cli | Target::Skill => true,
        Target::Claude => env.which("claude").is_some() || env.claude_dir.is_dir() || env.claude_json.is_file(),
        Target::Codex => env.which("codex").is_some() || env.codex_dir.is_dir(),
        Target::Cursor => env.which("cursor").is_some() || env.home.join(".cursor").is_dir() || env.app_exists("Cursor.app"),
        Target::Opencode => env.which("opencode").is_some() || env.opencode_dir.is_dir(),
        Target::Vscode => {
            env.which("code").is_some() || env.app_exists("Visual Studio Code.app") || env.copilot_dir.is_dir()
        }
    }
}

pub fn status(env: &Env) -> Status {
    let resolved = resolve_command(env);
    let plugin = claude_plugin(env);
    let agents = Target::AGENTS
        .iter()
        .chain(std::iter::once(&Target::Skill))
        .map(|&t| agent_status(env, t, plugin.as_deref()))
        .collect();
    let on_path = resolved.as_ref().and_then(|(c, _)| on_login_path(env, c.parent()?));
    Status {
        version: env!("CARGO_PKG_VERSION"),
        command: resolved.as_ref().map(|(p, _)| p.display().to_string()),
        command_source: resolved.as_ref().map(|(_, s)| s.clone()),
        cli_installed: resolved.as_ref().is_some_and(|(_, s)| *s != CliSource::App),
        on_path,
        path_hint: path_hint(env),
        cli_bundled: env.bundled_cli.as_ref().is_some_and(|b| b.is_file()),
        cli_link: env.show(&env.local_bin().join("milkrice")),
        claude_plugin: plugin,
        agents,
    }
}

fn agent_status(env: &Env, t: Target, plugin: Option<&str>) -> AgentStatus {
    let mut warnings = vec![];
    let mcp = mcp_entry(env, t).map(|(file, e)| {
        let command = entry_command(&e);
        let works = command.as_deref().is_some_and(|c| Path::new(c).is_file());
        if let Some(c) = &command {
            if !is_milkrice_bin(c) {
                warnings.push(format!("the 'milkrice' MCP entry in {} runs {c}, which isn't Milkrice", env.show(&file)));
            } else if !works {
                warnings.push(format!("the MCP entry points at {c}, which no longer exists; run `milkrice agents install {}` again", t.id()));
            } else if c.contains(".app/Contents/MacOS/") {
                warnings.push(format!(
                    "the MCP entry uses the path inside the app bundle, which breaks if the app moves; run `milkrice agents install {}` again to use a stable path",
                    t.id()
                ));
            }
        }
        McpStatus { file: env.show(&file), command, works }
    });
    let dirs = skill_dirs(env, t);
    let skills: Vec<String> = dirs
        .iter()
        .map(|d| d.join("SKILL.md"))
        .filter(|f| std::fs::read_to_string(f).is_ok_and(|s| is_our_skill(&s)))
        .map(|f| env.show(&f))
        .collect();
    let skills_ok = skills.len() == dirs.len();
    let mut connected = match t {
        Target::Skill => skills_ok,
        _ => mcp.as_ref().is_some_and(|m| m.works) && skills_ok,
    };
    if t == Target::Claude {
        if let Some(id) = plugin {
            if mcp.is_some() || !skills.is_empty() {
                warnings.push(format!(
                    "the Claude Code plugin ({id}) is installed and Milkrice is also registered directly, so Claude sees it twice. Run `milkrice agents uninstall claude` to keep just the plugin."
                ));
            }
            connected = true;
        }
    }
    AgentStatus { target: t, name: t.label(), detected: detected(env, t), mcp, skills, connected, warnings }
}

// ------------------------------------------------------------------ install / uninstall

/// The targets `all` stands for: every detected agent plus the shared skill.
pub fn expand(env: &Env, arg: &str, installing: bool) -> Result<Vec<Target>> {
    if arg != "all" {
        return Ok(vec![Target::parse(arg)?]);
    }
    if !installing {
        return Ok(Target::ALL.iter().rev().copied().collect()); // cli last
    }
    let mut v: Vec<Target> = Target::AGENTS.iter().copied().filter(|&t| detected(env, t)).collect();
    if claude_plugin(env).is_some() {
        v.retain(|&t| t != Target::Claude);
    }
    v.push(Target::Skill);
    Ok(v)
}

/// Install one target. Everything except `cli` first makes sure the `milkrice` command exists,
/// because the skill tells agents to run it.
pub fn install(env: &Env, t: Target) -> Result<Log> {
    let mut log = Log::default();
    if t == Target::Claude {
        if let Some(id) = claude_plugin(env) {
            bail!(
                "the Milkrice plugin for Claude Code ({id}) is installed and already provides the MCP server and the skill. To use a direct install instead, run `claude plugin uninstall {id}` first."
            );
        }
    }
    ensure_cli(env, &mut log)?;
    if t == Target::Cli {
        if let Some(h) = path_hint(env) {
            log.warn(h);
        }
        return Ok(log);
    }
    let cmd = resolve_command(env)
        .map(|(p, _)| p.display().to_string())
        .ok_or_else(|| anyhow!("can't find the milkrice command"))?;
    match t {
        Target::Claude => {
            remove_legacy(env);
            claude_set(env, &cmd, &mut log)?;
        }
        Target::Codex => {
            remove_legacy(env);
            codex_set(env, &cmd, &mut log)?;
        }
        Target::Cursor => JsonConfig { file: &cursor_cfg(env), keys: &["mcpServers"] }.set(
            env,
            json!({"type": "stdio", "command": cmd, "args": ["mcp"]}),
            &mut log,
        )?,
        Target::Vscode => JsonConfig { file: &copilot_cfg(env), keys: &["mcpServers"] }.set(
            env,
            json!({"type": "stdio", "command": cmd, "args": ["mcp"], "tools": ["*"]}),
            &mut log,
        )?,
        Target::Opencode => {
            let f = opencode_cfg(env);
            let keys = opencode_keys(&f);
            let entry = opencode_entry(&cmd, keys.len() == 2);
            JsonConfig { file: &f, keys }.set(env, entry, &mut log)?
        }
        Target::Cli | Target::Skill => {}
    }
    for dir in skill_dirs(env, t) {
        install_skill(env, &dir, t, &mut log)?;
    }
    if t == Target::Skill && claude_plugin(env).is_some() {
        log.same("Skipped ~/.claude/skills: the Claude Code plugin already provides the skill");
    }
    if let Some(h) = path_hint(env) {
        log.warn(h);
    }
    Ok(log)
}

pub fn uninstall(env: &Env, t: Target) -> Result<Log> {
    let mut log = Log::default();
    match t {
        Target::Cli => return remove_cli(env, &mut log).map(|_| log),
        Target::Claude => claude_remove(env, SERVER, is_ours, &mut log)?,
        Target::Codex => codex_remove(env, SERVER, is_milkrice_bin, &mut log)?,
        Target::Cursor => JsonConfig { file: &cursor_cfg(env), keys: &["mcpServers"] }.remove(env, &mut log)?,
        Target::Vscode => JsonConfig { file: &copilot_cfg(env), keys: &["mcpServers"] }.remove(env, &mut log)?,
        Target::Opencode => {
            let f = opencode_cfg(env);
            let keys = opencode_keys(&f);
            JsonConfig { file: &f, keys }.remove(env, &mut log)?
        }
        Target::Skill => {}
    }
    // For `skill`, also cover ~/.claude/skills even when the plugin hid it from skill_dirs.
    let mut dirs = skill_dirs(env, t);
    if t == Target::Skill && !dirs.contains(&env.claude_dir.join("skills/milkrice")) {
        dirs.push(env.claude_dir.join("skills/milkrice"));
    }
    for dir in dirs {
        uninstall_skill(env, &dir, t, &mut log)?;
    }
    if t == Target::Claude {
        if let Some(id) = claude_plugin(env) {
            log.same(format!("The Claude Code plugin ({id}) is still installed; remove it with `claude plugin uninstall {id}`"));
        }
    }
    Ok(log)
}

// ------------------------------------------------------------------ pre-rename (Margin) cleanup

/// True if a symlink target is the pre-rename (Margin) CLI: inside a Margin.app bundle, or a
/// binary called margin-cli. Anything else (a user's own `margin` tool) is left alone.
fn is_legacy_cli_target(target: &Path) -> bool {
    let in_old_app = target.components().any(|c| c.as_os_str() == "Margin.app");
    in_old_app || target.file_name().is_some_and(|n| n == "margin-cli")
}

/// Remove ~/.local/bin/margin only when it is a symlink to the old Margin CLI.
fn remove_legacy_cli_link(dir: &Path) {
    let old = dir.join("margin");
    if let Ok(target) = std::fs::read_link(&old) {
        if is_legacy_cli_target(&target) {
            let _ = std::fs::remove_file(&old);
        }
    }
}

/// Remove the old skill folder (~/.claude/skills/margin etc.) if it is the one Margin installed.
fn remove_legacy_skill(dir: &Path) {
    let skill = dir.join("SKILL.md");
    if std::fs::read_to_string(&skill).is_ok_and(|t| t.lines().take(3).any(|l| l.trim() == "name: margin")) {
        let _ = std::fs::remove_file(&skill);
        let _ = std::fs::remove_dir(dir); // only succeeds if now empty
    }
}

/// Clean up what the app installed before it was renamed from Margin. Best effort.
fn remove_legacy(env: &Env) {
    if env.dry_run {
        return;
    }
    let legacy = |e: &Value| entry_command(e).is_some_and(|c| c.contains("margin-cli"));
    let _ = claude_remove(env, "margin", legacy, &mut Log::default());
    let _ = codex_remove(env, "margin", |c| c.contains("margin-cli"), &mut Log::default());
    remove_legacy_skill(&env.claude_dir.join("skills/margin"));
    remove_legacy_skill(&env.codex_dir.join("skills/margin"));
}

// ------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;

    struct Sandbox {
        root: PathBuf,
        env: Env,
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// A temp HOME with a fake app bundle holding milkrice-cli. Never touches the real ~.
    fn sandbox() -> Sandbox {
        let root = std::env::temp_dir().join(format!("milkrice-agents-{}", uuid::Uuid::new_v4()));
        let cli = root.join("Applications/Milkrice.app/Contents/MacOS/milkrice-cli");
        std::fs::create_dir_all(cli.parent().unwrap()).unwrap();
        std::fs::write(&cli, "#!/bin/sh\n").unwrap();
        let mut env = Env::sandbox(&root);
        env.bundled_cli = Some(cli);
        Sandbox { root, env }
    }

    fn backups(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .map(|r| {
                r.filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .filter(|n| n.contains(".milkrice-backup-"))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn read(p: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    #[test]
    fn parses_targets() {
        assert_eq!(Target::parse("Claude").unwrap(), Target::Claude);
        assert_eq!(Target::parse("vscode").unwrap(), Target::Vscode);
        assert!(Target::parse("emacs").is_err());
    }

    #[test]
    fn embedded_skill_is_ours() {
        assert!(is_our_skill(SKILL_MD));
    }

    #[test]
    fn prefers_brew_then_local_then_app() {
        let s = sandbox();
        let env = &s.env;
        let (p, src) = resolve_command(env).unwrap();
        assert_eq!((p, src), (env.bundled_cli.clone().unwrap(), CliSource::App));
        install(env, Target::Cli).unwrap();
        assert_eq!(resolve_command(env).unwrap().1, CliSource::Local);
        let brew = env.brew_bins[0].join("milkrice");
        std::fs::create_dir_all(brew.parent().unwrap()).unwrap();
        std::fs::write(&brew, "").unwrap();
        assert_eq!(resolve_command(env).unwrap(), (brew, CliSource::Homebrew));
    }

    #[test]
    fn cli_link_points_at_the_bundle_and_uninstalls() {
        let s = sandbox();
        let env = &s.env;
        let link = env.home.join(".local/bin/milkrice");
        install(env, Target::Cli).unwrap();
        assert_eq!(std::fs::read_link(&link).unwrap(), env.bundled_cli.clone().unwrap());
        let again = install(env, Target::Cli).unwrap();
        assert!(again.changed.is_empty(), "second install changes nothing: {again:?}");
        uninstall(env, Target::Cli).unwrap();
        assert!(std::fs::symlink_metadata(&link).is_err());
    }

    #[test]
    fn brew_users_get_no_local_link() {
        let s = sandbox();
        let env = &s.env;
        let brew = env.brew_bins[0].join("milkrice");
        std::fs::create_dir_all(brew.parent().unwrap()).unwrap();
        std::fs::write(&brew, "").unwrap();
        install(env, Target::Cursor).unwrap();
        assert!(std::fs::symlink_metadata(env.home.join(".local/bin/milkrice")).is_err());
        let cfg = read(&env.home.join(".cursor/mcp.json"));
        assert_eq!(cfg["mcpServers"]["milkrice"]["command"], json!(brew.display().to_string()));
    }

    #[test]
    fn connecting_an_agent_installs_the_cli_and_registers_the_stable_path() {
        let s = sandbox();
        let env = &s.env;
        install(env, Target::Cursor).unwrap();
        let link = env.home.join(".local/bin/milkrice");
        assert!(link.is_file(), "Connect also installs the CLI");
        let cfg = read(&env.home.join(".cursor/mcp.json"));
        let cmd = cfg["mcpServers"]["milkrice"]["command"].as_str().unwrap();
        assert_eq!(cmd, link.display().to_string(), "not the app-bundle path");
        assert_eq!(cfg["mcpServers"]["milkrice"]["args"], json!(["mcp"]));
    }

    #[test]
    fn repairs_a_dangling_link_after_the_app_moves() {
        let s = sandbox();
        let env = &s.env;
        let dir = env.home.join(".local/bin");
        std::fs::create_dir_all(&dir).unwrap();
        std::os::unix::fs::symlink(env.home.join("Old/Milkrice.app/Contents/MacOS/milkrice-cli"), dir.join("milkrice")).unwrap();
        assert!(repair_cli_link(&env.home, env.bundled_cli.as_deref()));
        assert_eq!(std::fs::read_link(dir.join("milkrice")).unwrap(), env.bundled_cli.clone().unwrap());
        assert!(!repair_cli_link(&env.home, env.bundled_cli.as_deref()), "a working link is left alone");
    }

    #[test]
    fn cursor_merge_keeps_other_servers_backs_up_and_reverts() {
        let s = sandbox();
        let env = &s.env;
        let file = env.home.join(".cursor/mcp.json");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let original = "{\n  \"zeta\": 1,\n  \"mcpServers\": {\n    \"other\": {\"command\": \"other-mcp\"}\n  }\n}\n";
        std::fs::write(&file, original).unwrap();

        install(env, Target::Cursor).unwrap();
        let cfg = read(&file);
        assert_eq!(cfg["mcpServers"]["other"]["command"], "other-mcp");
        assert_eq!(cfg["zeta"], 1);
        assert!(cfg["mcpServers"]["milkrice"].is_object());
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.find("zeta").unwrap() < text.find("mcpServers").unwrap(), "key order kept");
        let b = backups(file.parent().unwrap());
        assert_eq!(b.len(), 1);
        assert_eq!(std::fs::read_to_string(file.parent().unwrap().join(&b[0])).unwrap(), original);

        // Idempotent: no new write, no new backup.
        let log = install(env, Target::Cursor).unwrap();
        assert!(log.changed.is_empty(), "{log:?}");
        assert_eq!(backups(file.parent().unwrap()).len(), 1);

        uninstall(env, Target::Cursor).unwrap();
        let cfg = read(&file);
        assert!(cfg["mcpServers"].get("milkrice").is_none());
        assert_eq!(cfg["mcpServers"]["other"]["command"], "other-mcp");
        assert_eq!(cfg["zeta"], 1);
    }

    #[test]
    fn uninstall_deletes_a_file_that_only_held_milkrice() {
        let s = sandbox();
        let env = &s.env;
        install(env, Target::Cursor).unwrap();
        let file = env.home.join(".cursor/mcp.json");
        assert!(file.is_file());
        assert!(backups(file.parent().unwrap()).is_empty(), "nothing to back up for a new file");
        uninstall(env, Target::Cursor).unwrap();
        assert!(!file.exists());
    }

    #[test]
    fn uninstall_leaves_a_foreign_milkrice_entry() {
        let s = sandbox();
        let env = &s.env;
        let file = env.home.join(".cursor/mcp.json");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, r#"{"mcpServers":{"milkrice":{"command":"/usr/bin/something-else"}}}"#).unwrap();
        let log = uninstall(env, Target::Cursor).unwrap();
        assert!(!log.warnings.is_empty());
        assert_eq!(read(&file)["mcpServers"]["milkrice"]["command"], "/usr/bin/something-else");
    }

    #[test]
    fn refuses_to_edit_json_with_comments() {
        let s = sandbox();
        let env = &s.env;
        let file = env.home.join(".cursor/mcp.json");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let text = "{ // mine\n \"mcpServers\": {} }";
        std::fs::write(&file, text).unwrap();
        assert!(install(env, Target::Cursor).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), text, "untouched");
    }

    #[test]
    fn codex_toml_merge_keeps_comments_and_reverts() {
        let s = sandbox();
        let env = &s.env;
        let file = env.codex_dir.join("config.toml");
        std::fs::create_dir_all(&env.codex_dir).unwrap();
        let original = "# my settings\nmodel = \"gpt-5\"\n\n[mcp_servers.other]\ncommand = \"other\"\n";
        std::fs::write(&file, original).unwrap();

        install(env, Target::Codex).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("# my settings") && text.contains("[mcp_servers.other]"));
        assert!(text.contains("[mcp_servers.milkrice]"), "{text}");
        let doc: toml_edit::DocumentMut = text.parse().unwrap();
        assert_eq!(codex_entry(&doc, "milkrice").unwrap().1, vec!["mcp".to_string()]);
        assert_eq!(backups(&env.codex_dir).len(), 1);
        // Skill in both locations.
        assert!(env.home.join(".agents/skills/milkrice/SKILL.md").is_file());
        assert!(env.codex_dir.join("skills/milkrice/SKILL.md").is_file());

        assert!(install(env, Target::Codex).unwrap().changed.is_empty());

        uninstall(env, Target::Codex).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
        assert!(!env.home.join(".agents/skills/milkrice").exists());
        assert!(!env.codex_dir.join("skills/milkrice").exists());
    }

    #[test]
    fn codex_config_created_from_scratch_is_removed_on_uninstall() {
        let s = sandbox();
        let env = &s.env;
        install(env, Target::Codex).unwrap();
        let file = env.codex_dir.join("config.toml");
        assert!(std::fs::read_to_string(&file).unwrap().starts_with("[mcp_servers.milkrice]"));
        uninstall(env, Target::Codex).unwrap();
        assert!(!file.exists());
    }

    #[test]
    fn shared_skill_stays_until_its_last_user_is_uninstalled() {
        let s = sandbox();
        let env = &s.env;
        let shared = env.home.join(".agents/skills/milkrice/SKILL.md");
        install(env, Target::Cursor).unwrap();
        install(env, Target::Opencode).unwrap();
        uninstall(env, Target::Cursor).unwrap();
        assert!(shared.is_file(), "OpenCode still uses it");
        uninstall(env, Target::Opencode).unwrap();
        assert!(!shared.exists());
    }

    #[test]
    fn will_not_overwrite_someone_elses_skill() {
        let s = sandbox();
        let env = &s.env;
        let dir = env.home.join(".agents/skills/milkrice");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), "---\nname: other\n---\n").unwrap();
        assert!(install(env, Target::Skill).is_err());
        uninstall(env, Target::Skill).unwrap();
        assert!(dir.join("SKILL.md").is_file());
    }

    #[test]
    fn claude_without_cli_writes_claude_json_and_skill() {
        let s = sandbox();
        let env = &s.env;
        std::fs::write(&env.claude_json, "{\"numStartups\": 3, \"mcpServers\": {}}").unwrap();
        install(env, Target::Claude).unwrap();
        let cfg = read(&env.claude_json);
        assert_eq!(cfg["numStartups"], 3);
        assert_eq!(cfg["mcpServers"]["milkrice"]["args"], json!(["mcp"]));
        assert!(env.claude_dir.join("skills/milkrice/SKILL.md").is_file());
        let st = status(env);
        let claude = st.agents.iter().find(|a| a.target == Target::Claude).unwrap();
        assert!(claude.connected, "{claude:?}");
        uninstall(env, Target::Claude).unwrap();
        let cfg = read(&env.claude_json);
        assert_eq!(cfg["numStartups"], 3);
        assert!(cfg["mcpServers"].get("milkrice").is_none());
        assert!(!env.claude_dir.join("skills/milkrice").exists());
    }

    fn add_plugin(env: &Env) {
        let p = env.claude_dir.join("plugins/installed_plugins.json");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, r#"{"version":2,"plugins":{"milkrice@milkrice":[{"scope":"user"}]}}"#).unwrap();
    }

    #[test]
    fn plugin_blocks_direct_claude_install_and_status_warns() {
        let s = sandbox();
        let env = &s.env;
        install(env, Target::Claude).unwrap();
        add_plugin(env);
        let err = install(env, Target::Claude).unwrap_err().to_string();
        assert!(err.contains("claude plugin uninstall milkrice@milkrice"), "{err}");
        let st = status(env);
        assert_eq!(st.claude_plugin.as_deref(), Some("milkrice@milkrice"));
        let claude = st.agents.iter().find(|a| a.target == Target::Claude).unwrap();
        assert!(claude.warnings.iter().any(|w| w.contains("twice")), "{claude:?}");
        // `all` skips Claude when the plugin is there.
        assert!(!expand(env, "all", true).unwrap().contains(&Target::Claude));
    }

    #[test]
    fn disabled_plugin_does_not_count() {
        let s = sandbox();
        let env = &s.env;
        add_plugin(env);
        std::fs::write(env.claude_dir.join("settings.json"), r#"{"enabledPlugins":{"milkrice@milkrice":false}}"#).unwrap();
        assert!(claude_plugin(env).is_none());
    }

    #[test]
    fn vscode_and_opencode_formats() {
        let s = sandbox();
        let env = &s.env;
        install(env, Target::Vscode).unwrap();
        let v = read(&env.copilot_dir.join("mcp-config.json"));
        assert_eq!(v["mcpServers"]["milkrice"]["type"], "stdio");
        assert!(env.copilot_dir.join("skills/milkrice/SKILL.md").is_file());

        // OpenCode, existing newer config with mcp.servers.
        let oc = env.opencode_dir.join("opencode.json");
        std::fs::create_dir_all(&env.opencode_dir).unwrap();
        std::fs::write(&oc, r#"{"$schema":"https://opencode.ai/config.json","mcp":{"servers":{}}}"#).unwrap();
        install(env, Target::Opencode).unwrap();
        let o = read(&oc);
        assert_eq!(o["mcp"]["servers"]["milkrice"]["type"], "local");
        assert_eq!(o["mcp"]["servers"]["milkrice"]["command"][1], "mcp");
        uninstall(env, Target::Opencode).unwrap();
        assert!(read(&oc)["mcp"]["servers"].get("milkrice").is_none());
        assert_eq!(read(&oc)["$schema"], "https://opencode.ai/config.json");
    }

    #[test]
    fn opencode_default_uses_the_older_flat_form() {
        let s = sandbox();
        let env = &s.env;
        install(env, Target::Opencode).unwrap();
        let o = read(&env.opencode_dir.join("opencode.json"));
        assert_eq!(o["mcp"]["milkrice"]["enabled"], true);
    }

    #[test]
    fn dry_run_writes_nothing() {
        let s = sandbox();
        let mut env = s.env.clone();
        env.dry_run = true;
        let log = install(&env, Target::Codex).unwrap();
        assert!(!log.changed.is_empty());
        assert!(!env.codex_dir.exists());
        assert!(std::fs::symlink_metadata(env.home.join(".local/bin/milkrice")).is_err());
    }

    #[test]
    fn uninstall_all_reverts_install_all() {
        let s = sandbox();
        let env = &s.env;
        // Make every agent "detected".
        for d in [env.claude_dir.clone(), env.codex_dir.clone(), env.home.join(".cursor"), env.opencode_dir.clone(), env.copilot_dir.clone()] {
            std::fs::create_dir_all(d).unwrap();
        }
        let targets = expand(env, "all", true).unwrap();
        assert_eq!(targets.len(), 6);
        for t in &targets {
            install(env, *t).unwrap();
        }
        assert!(status(env).agents.iter().all(|a| a.connected), "{:?}", status(env).agents);
        for t in expand(env, "all", false).unwrap() {
            uninstall(env, t).unwrap();
        }
        let st = status(env);
        assert!(st.agents.iter().all(|a| !a.connected && a.mcp.is_none() && a.skills.is_empty()), "{:?}", st.agents);
        assert!(!st.cli_installed);
        for f in [env.home.join(".cursor/mcp.json"), env.codex_dir.join("config.toml"), env.copilot_dir.join("mcp-config.json")] {
            assert!(!f.exists(), "{} left behind", f.display());
        }
    }

    #[test]
    fn path_hint_when_local_bin_is_not_on_path() {
        let s = sandbox();
        let mut env = s.env.clone();
        install(&env, Target::Cli).unwrap();
        env.login_path = Some("/usr/bin:/bin".into());
        assert!(path_hint(&env).unwrap().contains("~/.local/bin"));
        env.login_path = Some(format!("/usr/bin:{}/.local/bin", env.home.display()));
        assert!(path_hint(&env).is_none());
        env.login_path = Some("/usr/bin:$HOME/.local/bin".into());
        assert!(path_hint(&env).is_none());
    }

    #[test]
    fn legacy_cli_targets() {
        assert!(is_legacy_cli_target(Path::new("/Applications/Margin.app/Contents/MacOS/margin-cli")));
        assert!(is_legacy_cli_target(Path::new("/x/target/release/margin-cli")));
        assert!(!is_legacy_cli_target(Path::new("/Applications/Milkrice.app/Contents/MacOS/milkrice-cli")));
        assert!(!is_legacy_cli_target(Path::new("/usr/local/bin/margin")));
    }

    #[test]
    fn removes_only_stale_legacy_symlink() {
        let s = sandbox();
        let d = s.env.home.join("bin");
        std::fs::create_dir_all(&d).unwrap();
        let link = d.join("margin");
        std::os::unix::fs::symlink("/Applications/Margin.app/Contents/MacOS/margin-cli", &link).unwrap();
        remove_legacy_cli_link(&d);
        assert!(std::fs::symlink_metadata(&link).is_err());
        std::os::unix::fs::symlink("/opt/tools/margin", &link).unwrap();
        remove_legacy_cli_link(&d);
        assert!(std::fs::symlink_metadata(&link).is_ok());
    }

    #[test]
    fn removes_only_our_old_skill() {
        let s = sandbox();
        let ours = s.env.home.join("margin");
        std::fs::create_dir_all(&ours).unwrap();
        std::fs::write(ours.join("SKILL.md"), "---\nname: margin\n---\n").unwrap();
        remove_legacy_skill(&ours);
        assert!(!ours.exists());
        let theirs = s.env.home.join("other");
        std::fs::create_dir_all(&theirs).unwrap();
        std::fs::write(theirs.join("SKILL.md"), "---\nname: something-else\n---\n").unwrap();
        remove_legacy_skill(&theirs);
        assert!(theirs.join("SKILL.md").exists());
    }
}
