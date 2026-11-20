# Margin

Local-first AI meeting notes for macOS, written in Rust. An open-source alternative to tools like Granola, with no account and no sign-up:

- **Listens on your Mac.** It captures your mic ("Me") and system audio (everyone else on the call), then transcribes live with an **open-weights model you download once**. The model choice follows Handy: Parakeet v2/v3, Canary, Cohere Transcribe, Whisper, Moonshine and SenseVoice. Audio is never uploaded, and it's only kept on disk if you turn on **Keep recordings**.
- **Knows who's talking.** A local speaker-embedding model (WeSpeaker ResNet34) separates the remote voices. Names come from the calendar invite and from cues in the conversation ("thanks, Priya"), and you can rename or merge speakers with a click.
- **Hooks into your calendar.** It reads the calendars already synced to macOS (iCloud, Google, Exchange) through EventKit. You get a notification (and a "Next: … · Start notes" item in the menu bar) as a meeting starts, or it can record automatically. While auto-record is on, Margin shows it in the sidebar and menu bar, with a one-click "Turn off". It stops when the meeting is over and the room goes quiet. It also notices unscheduled Zoom, Teams, Webex, Slack and FaceTime calls that are using the mic.
- **Writes your notes with the AI you already pay for.** It uses **Claude Haiku** through the Claude Code CLI or **Codex Luna** through the Codex CLI, reusing their existing subscription logins. A fully offline Ollama option is also available. No API keys are needed. You choose the provider once during setup and Margin remembers it. Until you do, nothing is sent anywhere and notes aren't written.
- **Granola-style workflow:**
  - Jot rough notes during the call. Afterwards Margin expands them with detail from the transcript, using templates for 1:1s, stand-ups, customer calls, interviews and more.
  - The AI notes are yours to edit: action items tick off, and the note says which model wrote it. Speaker names the AI guessed are marked until you confirm them.
  - Ask questions about the meeting ("draft a follow-up email").
  - Search across every note and transcript.
  - Export notes as Markdown.

## For agents: CLI, MCP and skill

Margin exposes your meetings to Claude Code, Codex and any MCP client. Access is read-only, and every piece of content is labelled with its source:

- **NOTES**: what you typed.
- **AI SUMMARY**: written by an LLM, which is named.
- **TRANSCRIPT**: speech-to-text from a named model. Your own lines are marked `(user)`.

To connect, use **Settings → Agents**. It installs the `margin` command, then for Claude Code or Codex it registers the MCP server and installs the skill (`skill/margin/SKILL.md`).

| Command | What it gives you |
|---|---|
| `margin ls [-q TEXT] [--since 7d]` | One line per meeting, with which content exists |
| `margin show ID [notes,ai,transcript\|all]` | Labelled sections; transcript left out by default |
| `margin tx ID [--from mm:ss --to mm:ss --speaker NAME --grep TEXT]` | Only the transcript lines you need |
| `margin search TEXT [--in notes\|ai\|transcript]` | Hits say where they matched |
| `margin mcp` | MCP server on stdio: `list_meetings`, `get_meeting`, `get_transcript`, `search` |

## Install

Apple Silicon Macs, macOS 14.2 or later.

**Homebrew**

```bash
brew install --cask thekindofme/tap/margin
```

