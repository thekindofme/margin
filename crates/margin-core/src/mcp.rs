//! Minimal MCP server (JSON-RPC 2.0 over stdio) exposing Margin's meetings to agents.
//! Read-only. Tools return the same compact, provenance-labelled text as the CLI.
use crate::agent::{self, Scope};
use crate::store::Store;
use anyhow::Result;
use serde_json::{json, Value};
use std::io::{BufRead, Write};

const PROTOCOL: &str = "2025-06-18";

fn tools() -> Value {
    json!([
        {
            "name": "list_meetings",
            "annotations": {"title": "List meetings", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false},
            "description": "List recent meetings (newest first) with short ids, date, length, people and which content exists: notes (user-written), ai (LLM summary), tx:N (transcript lines).",
            "inputSchema": {"type": "object", "properties": {
                "query": {"type": "string", "description": "Optional text filter across titles, notes, summaries, transcripts, attendees"},
                "since": {"type": "string", "description": "Only meetings newer than this, e.g. 7d, 24h, 2w"},
                "limit": {"type": "integer", "default": 20}
            }}
        },
        {
            "name": "get_meeting",
            "annotations": {"title": "Get meeting", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false},
            "description": "Get a meeting. Sections are labelled by source: 'notes' = written by the user, 'ai' = LLM-generated summary (may contain errors), 'transcript' = speech-to-text. Default sections: ai,notes (transcript is long; fetch it with get_transcript).",
            "inputSchema": {"type": "object", "required": ["id"], "properties": {
                "id": {"type": "string", "description": "Meeting id or 8-char prefix from list_meetings"},
                "sections": {"type": "string", "description": "Comma list of notes, ai, transcript, all", "default": "ai,notes"}
            }}
        },
        {
            "name": "get_transcript",
            "annotations": {"title": "Get transcript", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false},
            "description": "Speech-to-text transcript of a meeting as 'mm:ss Speaker: text' lines ('Me' is the user). Filter by time window, speaker or keyword to save tokens.",
            "inputSchema": {"type": "object", "required": ["id"], "properties": {
                "id": {"type": "string"},
                "from": {"type": "string", "description": "Start time mm:ss"},
                "to": {"type": "string", "description": "End time mm:ss"},
                "speaker": {"type": "string", "description": "Name or label substring"},
                "grep": {"type": "string", "description": "Only lines containing this text"},
                "limit": {"type": "integer", "default": 300}
            }}
        },
        {
            "name": "search",
            "annotations": {"title": "Search meetings", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false},
            "description": "Search all meetings. Each hit says where it matched: notes (user), ai (LLM summary) or transcript (with time and speaker).",
            "inputSchema": {"type": "object", "required": ["query"], "properties": {
                "query": {"type": "string"},
                "scope": {"type": "string", "enum": ["all", "notes", "ai", "transcript"], "default": "all"},
                "limit": {"type": "integer", "default": 20}
            }}
        }
    ])
}

fn s<'a>(args: &'a Value, k: &str) -> Option<&'a str> {
    args.get(k).and_then(|v| v.as_str()).filter(|v| !v.is_empty())
}

fn n(args: &Value, k: &str, d: usize) -> usize {
    args.get(k).and_then(|v| v.as_u64()).map(|v| v as usize).unwrap_or(d)
}

fn call(store: &Store, name: &str, args: &Value) -> Result<String> {
    match name {
        "list_meetings" => {
            let since = s(args, "since").map(agent::parse_since).transpose()?;
            agent::list(store, s(args, "query"), since, n(args, "limit", 20), false)
        }
        "get_meeting" => {
            let secs = agent::parse_sections(s(args, "sections").unwrap_or("ai,notes"))?;
            agent::show(store, s(args, "id").unwrap_or(""), &secs, false)
        }
        "get_transcript" => agent::transcript(
            store,
            s(args, "id").unwrap_or(""),
            s(args, "from").map(agent::parse_ts).transpose()?,
            s(args, "to").map(agent::parse_ts).transpose()?,
            s(args, "speaker"),
            s(args, "grep"),
            n(args, "limit", 300),
            false,
        ),
        "search" => {
            let scope = match s(args, "scope").unwrap_or("all") {
                "notes" => Scope::Notes,
                "ai" => Scope::Ai,
                "transcript" => Scope::Transcript,
                _ => Scope::All,
            };
            agent::search(store, s(args, "query").unwrap_or(""), scope, n(args, "limit", 20), false)
        }
        _ => anyhow::bail!("unknown tool {name}"),
    }
}

fn handle(store: &Store, req: &Value) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let result = match method {
        "initialize" => json!({
            "protocolVersion": req["params"]["protocolVersion"].as_str().unwrap_or(PROTOCOL),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "margin", "version": env!("CARGO_PKG_VERSION")},
            "instructions": agent::OVERVIEW,
        }),
        "ping" => json!({}),
        "tools/list" => json!({"tools": tools()}),
        "tools/call" => {
            let name = req["params"]["name"].as_str().unwrap_or("");
            let args = req["params"].get("arguments").cloned().unwrap_or(json!({}));
            match call(store, name, &args) {
                Ok(text) => json!({"content": [{"type": "text", "text": text}]}),
                Err(e) => json!({"content": [{"type": "text", "text": format!("error: {e}")}], "isError": true}),
            }
        }
        _ if id.is_none() => return None, // notification
        _ => return Some(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("method not found: {method}")}})),
    };
    id.map(|id| json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

/// Serve MCP on stdin/stdout until EOF.
pub fn serve(store: &Store) -> Result<()> {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let resp = match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(batch)) => {
                let r: Vec<Value> = batch.iter().filter_map(|m| handle(store, m)).collect();
                if r.is_empty() { None } else { Some(Value::Array(r)) }
            }
            Ok(req) => handle(store, &req),
            Err(e) => Some(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": e.to_string()}})),
        };
        if let Some(r) = resp {
            writeln!(out, "{r}")?;
            out.flush()?;
        }
    }
    Ok(())
}
