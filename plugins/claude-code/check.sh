#!/bin/sh
set -eu
cd "$(dirname "$0")"
for tool in claude npx; do
  command -v "$tool" >/dev/null || { echo "plugins/claude-code/check.sh: $tool isn't on the PATH, and the plugin checks need it." >&2; exit 1; }
done
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir "$tmp/config"
env -u ANTHROPIC_API_KEY -u ANTHROPIC_AUTH_TOKEN -u CLAUDE_CODE_OAUTH_TOKEN CLAUDE_CODE_TMPDIR="$tmp" CLAUDE_CONFIG_DIR="$tmp/config" claude -p /plugin-authoring >/dev/null 2>&1 || true
api=$(find "$tmp" -path '*/plugin-authoring/types/claude-code.d.ts' | head -n 1)
if [ -z "$api" ]; then
  echo "plugins/claude-code/check.sh: Claude Code $(claude --version) didn't write its plugin API types (claude-code.d.ts) under CLAUDE_CODE_TMPDIR when /plugin-authoring loaded, so the plugin can't be type-checked. Set plugins/claude-code/claude-code-version, which CI installs, to the last version that did." >&2
  exit 1
fi
cat >"$tmp/tsconfig.json" <<JSON
{ "extends": "$PWD/tsconfig.json", "include": ["$api", "$PWD/hooks", "$PWD/types", "$PWD/tests"] }
JSON
npx --yes -p typescript@5.9.3 tsc -p "$tmp/tsconfig.json"
claude plugin validate .
claude plugin test .
