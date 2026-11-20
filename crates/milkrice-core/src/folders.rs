//! Folder suggestions, computed on this Mac with no AI.
//!
//! A new note is compared with the notes already in each folder. The signals are the people
//! in the meeting, the words of the title, and (once there are notes) the words of the notes.
//! A calendar meeting with the same title as a filed one (a recurring series) is a near-certain
//! match.

use crate::types::Attendee;
use std::collections::{HashMap, HashSet};

/// What the suggester knows about a note.
#[derive(Debug, Clone, Default)]
pub struct NoteInfo {
    pub title: String,
    pub attendees: Vec<Attendee>,
    /// The note's text (AI notes, else the user's notes). May be empty.
    pub text: String,
    /// True when the note came from a calendar event.
    pub from_calendar: bool,
}

/// Suggest only when the best folder scores at least this (0..1).
pub const SUGGEST_MIN: f32 = 0.45;

const STOP: &[&str] = &[
    "the", "and", "for", "with", "from", "this", "that", "meeting", "call", "sync", "chat", "catch",
    "untitled", "new", "note", "notes", "re", "fwd", "via", "about", "our", "your", "are", "was",
    "will", "have", "has", "not", "but", "you", "she", "his", "her", "they", "them", "what", "when",
    "who", "how", "can", "all", "any", "some", "into", "out", "over", "just", "also", "then", "than",
    "there", "their", "were", "been", "being", "would", "should", "could", "next", "week", "today",
];

fn tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .map(|w| w.to_lowercase())
        .filter(|w| w.len() >= 3 && !w.chars().all(|c| c.is_ascii_digit()) && !STOP.contains(&w.as_str()))
        .collect()
}

