#!/bin/sh
# Renders fixtures/math/*.md with GitHub's Markdown API into reference/math-github/ (gitignored).
# Then `cargo test --test math -- --ignored` diffs the math against mddog's.
set -eu
cd "$(dirname "$0")/.."
mkdir -p reference/math-github
for f in fixtures/math/*.md; do
  [ "$(basename "$f")" = SOURCES.md ] && continue
  gh api -X POST /markdown -F text=@"$f" -f mode=gfm > "reference/math-github/$(basename "$f" .md).html"
done
