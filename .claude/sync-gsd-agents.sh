#!/usr/bin/env bash
# Regenerate the project-local GSD agent overrides in .claude/agents/.
#
# Project agents take precedence over the global ones in ~/.claude/agents/. Each override is the global definition
# verbatim, except that `LSP` is appended to its `tools:` line, so every GSD subagent can query rust-analyzer
# (CLAUDE.md rule 7: two code oracles). Re-run this after every GSD update; it never edits the global files.
set -euo pipefail

SRC="${CLAUDE_CONFIG_DIR:-$HOME/.claude}/agents"
DST="$(cd "$(dirname "$0")" && pwd)/agents"

AGENTS=(
  gsd-executor
  gsd-verifier
  gsd-code-reviewer
  gsd-code-fixer
  gsd-debugger
  gsd-planner
  gsd-plan-checker
  gsd-phase-researcher
  gsd-pattern-mapper
  gsd-integration-checker
  gsd-nyquist-auditor
  gsd-security-auditor
  gsd-codebase-mapper
)

mkdir -p "$DST"
for name in "${AGENTS[@]}"; do
  src="$SRC/$name.md"
  [ -f "$src" ] || { echo "missing $src" >&2; exit 1; }
  # Only the first `tools:` line, inside the front matter, is changed.
  awk '
    BEGIN { fm = 0; done = 0 }
    /^---$/ { fm++ }
    fm == 1 && !done && /^tools:/ {
      if ($0 !~ /(^|[ ,])LSP(,|$)/) { $0 = $0 ", LSP" }
      done = 1
    }
    { print }
    END { if (!done) { print "no tools: line in front matter" > "/dev/stderr"; exit 1 } }
  ' "$src" > "$DST/$name.md"
  echo "synced $name"
done
