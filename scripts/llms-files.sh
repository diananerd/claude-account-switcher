#!/bin/sh
# The site's files for LLMs, built from the docs so they cannot drift:
#
#   llms.txt       the summary (llms.txt at the root, as is)
#   llms-full.txt  the summary followed by every doc page, in one plain file
#
#   scripts/llms-files.sh [out-dir]    default: docs/public

set -eu
cd "$(dirname "$0")/.."
out=${1:-docs/public}

cp llms.txt "$out/llms.txt"
{
  cat llms.txt
  for f in README.md docs/reference.md docs/how-it-works.md docs/comparison.md; do
    printf '\n\n---\n\n<!-- Source: %s -->\n\n' "$f"
    cat "$f"
  done
} > "$out/llms-full.txt"