/// Key for a recurring series: the title without numbers, dates or punctuation.
/// "Weekly sync #12 (3 Nov)" and "Weekly sync #13 (10 Nov)" share a key.
pub fn series_key(title: &str) -> String {
    const MONTHS: &[&str] = &["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "sept", "oct", "nov", "dec"];
    title
        .split(|c: char| !c.is_alphabetic())
        .map(|w| w.to_lowercase())
        .filter(|w| !w.is_empty() && !MONTHS.contains(&w.as_str()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn generic_title(key: &str) -> bool {
    key.is_empty() || matches!(key, "untitled" | "untitled meeting" | "new note" | "meeting" | "call")
}

fn people(a: &[Attendee]) -> HashSet<String> {
    a.iter()
        .filter(|x| !x.is_self)
        .map(|x| x.email.clone().filter(|e| !e.is_empty()).unwrap_or_else(|| x.name.clone()).trim().to_lowercase())
        .filter(|k| !k.is_empty())
        .collect()
}

fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f32 {
    let inter = a.intersection(b).count() as f32;
    let union = a.union(b).count() as f32;
    if union == 0.0 { 0.0 } else { inter / union }
}

fn counts(t: &[String]) -> HashMap<&str, f32> {
    let mut m = HashMap::new();
    for w in t {
        *m.entry(w.as_str()).or_insert(0.0) += 1.0;
    }
    m
}

fn cosine(a: &HashMap<&str, f32>, b: &HashMap<&str, f32>) -> f32 {
    let dot: f32 = a.iter().filter_map(|(k, v)| b.get(k).map(|w| v * w)).sum();
    let na: f32 = a.values().map(|v| v * v).sum::<f32>().sqrt();
    let nb: f32 = b.values().map(|v| v * v).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 { 0.0 } else { dot / (na * nb) }
}

/// How alike two notes are, 0..1, from whatever signals both have.
pub fn similarity(a: &NoteInfo, b: &NoteInfo) -> f32 {
    let (ka, kb) = (series_key(&a.title), series_key(&b.title));
    if a.from_calendar && b.from_calendar && !generic_title(&ka) && ka == kb {
        return 1.0;
    }
    let mut num = 0.0;
    let mut den = 0.0;
    let (pa, pb) = (people(&a.attendees), people(&b.attendees));
    if !pa.is_empty() && !pb.is_empty() {
        num += 0.5 * jaccard(&pa, &pb);
        den += 0.5;
    }
    let (ta, tb): (HashSet<String>, HashSet<String>) = (tokens(&a.title).into_iter().collect(), tokens(&b.title).into_iter().collect());
    if !ta.is_empty() && !tb.is_empty() {
        num += 0.3 * jaccard(&ta, &tb);
        den += 0.3;
    }
    let (xa, xb) = (tokens(&a.text), tokens(&b.text));
    if xa.len() >= 25 && xb.len() >= 25 {
        num += 0.3 * cosine(&counts(&xa), &counts(&xb));
        den += 0.3;
    }
    if den == 0.0 { 0.0 } else { num / den }
}

/// Best folder for `note` given the notes already filed (`(folder_id, note)` pairs).
/// A folder's score is the average of its two most similar notes, so one lucky match in a big
/// folder doesn't beat a folder that clearly fits. Returns `(folder_id, score)` if it clears
/// [`SUGGEST_MIN`].
pub fn suggest(note: &NoteInfo, filed: &[(String, NoteInfo)]) -> Option<(String, f32)> {
    let mut per: HashMap<&str, Vec<f32>> = HashMap::new();
    for (fid, other) in filed {
        per.entry(fid.as_str()).or_default().push(similarity(note, other));
    }
    let mut best: Option<(String, f32)> = None;
    for (fid, mut s) in per {
        s.sort_by(|a, b| b.partial_cmp(a).unwrap());
        let score = if s[0] >= 1.0 || s.len() == 1 { s[0] } else { (s[0] + s[1]) / 2.0 };
        if best.as_ref().map(|b| score > b.1 || (score == b.1 && fid < b.0.as_str())).unwrap_or(true) {
            best = Some((fid.to_string(), score));
        }
    }
    best.filter(|b| b.1 >= SUGGEST_MIN)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn att(names: &[&str]) -> Vec<Attendee> {
        names.iter().map(|n| Attendee { name: n.to_string(), email: None, is_self: false }).collect()
    }
    fn note(title: &str, who: &[&str], cal: bool) -> NoteInfo {
        NoteInfo { title: title.into(), attendees: att(who), text: String::new(), from_calendar: cal }
    }

    #[test]
    fn series_key_ignores_numbers_and_dates() {
        assert_eq!(series_key("Weekly sync #12 (3 Nov)"), series_key("Weekly sync #13 (10 Nov)"));
        assert_ne!(series_key("Weekly sync"), series_key("Design review"));
    }

    #[test]
    fn recurring_meeting_goes_to_its_folder() {
        let filed = vec![
            ("product".to_string(), note("Product weekly", &["Priya"], true)),
            ("hiring".to_string(), note("Interview: backend", &["Sam"], true)),
        ];
        let s = suggest(&note("Product weekly", &[], true), &filed).unwrap();
        assert_eq!(s.0, "product");
    }

    #[test]
    fn same_people_suggest_a_folder() {
        let filed = vec![
            ("acme".to_string(), note("Acme kickoff", &["Jo Lee", "Max Ng"], true)),
            ("acme".to_string(), note("Acme pricing", &["Jo Lee", "Max Ng"], true)),
            ("team".to_string(), note("Standup", &["Ana", "Ben", "Cy"], true)),
        ];
        let s = suggest(&note("Contract review", &["Jo Lee", "Max Ng"], true), &filed).unwrap();
        assert_eq!(s.0, "acme");
    }

    #[test]
    fn no_signal_no_suggestion() {
        let filed = vec![("acme".to_string(), note("Acme kickoff", &["Jo Lee"], true))];
        assert!(suggest(&note("", &[], false), &filed).is_none());
        assert!(suggest(&note("Dentist", &["Dr Who"], true), &filed).is_none());
        assert!(suggest(&note("Untitled", &[], true), &[("x".into(), note("Untitled", &[], true))]).is_none());
    }

    #[test]
    fn note_text_can_carry_a_suggestion() {
        let words = "pricing tiers launch december payments provider approval dashboard funnel signup ";
        let mut a = note("Quick chat", &[], false);
        a.text = words.repeat(4);
        let mut b = note("Pricing launch plan", &[], false);
        b.text = words.repeat(3);
        let mut c = note("Offsite logistics", &[], false);
        c.text = "venue catering travel hotel agenda budget flights rooms dinner bus ".repeat(4);
        let s = suggest(&a, &[("launch".into(), b), ("offsite".into(), c)]).unwrap();
        assert_eq!(s.0, "launch");
    }
}
