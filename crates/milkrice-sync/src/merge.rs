//! Records and the merge. The iPhone (Swift) implements exactly the same rules; the shared test
//! vectors in `tests/vectors/v1.json` keep the two honest.
//!
//! A record is one device's view of one meeting: `{v, id, device, fields: {name: {v, hlc}}}`.
//! Each field merges on its own, last writer wins by hybrid logical clock (HLC). An HLC is the
//! string `"<unix ms, 13 digits>-<counter, 4 digits>-<device id>"`, so plain string order is
//! clock order, and clock skew between devices can't reorder edits made more than a moment apart.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// The fields that sync. Anything else in a record is ignored by this version.
pub const FIELDS: &[&str] = &[
    "title", "notes", "enhanced", "enhanced_by", "transcript", "transcribed_by", "created_at", "duration_ms", "mode",
    "origin", "deleted", "notes_request",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stamped {
    pub v: Value,
    pub hlc: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub v: u32,
    pub id: String,
    pub device: String,
    pub fields: BTreeMap<String, Stamped>,
}

/// What this device last saw for one field of one meeting: a hash of the value and its HLC.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldState {
    pub hash: String,
    pub hlc: String,
}

pub type MeetingState = BTreeMap<String, FieldState>;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Clock {
    pub device: String,
    pub ms: u64,
    pub counter: u32,
}

impl Clock {
    pub fn new(device: &str) -> Self {
        Self { device: device.into(), ms: 0, counter: 0 }
    }

    pub fn tick(&mut self, wall_ms: u64) -> String {
        if wall_ms > self.ms {
            self.ms = wall_ms;
            self.counter = 0;
        } else {
            self.counter += 1;
        }
        format!("{:013}-{:04}-{}", self.ms, self.counter, self.device)
    }

    /// Move past a clock seen on another device so our next tick sorts after it.
    pub fn observe(&mut self, hlc: &str) {
        let mut it = hlc.splitn(3, '-');
        let (Some(ms), Some(c)) = (it.next().and_then(|s| s.parse::<u64>().ok()), it.next().and_then(|s| s.parse::<u32>().ok())) else {
            return;
        };
        if ms > self.ms || (ms == self.ms && c > self.counter) {
            self.ms = ms;
            self.counter = c;
        }
    }
}

/// A short hash of a value over a canonical encoding that Swift reproduces byte for byte
/// (JSON text differs between encoders, e.g. Swift escapes "/").
pub fn value_hash(v: &Value) -> String {
    let mut buf = Vec::new();
    canonical(v, &mut buf);
    hex::encode(&Sha256::digest(&buf)[..8])
}

/// `s<len>:<utf8>` · `i<int>;` · `f<float>;` · `b1`/`b0` · `n` · `a<n>[...]` · `o<n>{key value...}` (keys sorted).
pub fn canonical(v: &Value, out: &mut Vec<u8>) {
    match v {
        Value::Null => out.push(b'n'),
        Value::Bool(b) => out.extend_from_slice(if *b { b"b1" } else { b"b0" }),
        Value::Number(n) => match n.as_i64() {
            Some(i) => out.extend_from_slice(format!("i{i};").as_bytes()),
            None => out.extend_from_slice(format!("f{};", n.as_f64().unwrap_or(0.0)).as_bytes()),
        },
        Value::String(s) => {
            out.extend_from_slice(format!("s{}:", s.len()).as_bytes());
            out.extend_from_slice(s.as_bytes());
        }
        Value::Array(a) => {
            out.extend_from_slice(format!("a{}[", a.len()).as_bytes());
            a.iter().for_each(|x| canonical(x, out));
            out.push(b']');
        }
        Value::Object(o) => {
            out.extend_from_slice(format!("o{}{{", o.len()).as_bytes());
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort();
            for k in keys {
                canonical(&Value::String(k.clone()), out);
                canonical(&o[k], out);
            }
            out.push(b'}');
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outcome {
    /// Fields whose value another device changed: apply these to the local store.
    pub apply: BTreeMap<String, Value>,
    /// This device's record to write (its full current view, stamped).
    pub record: Record,
}

/// Merge one meeting.
///
/// * `local`: this device's current values (absent fields are simply not known here).
/// * `state`: what this device saw last time (updated in place).
/// * `remotes`: other devices' records for the meeting.
pub fn merge(
    id: &str,
    local: &BTreeMap<String, Value>,
    state: &mut MeetingState,
    remotes: &[Record],
    clock: &mut Clock,
    wall_ms: u64,
) -> Outcome {
    for r in remotes {
        for s in r.fields.values() {
            clock.observe(&s.hlc);
        }
    }
    // 1. Local edits since last time get a fresh stamp.
    for (name, v) in local {
        if !FIELDS.contains(&name.as_str()) {
            continue;
        }
        let h = value_hash(v);
        if state.get(name).map(|s| s.hash != h).unwrap_or(true) {
            state.insert(name.clone(), FieldState { hash: h, hlc: clock.tick(wall_ms) });
        }
    }
    // 2. Newer remote values win.
    let mut values: BTreeMap<String, Value> = local.clone();
    let mut apply = BTreeMap::new();
    for r in remotes {
        if r.id != id {
            continue;
        }
        for (name, s) in &r.fields {
            if !FIELDS.contains(&name.as_str()) {
                continue;
            }
            let newer = state.get(name).map(|cur| s.hlc > cur.hlc).unwrap_or(true);
            if newer {
                let h = value_hash(&s.v);
                let changed = values.get(name).map(|lv| value_hash(lv) != h).unwrap_or(true);
                state.insert(name.clone(), FieldState { hash: h, hlc: s.hlc.clone() });
                if changed {
                    apply.insert(name.clone(), s.v.clone());
                }
                values.insert(name.clone(), s.v.clone());
            }
        }
    }
    // 3. Our record: every field we hold a value for, with its winning stamp.
    let fields = values
        .into_iter()
        .filter_map(|(name, v)| state.get(&name).map(|s| (name, Stamped { v, hlc: s.hlc.clone() })))
        .collect();
    Outcome { apply, record: Record { v: 1, id: id.into(), device: clock.device.clone(), fields } }
}

/// True when the phone asked for notes after the last notes were written.
pub fn notes_requested(state: &MeetingState, request: Option<&Value>) -> bool {
    let Some(req) = state.get("notes_request") else { return false };
    if request.and_then(|v| v.as_str()).map(|s| s.is_empty()).unwrap_or(true) {
        return false;
    }
    state.get("enhanced").map(|e| req.hlc > e.hlc).unwrap_or(true)
}
