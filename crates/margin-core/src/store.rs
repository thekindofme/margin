//! Local SQLite storage for meetings, transcripts, notes and settings.
use crate::types::{AudioSource, Attendee, Segment};
use anyhow::Result;
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Meeting {
    pub id: String,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    /// Total recorded audio in ms (across pauses/resumes).
    pub duration_ms: u64,
    pub event_id: Option<String>,
    pub attendees: Vec<Attendee>,
    /// The user's own notes (markdown-ish plain text).
    pub notes: String,
    /// AI-enhanced notes (markdown).
    pub enhanced: String,
    pub template: String,
    pub status: String,
    /// speaker key ("me", "s1", ...) -> display name
    pub speakers: BTreeMap<String, String>,
    /// Saved diarization state so a meeting can be resumed.
    pub clusterer: Option<serde_json::Value>,
    pub meeting_url: Option<String>,
    /// Who wrote `enhanced`, e.g. "claude:haiku" or "codex:gpt-6-luna".
    #[serde(default)]
    pub enhanced_by: String,
    /// Speech model that produced the transcript, e.g. "parakeet-v2".
    #[serde(default)]
    pub stt_model: String,
    /// Last file written by the Obsidian/folder export (to follow renames).
    #[serde(default)]
    pub export_path: String,
    /// The user edited the AI notes after they were written (cleared when regenerated).
    #[serde(default)]
    pub enhanced_edited: bool,
    /// Speaker keys whose names were guessed by the AI and not yet confirmed by the user.
    #[serde(default)]
    pub speakers_guessed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingSummary {
    pub id: String,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub duration_ms: u64,
    pub status: String,
    pub attendees: Vec<Attendee>,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

pub struct Store {
    conn: Mutex<Connection>,
    /// Optional columns present in this database (a read-only handle can't migrate an older one).
    cols: [bool; 5],
}

/// Optional columns added after the first release, in `cols` order.
const OPT_COLS: [&str; 5] = ["enhanced_by", "stt_model", "export_path", "enhanced_edited", "speakers_guessed"];

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS meetings (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  started_at TEXT,
  ended_at TEXT,
  duration_ms INTEGER NOT NULL DEFAULT 0,
  event_id TEXT,
  attendees TEXT NOT NULL DEFAULT '[]',
  notes TEXT NOT NULL DEFAULT '',
  enhanced TEXT NOT NULL DEFAULT '',
  template TEXT NOT NULL DEFAULT 'auto',
  status TEXT NOT NULL DEFAULT 'draft',
  speakers TEXT NOT NULL DEFAULT '{}',
  clusterer TEXT,
  meeting_url TEXT
);
CREATE INDEX IF NOT EXISTS meetings_created ON meetings(created_at DESC);
CREATE INDEX IF NOT EXISTS meetings_event ON meetings(event_id);
CREATE TABLE IF NOT EXISTS segments (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  start_ms INTEGER NOT NULL,
  end_ms INTEGER NOT NULL,
  source TEXT NOT NULL,
  speaker TEXT NOT NULL,
  text TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS segments_meeting ON segments(meeting_id, start_ms);
CREATE TABLE IF NOT EXISTS chats (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  role TEXT NOT NULL,
  content TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS event_flags (event_id TEXT PRIMARY KEY, flag TEXT NOT NULL);
"#;

fn src_str(s: AudioSource) -> &'static str {
    match s {
        AudioSource::Mic => "mic",
        AudioSource::System => "system",
    }
}

fn parse_dt(s: Option<String>) -> Option<DateTime<Utc>> {
    s.and_then(|s| DateTime::parse_from_rfc3339(&s).ok()).map(|d| d.with_timezone(&Utc))
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)?;
        // Migrations: provenance columns.
        for (col, ddl) in [
            ("enhanced_by", "ALTER TABLE meetings ADD COLUMN enhanced_by TEXT NOT NULL DEFAULT ''"),
            ("stt_model", "ALTER TABLE meetings ADD COLUMN stt_model TEXT NOT NULL DEFAULT ''"),
            ("export_path", "ALTER TABLE meetings ADD COLUMN export_path TEXT NOT NULL DEFAULT ''"),
            ("enhanced_edited", "ALTER TABLE meetings ADD COLUMN enhanced_edited INTEGER NOT NULL DEFAULT 0"),
            ("speakers_guessed", "ALTER TABLE meetings ADD COLUMN speakers_guessed TEXT NOT NULL DEFAULT '[]'"),
        ] {
            let has: bool = conn
                .prepare("SELECT 1 FROM pragma_table_info('meetings') WHERE name=?1")?
                .exists([col])?;
            if !has {
                conn.execute_batch(ddl)?;
            }
        }
        // Older builds stored speaker voice prints; never keep biometric data on disk.
        conn.execute("UPDATE meetings SET clusterer=NULL WHERE clusterer IS NOT NULL", [])?;
        // A crash or force-quit can leave a meeting marked as recording; nothing is recording now.
        conn.execute("UPDATE meetings SET status='done', ended_at=COALESCE(ended_at, started_at) WHERE status='recording'", [])?;
        Ok(Self { conn: Mutex::new(conn), cols: [true; 5] })
    }

    fn has_col(conn: &Connection, col: &str) -> bool {
        conn.prepare("SELECT 1 FROM pragma_table_info('meetings') WHERE name=?1")
            .and_then(|mut st| st.exists([col]))
            .unwrap_or(false)
    }

    /// Read-only handle for the CLI / MCP server: agents can never modify meetings.
    pub fn open_readonly(path: &Path) -> Result<Self> {
        let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let cols = OPT_COLS.map(|c| Self::has_col(&conn, c));
        Ok(Self { conn: Mutex::new(conn), cols })
    }

    pub fn open_default() -> Result<Self> {
        Self::open(&crate::paths::db_path())
    }

    pub fn create_meeting(&self, m: &Meeting) -> Result<()> {
        let c = self.conn.lock();
        c.execute(
            "INSERT INTO meetings (id,title,created_at,started_at,ended_at,duration_ms,event_id,attendees,notes,enhanced,template,status,speakers,clusterer,meeting_url)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            params![
                m.id,
                m.title,
                m.created_at.to_rfc3339(),
                m.started_at.map(|d| d.to_rfc3339()),
                m.ended_at.map(|d| d.to_rfc3339()),
                m.duration_ms as i64,
                m.event_id,
                serde_json::to_string(&m.attendees)?,
                m.notes,
                m.enhanced,
                m.template,
                m.status,
                serde_json::to_string(&m.speakers)?,
                m.clusterer.as_ref().map(|v| v.to_string()),
                m.meeting_url,
            ],
        )?;
        Ok(())
    }

    pub fn get_meeting(&self, id: &str) -> Result<Option<Meeting>> {
        let c = self.conn.lock();
        let fallback = |col: &str| match col {
            "enhanced_edited" => "0",
            "speakers_guessed" => "'[]'",
            _ => "''",
        };
        let opt: Vec<String> = OPT_COLS
            .iter()
            .zip(self.cols)
            .map(|(col, present)| if present { col.to_string() } else { format!("{} AS {col}", fallback(col)) })
            .collect();
        let sql = format!(
            "SELECT id,title,created_at,started_at,ended_at,duration_ms,event_id,attendees,notes,enhanced,template,status,speakers,clusterer,meeting_url,{} FROM meetings WHERE id=?1",
            opt.join(",")
        );
        let m = c
            .query_row(
                &sql,
                [id],
                |r| {
                    Ok(Meeting {
                        id: r.get(0)?,
                        title: r.get(1)?,
                        created_at: parse_dt(r.get(2)?).unwrap_or_else(Utc::now),
                        started_at: parse_dt(r.get(3)?),
                        ended_at: parse_dt(r.get(4)?),
                        duration_ms: r.get::<_, i64>(5)? as u64,
                        event_id: r.get(6)?,
                        attendees: serde_json::from_str(&r.get::<_, String>(7)?).unwrap_or_default(),
                        notes: r.get(8)?,
                        enhanced: r.get(9)?,
                        template: r.get(10)?,
                        status: r.get(11)?,
                        speakers: serde_json::from_str(&r.get::<_, String>(12)?).unwrap_or_default(),
                        clusterer: r
                            .get::<_, Option<String>>(13)?
                            .and_then(|s| serde_json::from_str(&s).ok()),
                        meeting_url: r.get(14)?,
                        enhanced_by: r.get(15)?,
                        stt_model: r.get(16)?,
                        export_path: r.get(17)?,
                        enhanced_edited: r.get::<_, i64>(18)? != 0,
                        speakers_guessed: serde_json::from_str(&r.get::<_, String>(19)?).unwrap_or_default(),
                    })
                },
            )
            .optional()?;
        Ok(m)
    }

    pub fn find_by_event(&self, event_id: &str) -> Result<Option<String>> {
        let c = self.conn.lock();
        Ok(c.query_row(
            "SELECT id FROM meetings WHERE event_id=?1 ORDER BY created_at DESC LIMIT 1",
            [event_id],
            |r| r.get(0),
        )
        .optional()?)
    }

    /// Update a single editable text/json field.
    pub fn set_field(&self, id: &str, field: &str, value: &str) -> Result<()> {
        const ALLOWED: &[&str] =
            &["title", "notes", "enhanced", "template", "status", "speakers", "attendees", "clusterer", "meeting_url", "enhanced_by", "stt_model", "export_path", "enhanced_edited", "speakers_guessed"];
        anyhow::ensure!(ALLOWED.contains(&field), "field {field} is not editable");
        let c = self.conn.lock();
        c.execute(&format!("UPDATE meetings SET {field}=?1 WHERE id=?2"), params![value, id])?;
        Ok(())
    }

    pub fn mark_started(&self, id: &str) -> Result<()> {
        let c = self.conn.lock();
        c.execute(
            "UPDATE meetings SET started_at=COALESCE(started_at, ?1), status='recording' WHERE id=?2",
            params![Utc::now().to_rfc3339(), id],
        )?;
        Ok(())
    }

    pub fn mark_stopped(&self, id: &str, duration_ms: u64) -> Result<()> {
        let c = self.conn.lock();
        c.execute(
            "UPDATE meetings SET ended_at=?1, status='done', duration_ms=?2 WHERE id=?3",
            params![Utc::now().to_rfc3339(), duration_ms as i64, id],
        )?;
        Ok(())
    }

    pub fn list_meetings(&self, query: Option<&str>, limit: usize) -> Result<Vec<MeetingSummary>> {
        let c = self.conn.lock();
        let q = query.map(|s| s.trim()).filter(|s| !s.is_empty());
        let sql = if q.is_some() {
            "SELECT m.id,m.title,m.created_at,m.duration_ms,m.status,m.attendees,
               COALESCE(NULLIF(substr(m.enhanced,1,240),''), substr(m.notes,1,240)) FROM meetings m
             WHERE m.title LIKE ?1 OR m.notes LIKE ?1 OR m.enhanced LIKE ?1 OR m.attendees LIKE ?1
               OR EXISTS (SELECT 1 FROM segments s WHERE s.meeting_id=m.id AND s.text LIKE ?1)
             ORDER BY m.created_at DESC LIMIT ?2"
        } else {
            "SELECT m.id,m.title,m.created_at,m.duration_ms,m.status,m.attendees,
               COALESCE(NULLIF(substr(m.enhanced,1,240),''), substr(m.notes,1,240)) FROM meetings m
             WHERE ?1 IS NULL OR ?1 IS NOT NULL ORDER BY m.created_at DESC LIMIT ?2"
        };
        let like = q.map(|s| format!("%{}%", s.replace('%', "\\%")));
        let mut st = c.prepare(sql)?;
        let rows = st.query_map(params![like, limit as i64], |r| {
            Ok(MeetingSummary {
                id: r.get(0)?,
                title: r.get(1)?,
                created_at: parse_dt(r.get(2)?).unwrap_or_else(Utc::now),
                duration_ms: r.get::<_, i64>(3)? as u64,
                status: r.get(4)?,
                attendees: serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or_default(),
                snippet: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Resolve a full or prefix id (agents use short 8-char ids).
    pub fn resolve_id(&self, prefix: &str) -> Result<Option<String>> {
        let c = self.conn.lock();
        let mut st = c.prepare("SELECT id FROM meetings WHERE id LIKE ?1 ORDER BY created_at DESC LIMIT 2")?;
        let ids: Vec<String> = st.query_map([format!("{prefix}%")], |r| r.get(0))?.filter_map(|r| r.ok()).collect();
        anyhow::ensure!(ids.len() <= 1, "id prefix '{prefix}' is ambiguous");
        Ok(ids.into_iter().next())
    }

    /// Meetings created at or after `since`, newest first.
    pub fn meetings_since(&self, since: DateTime<Utc>, limit: usize) -> Result<Vec<MeetingSummary>> {
        Ok(self.list_meetings(None, 10_000)?.into_iter().filter(|m| m.created_at >= since).take(limit).collect())
    }

    pub fn delete_meeting(&self, id: &str) -> Result<()> {
        let c = self.conn.lock();
        c.execute("DELETE FROM meetings WHERE id=?1", [id])?;
        Ok(())
    }

    pub fn add_segment(&self, meeting_id: &str, s: &Segment) -> Result<i64> {
        let c = self.conn.lock();
        c.execute(
            "INSERT INTO segments (meeting_id,start_ms,end_ms,source,speaker,text) VALUES (?1,?2,?3,?4,?5,?6)",
            params![meeting_id, s.start_ms as i64, s.end_ms as i64, src_str(s.source), s.speaker, s.text],
        )?;
        Ok(c.last_insert_rowid())
    }

    pub fn segments(&self, meeting_id: &str) -> Result<Vec<Segment>> {
        let c = self.conn.lock();
        let mut st = c.prepare(
            "SELECT id,start_ms,end_ms,source,speaker,text FROM segments WHERE meeting_id=?1 ORDER BY start_ms, id",
        )?;
        let rows = st.query_map([meeting_id], |r| {
            Ok(Segment {
                id: r.get(0)?,
                start_ms: r.get::<_, i64>(1)? as u64,
                end_ms: r.get::<_, i64>(2)? as u64,
                source: if r.get::<_, String>(3)? == "mic" { AudioSource::Mic } else { AudioSource::System },
                speaker: r.get(4)?,
                text: r.get(5)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn update_segment_text(&self, id: i64, text: &str) -> Result<()> {
        self.conn.lock().execute("UPDATE segments SET text=?1 WHERE id=?2", params![text, id])?;
        Ok(())
    }

    /// Move every segment of one speaker key to another (merging speakers).
    pub fn reassign_speaker(&self, meeting_id: &str, from: &str, to: &str) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE segments SET speaker=?1 WHERE meeting_id=?2 AND speaker=?3",
            params![to, meeting_id, from],
        )?;
        Ok(())
    }

    pub fn add_chat(&self, meeting_id: &str, role: &str, content: &str) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO chats (meeting_id,role,content,created_at) VALUES (?1,?2,?3,?4)",
            params![meeting_id, role, content, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn chats(&self, meeting_id: &str) -> Result<Vec<ChatMessage>> {
        let c = self.conn.lock();
        let mut st = c.prepare("SELECT role,content,created_at FROM chats WHERE meeting_id=?1 ORDER BY id")?;
        let rows = st.query_map([meeting_id], |r| {
            Ok(ChatMessage {
                role: r.get(0)?,
                content: r.get(1)?,
                created_at: parse_dt(r.get(2)?).unwrap_or_else(Utc::now),
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_setting(&self, key: &str) -> Option<String> {
        self.conn
            .lock()
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO settings (key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Per-calendar-event flags such as "notified" or "skipped".
    pub fn event_flag(&self, event_id: &str) -> Option<String> {
        self.conn
            .lock()
            .query_row("SELECT flag FROM event_flags WHERE event_id=?1", [event_id], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn set_event_flag(&self, event_id: &str, flag: &str) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO event_flags (event_id,flag) VALUES (?1,?2) ON CONFLICT(event_id) DO UPDATE SET flag=excluded.flag",
            params![event_id, flag],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let dir = std::env::temp_dir().join(format!("margin-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let s = Store::open(&dir.join("t.sqlite")).unwrap();
        let m = Meeting {
            id: "m1".into(),
            title: "Weekly sync".into(),
            created_at: Utc::now(),
            template: "auto".into(),
            status: "draft".into(),
            ..Default::default()
        };
        s.create_meeting(&m).unwrap();
        s.add_segment(
            "m1",
            &Segment { id: 0, start_ms: 0, end_ms: 1000, source: AudioSource::Mic, speaker: "me".into(), text: "budget is approved".into() },
        )
        .unwrap();
        s.set_field("m1", "notes", "hello").unwrap();
        assert_eq!(s.get_meeting("m1").unwrap().unwrap().notes, "hello");
        assert_eq!(s.list_meetings(Some("budget"), 10).unwrap().len(), 1);
        assert_eq!(s.list_meetings(None, 10).unwrap().len(), 1);
        assert!(s.set_field("m1", "id; DROP", "x").is_err());
        s.set_field("m1", "enhanced_edited", "1").unwrap();
        s.set_field("m1", "speakers_guessed", r#"["s1"]"#).unwrap();
        let got = s.get_meeting("m1").unwrap().unwrap();
        assert!(got.enhanced_edited);
        assert_eq!(got.speakers_guessed, vec!["s1".to_string()]);
        s.delete_meeting("m1").unwrap();
        assert!(s.segments("m1").unwrap().is_empty());
    }
}
