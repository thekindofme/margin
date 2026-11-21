#!/usr/bin/env bash
# End-to-end check that agents can read Milkrice through the CLI, the skill and MCP.
# Uses temporary configs only — your Claude Code / Codex settings are not modified.
# Usage: scripts/e2e-agents.sh [path/to/milkrice-cli]
set -euo pipefail
CLI="${1:-$(cd "$(dirname "$0")/.." && pwd)/target/release/milkrice-cli}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CLI="$(cd "$(dirname "$CLI")" && pwd)/$(basename "$CLI")"
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
mkdir -p "$T/bin" "$T/proj/.claude/skills/milkrice"
ln -s "$CLI" "$T/bin/milkrice"
cp "$ROOT/skills/milkrice/SKILL.md" "$T/proj/.claude/skills/milkrice/"
pass=0; fail=0
check() { if grep -qiE "$2" <<<"$3"; then echo "PASS  $1"; pass=$((pass+1)); else echo "FAIL  $1"; echo "$3" | head -20; fail=$((fail+1)); fi; }

out=$("$T/bin/milkrice" ls); check "cli ls" "id +date" "$out"
id=$(sed -n 2p <<<"$out" | cut -c1-8)
out=$("$T/bin/milkrice" show "$id" all); check "cli show labels sources" "AI SUMMARY.*TRANSCRIPT|TRANSCRIPT" "$out"
out=$(printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' | "$T/bin/milkrice" mcp)
check "mcp tools/list + readOnly" "readOnlyHint" "$out"

q="Using Milkrice, what is my most recent meeting called? Reply with just the title."
out=$(cd "$T/proj" && echo "$q" | PATH="$T/bin:$PATH" claude -p --model haiku --setting-sources project --allowedTools "Bash(milkrice:*)" "Skill" --no-session-persistence 2>&1)
check "claude + skill" "[a-z]" "$out"; echo "      → $out"
echo "{\"mcpServers\":{\"milkrice\":{\"command\":\"$CLI\",\"args\":[\"mcp\"]}}}" > "$T/mcp.json"
out=$(cd "$T" && echo "$q" | claude -p --model haiku --mcp-config "$T/mcp.json" --strict-mcp-config --setting-sources "" --allowedTools "mcp__milkrice__list_meetings,mcp__milkrice__get_meeting,mcp__milkrice__search,mcp__milkrice__get_transcript" --no-session-persistence 2>&1)
check "claude + mcp" "[a-z]" "$out"; echo "      → $out"
out=$(cd "$T" && codex exec --skip-git-repo-check --ephemeral --sandbox read-only -c model_reasoning_effort=low -c 'notify=[]' -c "mcp_servers.milkrice.command=\"$CLI\"" -c 'mcp_servers.milkrice.args=["mcp"]' -o "$T/c.txt" "$q Use the milkrice MCP tools." </dev/null >/dev/null 2>&1; cat "$T/c.txt")
check "codex + mcp" "[a-z]" "$out"; echo "      → $out"
# Installer: a throwaway HOME, so nothing in your real ~/.cursor, ~/.codex or ~/.agents changes.
H="$T/home"; mkdir -p "$H/.cursor" "$H/.codex"
echo '{"mcpServers":{"other":{"command":"x"}}}' > "$H/.cursor/mcp.json"
agents() { env -u CLAUDE_CONFIG_DIR -u CODEX_HOME -u COPILOT_HOME -u XDG_CONFIG_HOME HOME="$H" "$CLI" agents "$@" 2>&1; }
out=$(agents install cursor; agents install codex; cat "$H/.cursor/mcp.json" "$H/.codex/config.toml")
check "agents install merges configs" '"other".*|mcp_servers.milkrice' "$out"
out=$(agents status); check "agents status" "Cursor +connected" "$out"
out=$(agents uninstall all; cat "$H/.cursor/mcp.json"; ls "$H/.agents/skills" "$H/.codex" 2>&1)
if grep -q '"milkrice"' <<<"$out" || [ -e "$H/.codex/config.toml" ] || [ -e "$H/.agents/skills/milkrice" ]; then echo "FAIL  agents uninstall all"; echo "$out" | head -20; fail=$((fail+1)); else echo "PASS  agents uninstall all"; pass=$((pass+1)); fi
out=$(printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | HOME="$T" PATH="$T/bin:$PATH" "$ROOT/scripts/milkrice-mcp")
check "plugin MCP shim" "serverInfo" "$out"
if command -v claude >/dev/null; then out=$(claude plugin validate "$ROOT" 2>&1); check "claude plugin validate" "Validation passed" "$out"; fi

echo "$pass passed, $fail failed"; [ "$fail" = 0 ]
