# Changelog

All notable changes to Milkrice are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## 0.1.0 (Unreleased)

First public release.

- Live transcription of mic and system audio with downloadable open-weights models (Parakeet, Canary, Whisper, Moonshine, SenseVoice, Cohere Transcribe)
- Local speaker separation, with names from calendar invites and conversation cues
- Calendar integration through EventKit and optional iCal feeds, with notify or auto-record modes
- AI-written notes through Claude Code, Codex or Ollama
- Search, folders, Markdown and Obsidian export
- `milkrice` command-line tool, MCP server and agent skill for read-only access to meetings
- Automatic update checks (asks before installing)
- Homebrew cask: `brew install --cask thekindofme/tap/milkrice`
- Pre-release builds were called Margin. On first launch Milkrice moves the old `Application Support/Margin` data folder to `Milkrice` (database and log renamed), and Settings, Agents replaces the old `margin` command and MCP registration
