//! Subscribed calendars (Google "Secret address in iCal format", Outlook ICS links, …).
//! Feeds are fetched every 10 minutes into memory; the secret URL is never logged.
use crate::state::AppState;
use chrono::{DateTime, Utc};
use margin_core::CalEvent;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Feed {
    pub id: String,
    pub name: String,
    pub url: String,
    /// Your address on that calendar, so declined events are skipped and "you" is recognised.
    #[serde(default)]
    pub owner_email: Option<String>,
}

struct Cached {
    fetched: Instant,
    text: Result<String, String>,
}

static CACHE: Mutex<Option<HashMap<String, Cached>>> = Mutex::new(None);
const TTL: Duration = Duration::from_secs(600);

/// Redacted form for logs/UI: host only.
pub fn host(url: &str) -> String {
    url.split("://").nth(1).and_then(|r| r.split('/').next()).unwrap_or("calendar").to_string()
}

fn text_for(feed: &Feed, force: bool) -> Result<String, String> {
    {
        let cache = CACHE.lock();
        if let Some(c) = cache.as_ref().and_then(|m| m.get(&feed.id)) {
            if !force && c.fetched.elapsed() < TTL {
                return c.text.clone();
            }
        }
    }
    let text = margin_core::ics::fetch(&feed.url).map_err(|e| e.to_string());
    if let Err(e) = &text {
        log::warn!("calendar feed '{}' ({}): {e}", feed.name, host(&feed.url));
    }
    CACHE.lock().get_or_insert_with(HashMap::new).insert(feed.id.clone(), Cached { fetched: Instant::now(), text: text.clone() });
    text
}

pub fn events_between(app: &AppHandle, from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<CalEvent> {
    let feeds = app.state::<AppState>().settings().calendar_feeds;
    let mut out = Vec::new();
    for f in &feeds {
        if let Ok(t) = text_for(f, false) {
            out.extend(margin_core::ics::parse(&t, &f.name, f.owner_email.as_deref(), from, to));
        }
    }
    out
}

/// Validate a feed by fetching and parsing it. Returns the number of upcoming events.
pub fn test_feed(feed: &Feed) -> anyhow::Result<usize> {
    let text = text_for(feed, true).map_err(anyhow::Error::msg)?;
    let now = Utc::now();
    Ok(margin_core::ics::parse(&text, &feed.name, feed.owner_email.as_deref(), now, now + chrono::Duration::days(30)).len())
}

pub fn forget(id: &str) {
    if let Some(m) = CACHE.lock().as_mut() {
        m.remove(id);
    }
}