**Direct download.** Get the DMG from the [latest release](https://github.com/thekindofme/margin/releases/latest), open it and drag Margin to Applications. Checksums are in `SHA256SUMS.txt` on the release page.

**Build from source.** See [Build and run](#build-and-run) and [CONTRIBUTING.md](CONTRIBUTING.md).

Margin checks GitHub for new releases shortly after launch (and from "Check for updates…" in the menu bar) and asks before installing one.

## Layout

```
crates/margin-core   the engine (no UI)
  audio/             mic (cpal) + system audio (ScreenCaptureKit) → 16 kHz mono
  vad.rs             adaptive energy VAD, cuts streams into utterances
  stt.rs, catalog.rs transcribe-rs engines + model catalog
  models.rs          download / unpack / delete models
  diarize/           Kaldi fbank + WeSpeaker embeddings + online clustering
  session.rs         live pipeline: capture → VAD → STT → speakers → echo removal
  calendar.rs        EventKit (events, attendees, call links)
  detect.rs          "is a meeting app using the mic?"
  llm.rs             Claude Code / Codex / Ollama runners + prompts
  store.rs           SQLite (WAL) storage
  bin/margin-cli.rs  headless tool, including a full pipeline simulator
src-tauri            the desktop app: commands, scheduler, tray, notifications
ui                   the interface: plain HTML/CSS/JS, no build step
design               brand notes and Codex-generated artwork
```

## Build and run

```bash
cargo tauri build --bundles app
```

```bash
open target/release/bundle/macos/Margin.app
```

Requirements: macOS 14.2+, Rust, `cargo install tauri-cli`, and cmake (for whisper.cpp).
On first launch, Margin asks for permission to use the microphone, calendars and "Screen & System Audio Recording". The last one is needed to hear the other participants; your screen is never captured.

### Headless

```bash
cargo run --release -p margin-core --bin margin-cli -- models
```

```bash
cargo run --release -p margin-core --bin margin-cli -- simulate parakeet-v2 mic.wav system.wav claude
```

`simulate` streams two 16 kHz mono WAVs (your mic and the call audio) through the exact live pipeline. It then names the speakers, titles the meeting, writes enhanced notes and answers a question with the chosen AI.

### Automated end-to-end checks

These run the real app binary with test audio in place of the live devices:

```bash
MARGIN_SELFTEST=dir/ target/release/margin
```

Plays `dir/mic.wav` and `dir/system.wav` through the recorder, storage and AI pass, then quits.

```bash
MARGIN_TEST_EVENT=50 MARGIN_QUIET_SECS=15 MARGIN_TEST_AUDIO=dir/ target/release/margin
```

Injects a calendar meeting. The real scheduler auto-starts recording, auto-stops after the event ends and the room goes quiet, then names speakers and writes the notes.

## Performance (M5 MacBook Air, CPU int8)

| Model | 5 s utterance | Peak RAM |
|---|---|---|
| Parakeet v2 | 0.27 s (~19× real time) | 1.9 GB |
| Canary 180M Flash | 0.34 s | 0.7 GB |
| Moonshine Base | 0.25 s | 0.6 GB |
| Whisper Small (Metal) | 1.1 s | 0.8 GB |

- **Full pipeline (two streams, diarization, echo removal):** 26× real time, so a 44 s meeting takes 1.7 s.
- **Model load:** 0.5 s.
- **Idle app:** ~0% CPU, ~100 MB RAM.

## Privacy

- **Stays on your Mac:** audio, transcripts, notes and models live in `~/Library/Application Support/Margin`. There is no account, no telemetry and no Margin server.
- **Recordings:** audio is processed in memory and discarded. With **Keep recordings** on, Margin keeps a compressed copy per meeting and deletes it after the retention period you choose.
- **Speaker voice prints:** these are kept in memory only, never written to disk.
- **What goes over the network:**
  - Model downloads.
  - Your calendar feed, if you add a secret iCal address.
  - A check for new Margin releases on GitHub (the app asks before installing anything).
  - When notes are written, the transcript text and your notes go to the AI you chose (Claude Code or Codex, using your own subscription). Choose Ollama to keep even that on-device.
- **Obsidian:** export writes plain Markdown files into the folder you pick.

## License

MIT. See [LICENSE](LICENSE). The speech models Margin downloads keep their own licenses, some of which require attribution; see the acknowledgements page on the website (`site/acknowledgements.html`).

Margin is an independent project. It isn't affiliated with Granola, Anthropic, OpenAI or the Handy project. Their names are used only to describe compatibility.
