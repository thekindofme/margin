//! Export meetings into an Obsidian vault (or any folder) as plain Markdown + YAML front matter.
//!
//! One file per meeting. Margin only rewrites the block between its markers, so anything the
//! user adds above or below in Obsidian survives every re-export.
use crate::llm::speaker_name;
use crate::store::Meeting;
use crate::types::Segment;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fmt::Write;
use std::path::{Path, PathBuf};

pub const START: &str = "<!-- margin:start — Margin updates this block; write your own notes outside it -->";
pub const END: &str = "<!-- margin:end -->";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportOptions {
    /// Vault (or any) root folder.
    pub root: String,
    /// Subfolder inside the root for meeting notes.
    pub folder: String,
    /// Attendees and named speakers as [[wikilinks]].
    pub link_people: bool,
    /// Link the meeting date to a daily note: [[2026-11-16]].
    pub link_daily_note: bool,
    pub include_transcript: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self { root: String::new(), folder: "Meetings".into(), link_people: true, link_daily_note: true, include_transcript: true }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Vault {
    pub name: String,
    pub path: String,
}

/// Vaults Obsidian knows about (from its own config), most recently used first.
pub fn detect_vaults() -> Vec<Vault> {
    let cfg = dirs::home_dir().unwrap_or_default().join("Library/Application Support/obsidian/obsidian.json");
    let Ok(text) = std::fs::read_to_string(cfg) else { return vec![] };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return vec![] };
    let mut out: Vec<(i64, Vault)> = v["vaults"]
        .as_object()
        .map(|m| {
            m.values()
                .filter_map(|x| {
                    let path = x["path"].as_str()?.to_string();
                    if !Path::new(&path).is_dir() {
                        return None;
                    }
                    let name = Path::new(&path).file_name()?.to_string_lossy().to_string();
                    Some((x["ts"].as_i64().unwrap_or(0), Vault { name, path }))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out.into_iter().map(|x| x.1).collect()
}

fn sanitize(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '#' | '^' | '[' | ']') { ' ' } else { c })
        .collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let t: String = t.chars().take(90).collect();
    if t.is_empty() { "Untitled meeting".into() } else { t }
}

fn yaml_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Where a meeting's note lives: <root>/<folder>/<YYYY-MM-DD> <title>.md
pub fn note_path(opts: &ExportOptions, m: &Meeting) -> PathBuf {
    let local = m.created_at.with_timezone(&chrono::Local);
    let title = if m.title.trim().is_empty() { "Untitled meeting".to_string() } else { m.title.clone() };
    Path::new(&opts.root).join(&opts.folder).join(format!("{} {}.md", local.format("%Y-%m-%d"), sanitize(&title)))
}

fn link(name: &str, on: bool) -> String {
    if on { format!("[[{}]]", sanitize(name)) } else { name.to_string() }
}

/// The Margin-owned block (front matter is handled separately).
pub fn render_block(m: &Meeting, segs: &[Segment], opts: &ExportOptions) -> String {
    let mut o = String::new();
    let local = m.created_at.with_timezone(&chrono::Local);
    let people: Vec<String> = m.attendees.iter().filter(|a| !a.is_self).map(|a| link(&a.name, opts.link_people)).collect();
    let _ = writeln!(o, "{START}");
    let _ = writeln!(o, "# {}\n", if m.title.trim().is_empty() { "Untitled meeting" } else { m.title.trim() });
    let date = if opts.link_daily_note { format!("[[{}]]", local.format("%Y-%m-%d")) } else { local.format("%Y-%m-%d").to_string() };
    let mut meta = format!("{date} · {}", local.format("%H:%M"));
    if m.duration_ms > 0 {
        let _ = write!(meta, " · {} min", (m.duration_ms + 30_000) / 60_000);
    }
    if !people.is_empty() {
        let _ = write!(meta, " · with {}", people.join(", "));
    }
    let _ = writeln!(o, "{meta}\n");
    if !m.enhanced.trim().is_empty() {
        let by = if m.enhanced_by.is_empty() { "an AI model".to_string() } else { m.enhanced_by.clone() };
        let label = if m.enhanced_edited { "AI summary (edited by you)" } else { "AI summary" };
        let _ = writeln!(o, "## Summary\n> [!info] {label}\n> Written by {by} from the transcript and your notes. It may contain mistakes.\n");
        let _ = writeln!(o, "{}\n", m.enhanced.trim());
    }
    let _ = writeln!(o, "## My notes\n");
    let _ = writeln!(o, "{}\n", if m.notes.trim().is_empty() { "_No notes taken._" } else { m.notes.trim() });
    if opts.include_transcript && !segs.is_empty() {
        let model = if m.stt_model.is_empty() { "a local speech model".to_string() } else { m.stt_model.clone() };
        let _ = writeln!(o, "## Transcript\n> [!quote]- Transcript ({} lines · speech-to-text by {model} on this Mac)", segs.len());
        let mut last: Option<&str> = None;
        for s in segs.iter().filter(|s| !s.text.trim().is_empty()) {
            let name = speaker_name(&s.speaker, &m.speakers);
            let sec = s.start_ms / 1000;
            if last == Some(s.speaker.as_str()) {
                let _ = writeln!(o, "> {}", s.text.trim());
            } else {
                let who = if s.speaker == "me" || name.starts_with("Speaker ") { name.clone() } else { link(&name, opts.link_people) };
                let _ = writeln!(o, ">\n> **{:02}:{:02} {who}:** {}", sec / 60, sec % 60, s.text.trim());
                last = Some(&s.speaker);
            }
        }
        o.push('\n');
    }
    let _ = writeln!(o, "{END}");
    o
}

pub fn render_front_matter(m: &Meeting, opts: &ExportOptions) -> String {
    let local = m.created_at.with_timezone(&chrono::Local);
    let mut f = String::from("---\n");
    let _ = writeln!(f, "title: {}", yaml_str(if m.title.trim().is_empty() { "Untitled meeting" } else { m.title.trim() }));
    let _ = writeln!(f, "date: {}", local.format("%Y-%m-%d"));
    let _ = writeln!(f, "time: \"{}\"", local.format("%H:%M"));
    if m.duration_ms > 0 {
        let _ = writeln!(f, "duration_min: {}", (m.duration_ms + 30_000) / 60_000);
    }
    let people: Vec<String> = m.attendees.iter().filter(|a| !a.is_self).map(|a| yaml_str(&link(&a.name, opts.link_people))).collect();
    if !people.is_empty() {
        let _ = writeln!(f, "attendees: [{}]", people.join(", "));
    }
    let _ = writeln!(f, "tags: [meeting, margin]");
    if !m.enhanced_by.is_empty() {
        let _ = writeln!(f, "summary_by: {}", yaml_str(&m.enhanced_by));
    }
    if !m.stt_model.is_empty() {
        let _ = writeln!(f, "transcript_model: {}", yaml_str(&m.stt_model));
    }
    if let Some(u) = &m.meeting_url {
        let _ = writeln!(f, "call_link: {}", yaml_str(u));
    }
    let _ = writeln!(f, "margin_id: {}", m.id);
    f.push_str("---\n");
    f
}

/// Replace Margin's block (and front matter) in an existing note, keeping the user's additions.
pub fn merge(existing: &str, front: &str, block: &str) -> String {
    // Keep any user front-matter keys we don't manage? Simpler and predictable: Margin owns the
    // front matter; the user's notes live outside the block.
    let body = existing
        .strip_prefix("---\n")
        .and_then(|rest| rest.find("\n---\n").map(|i| &rest[i + 5..]))
        .unwrap_or(existing);
    match (body.find(START), body.find(END)) {
        (Some(a), Some(b)) if b > a => {
            let after = &body[b + END.len()..];
            format!("{front}{}{}{}", &body[..a], block.trim_end(), after)
        }
        _ => {
            let extra = body.trim();
            if extra.is_empty() { format!("{front}{block}") } else { format!("{front}{block}\n{extra}\n") }
        }
    }
}

/// Write (or update) a meeting's note. `previous` is the path written last time, so a renamed
/// meeting moves its file instead of leaving a duplicate. Returns the path written.
pub fn export(m: &Meeting, segs: &[Segment], opts: &ExportOptions, previous: Option<&Path>) -> Result<PathBuf> {
    anyhow::ensure!(!opts.root.trim().is_empty(), "no Obsidian vault or folder chosen");
    anyhow::ensure!(Path::new(&opts.root).is_dir(), "folder not found: {}", opts.root);
    let path = note_path(opts, m);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).context("creating meetings folder")?;
    }
    let mut existing = std::fs::read_to_string(&path).ok();
    if let Some(prev) = previous.filter(|p| *p != path && p.is_file()) {
        if existing.is_none() {
            existing = std::fs::read_to_string(prev).ok();
            std::fs::rename(prev, &path).ok();
        }
    }
    let content = merge(existing.as_deref().unwrap_or(""), &render_front_matter(m, opts), &render_block(m, segs, opts));
    // Atomic replace so Obsidian never sees a half-written file.
    let tmp = path.with_extension("md.margin-tmp");
    std::fs::write(&tmp, &content)?;
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AudioSource, Attendee};

    fn meeting() -> Meeting {
        Meeting {
            id: "abc".into(),
            title: "Beta launch: sync".into(),
            created_at: chrono::Utc::now(),
            duration_ms: 31 * 60_000,
            attendees: vec![Attendee { name: "Priya Shah".into(), email: None, is_self: false }],
            notes: "beta date?".into(),
            enhanced: "### Decisions\n- Ship Friday".into(),
            enhanced_by: "claude:haiku".into(),
            stt_model: "parakeet-v2".into(),
            ..Default::default()
        }
    }

    #[test]
    fn writes_and_preserves_user_text() {
        let dir = std::env::temp_dir().join(format!("margin-obs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let opts = ExportOptions { root: dir.to_string_lossy().into(), ..Default::default() };
        let mut m = meeting();
        let segs = vec![Segment { id: 1, start_ms: 0, end_ms: 1000, source: AudioSource::System, speaker: "s1".into(), text: "Hello".into() }];
        m.speakers.insert("s1".into(), "Priya Shah".into());
        let p = export(&m, &segs, &opts, None).unwrap();
        assert!(p.ends_with("Meetings/".to_string() + &format!("{} Beta launch sync.md", m.created_at.with_timezone(&chrono::Local).format("%Y-%m-%d"))));
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.starts_with("---\ntitle: \"Beta launch: sync\""));
        assert!(text.contains("[[Priya Shah]]"));
        assert!(text.contains("> [!quote]- Transcript"));
        // User adds their own notes outside the block; re-export keeps them.
        std::fs::write(&p, format!("{text}\n## Follow-ups\n- call Priya\n")).unwrap();
        m.enhanced = "### Decisions\n- Ship Monday".into();
        let p2 = export(&m, &segs, &opts, Some(&p)).unwrap();
        let t2 = std::fs::read_to_string(&p2).unwrap();
        assert!(t2.contains("Ship Monday") && !t2.contains("Ship Friday"));
        assert!(t2.contains("## Follow-ups\n- call Priya"));
        // Renaming the meeting moves the file and keeps the user's text.
        m.title = "Beta launch retro".into();
        let p3 = export(&m, &segs, &opts, Some(&p2)).unwrap();
        assert!(!p2.exists() && p3.exists());
        assert!(std::fs::read_to_string(&p3).unwrap().contains("call Priya"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
