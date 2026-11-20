//! Who was probably in a meeting, from the invite and the title, for naming speakers.
//!
//! Calendar feeds often give only an email ("maya@acme.io" shown as "maya"), while the title
//! has the full name ("Sam Carter and Maya Lindqvist"). Titles are also the only source
//! when an event has no attendee list ("Recruiter screen wt Omar Haddad").

use crate::types::Attendee;

/// Words that introduce a person in a meeting title.
const MARKERS: &[&str] = &["with", "wt", "w", "and", "x", "vs", "meet", "meets"];
/// "Alice & Bob", "Alice / Bob": joins two people.
const CONJ: &[char] = &['&', '/', ',', '+', '<', '>'];
/// "Interview: …", "(Alice)": separates parts of a title.
const SEPS: &[char] = &['(', ')', ':', '|', '-', '–', '—', '[', ']'];
const CONJ_WORDS: &[&str] = &["and", "x", "vs", "&"];

fn is_name_word(w: &str) -> bool {
    let mut cs = w.chars();
    match cs.next() {
        // "Maya", "O'Brien", "Jean-Luc"; not "AI", "ACME", "1:1".
        Some(c) if c.is_uppercase() => {
            let rest: String = cs.collect();
            !rest.is_empty() && rest.chars().all(|c| c.is_alphabetic() || c == '\'' || c == '-') && rest.chars().any(|c| c.is_lowercase())
        }
        _ => false,
    }
}

fn pretty(s: &str) -> String {
    s.split(|c: char| c == '.' || c == '_' || c == '-' || c.is_whitespace())
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c.flat_map(|x| x.to_lowercase())).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Title split into words, with joiners like "&" or "(" turned into separate marker tokens.
fn title_words(title: &str) -> Vec<String> {
    let mut spaced = String::new();
    for c in title.chars() {
        if CONJ.contains(&c) {
            spaced.push_str(" & ");
        } else if SEPS.contains(&c) {
            spaced.push_str(" § ");
        } else {
            spaced.push(c);
        }
    }
    spaced.split_whitespace().map(|w| w.trim_matches(|c: char| c == '.' || c == '"' || c == '\'').to_string()).filter(|w| !w.is_empty()).collect()
}

/// Runs of 1–3 capitalised words at the start of the title or right after a marker
/// ("with", "and", "&", "(").
fn title_names(title: &str) -> Vec<String> {
    let w = title_words(title);
    let mut out = Vec::new();
    let mut i = 0;
    while i < w.len() {
        let after_marker = i == 0 || MARKERS.contains(&w[i - 1].to_lowercase().as_str()) || w[i - 1] == "&" || w[i - 1] == "§";
        if after_marker && is_name_word(&w[i]) {
            let mut j = i;
            while j < w.len() && j - i < 3 && is_name_word(&w[j]) {
                j += 1;
            }
            // A run that ends in a word not followed by a marker or the end is a phrase,
            // not a name ("Recruiter Screen wt …" only counts "Omar Haddad").
            let next = w.get(j).map(|x| x.to_lowercase());
            let ends_cleanly = if i == 0 {
                // The title's opening words are a name only when joined to another person
                // ("Alice Wong and Bob"), not when they're the meeting's name ("Recruiter
                // Screen wt …", "Interview: …").
                next.as_deref().is_some_and(|n| CONJ_WORDS.contains(&n))
            } else {
                next.as_deref().is_none_or(|n| MARKERS.contains(&n) || n == "&" || n == "§")
            };
            if ends_cleanly {
                out.push(w[i..j].join(" "));
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    out
}

/// People other than the user who were probably in the meeting, best-known name first.
/// `user_name` is excluded (and so is anyone marked `is_self`).
pub fn candidates(title: &str, attendees: &[Attendee], user_name: &str) -> Vec<String> {
    let me: Vec<String> = user_name.split_whitespace().map(|w| w.to_lowercase()).collect();
    let is_me = |n: &str| {
        let f = n.split_whitespace().next().unwrap_or("").to_lowercase();
        !f.is_empty() && me.first() == Some(&f)
    };
    let from_title: Vec<String> = title_names(title).into_iter().filter(|n| !is_me(n)).collect();
    let mut out: Vec<String> = Vec::new();
    for a in attendees.iter().filter(|a| !a.is_self) {
        let local = a.email.as_deref().and_then(|e| e.split('@').next()).unwrap_or("");
        let base = if a.name.trim().is_empty() || a.name.contains('@') { local.to_string() } else { a.name.trim().to_string() };
        if base.is_empty() || is_me(&base) {
            continue;
        }
        let first = pretty(&base).split_whitespace().next().unwrap_or("").to_lowercase();
        // The title's fuller version of the same person wins ("maya" -> "Maya Lindqvist").
        let full = from_title
            .iter()
            .find(|t| t.split_whitespace().next().map(|f| f.to_lowercase()) == Some(first.clone()) && t.split_whitespace().count() > base.split_whitespace().count())
            .cloned();
        let name = full.unwrap_or_else(|| if base.chars().any(|c| c.is_uppercase()) { base.clone() } else { pretty(&base) });
        if !out.iter().any(|o| o.to_lowercase() == name.to_lowercase()) {
            out.push(name);
        }
    }
    for t in from_title {
        let f = t.split_whitespace().next().unwrap_or("").to_lowercase();
        if !out.iter().any(|o| o.split_whitespace().next().map(|x| x.to_lowercase()) == Some(f.clone())) {
            out.push(t);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn att(name: &str, email: &str, me: bool) -> Attendee {
        Attendee { name: name.into(), email: Some(email.into()), is_self: me }
    }

    #[test]
    fn title_gives_the_full_name() {
        let a = [att("sam", "sam@example.com", true), att("maya", "maya@acme.io", false)];
        assert_eq!(candidates("Sam Carter and Maya Lindqvist", &a, "Sam Carter"), vec!["Maya Lindqvist"]);
    }

    #[test]
    fn titles_without_invites() {
        assert_eq!(candidates("Northwind Recruiter Screen wt Omar Haddad", &[], "Sam Carter"), vec!["Omar Haddad"]);
        assert_eq!(
            candidates("Interview: Contoso AI (Ines Moreau & Theo Park)", &[], "Sam"),
            vec!["Ines Moreau", "Theo Park"]
        );
        assert_eq!(candidates("Interview with ACME wt Ines and Theo", &[], "Sam"), vec!["Ines", "Theo"]);
        assert!(candidates("Weekly Product Sync", &[], "Sam").is_empty());
        assert!(candidates("Fabrikam Interview (Sam Carter)", &[], "Sam Carter").is_empty());
    }

    #[test]
    fn email_only_attendees_are_prettified() {
        let a = [att("", "jo.lee@acme.com", false)];
        assert_eq!(candidates("Pricing review", &a, "Sam"), vec!["Jo Lee"]);
    }

    #[test]
    fn scheduler_plus_title_names_gives_several() {
        let a = [att("Priya Shah", "priya@acme.io", false)];
        assert_eq!(candidates("Interview with ACME wt Ines and Theo", &a, "Sam"), vec!["Priya Shah", "Ines", "Theo"]);
    }
}
