#!/usr/bin/env bash
# SessionStart: hands Claude the `using-superpowers` skill, the entry point that
# makes it look for a matching skill before acting. This is what the Superpowers
# plugin's own hook does; the skills are vendored here (scripts/sync-agent-skills.sh)
# because plugins do not load in cloud sessions.
#
# Reads one file and prints JSON; never blocks a session: any failure exits 0.
set -uo pipefail

skill="${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "$0")/../.." && pwd)}/.claude/skills/using-superpowers/SKILL.md"
[ -r "${skill}" ] || exit 0
content="$(<"${skill}")"

escape_json() {
  local s="$1"
  s="${s//\\/\\\\}"
  s="${s//\"/\\\"}"
  s="${s//$'\n'/\\n}"
  s="${s//$'\r'/\\r}"
  s="${s//$'\t'/\\t}"
  printf '%s' "${s}"
}

intro="This project vendors the Superpowers skills in .claude/skills/ (invoke them by their plain name, e.g. \`brainstorming\`, \`test-driven-development\`). Follow \`karpathy-guidelines\` when writing or reviewing code. Below is the \`using-superpowers\` skill:"
printf '{"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":"%s\\n\\n%s"}}\n' \
  "$(escape_json "${intro}")" "$(escape_json "${content}")"
exit 0
