#!/usr/bin/env bash
# End-to-end check that agents can read Margin through the CLI, the skill and MCP.
# Uses temporary configs only — your Claude Code / Codex settings are not modified.
# Usage: scripts/e2e-agents.sh [path/to/margin-cli]
set -euo pipefail
CLI="${1:-$(cd "$(dirname "$0")/.." && pwd)/target/release/margin-cli}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CLI="$(cd "$(dirname "$CLI")" && pwd)/$(basename "$CLI")"
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
mkdir -p "$T/bin" "$T/proj/.claude/skills/margin"
ln -s "$CLI" "$T/bin/margin"
cp "$ROOT/skill/margin/SKILL.md" "$T/proj/.claude/skills/margin/"
pass=0; fail=0
check() { if grep -qiE "$2" <<<"$3"; then echo "PASS  $1"; pass=$((pass+1)); else echo "FAIL  $1"; echo "$3" | head -20; fail=$((fail+1)); fi; }

out=$("$T/bin/margin" ls); check "cli ls" "id +date" "$out"
id=$(sed -n 2p <<<"$out" | cut -c1-8)
out=$("$T/bin/margin" show "$id" all); check "cli show labels sources" "AI SUMMARY.*TRANSCRIPT|TRANSCRIPT" "$out"
out=$(printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' | "$T/bin/margin" mcp)
check "mcp tools/list + readOnly" "readOnlyHint" "$out"

q="Using Margin, what is my most recent meeting called? Reply with just the title."
out=$(cd "$T/proj" && echo "$q" | PATH="$T/bin:$PATH" claude -p --model haiku --setting-sources project --allowedTools "Bash(margin:*)" "Skill" --no-session-persistence 2>&1)
check "claude + skill" "[a-z]" "$out"; echo "      → $out"
echo "{\"mcpServers\":{\"margin\":{\"command\":\"$CLI\",\"args\":[\"mcp\"]}}}" > "$T/mcp.json"
out=$(cd "$T" && echo "$q" | claude -p --model haiku --mcp-config "$T/mcp.json" --strict-mcp-config --setting-sources "" --allowedTools "mcp__margin__list_meetings,mcp__margin__get_meeting,mcp__margin__search,mcp__margin__get_transcript" --no-session-persistence 2>&1)
check "claude + mcp" "[a-z]" "$out"; echo "      → $out"
out=$(cd "$T" && codex exec --skip-git-repo-check --ephemeral --sandbox read-only -c model_reasoning_effort=low -c 'notify=[]' -c "mcp_servers.margin.command=\"$CLI\"" -c 'mcp_servers.margin.args=["mcp"]' -o "$T/c.txt" "$q Use the margin MCP tools." </dev/null >/dev/null 2>&1; cat "$T/c.txt")
check "codex + mcp" "[a-z]" "$out"; echo "      → $out"
echo "$pass passed, $fail failed"; [ "$fail" = 0 ]
