#!/usr/bin/env bash
# Print the CHANGELOG.md section of one version, for the GitHub release notes.
# Usage: release-notes.sh <X.Y.Z> [changelog file, default CHANGELOG.md]
# The section is the text under "## <version>" up to the next "## " heading, without
# leading and trailing blank lines. Exits 1 if the section is missing or has no text,
# 2 on bad usage or an unreadable file, so a release never goes out with empty notes.
set -euo pipefail

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  echo "usage: release-notes.sh <X.Y.Z> [changelog]" >&2
  exit 2
fi
version=$1
file=${2:-CHANGELOG.md}

if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "release-notes: version '$version' is not X.Y.Z" >&2
  exit 2
fi
if [ ! -r "$file" ]; then
  echo "release-notes: $file: cannot read the changelog" >&2
  exit 2
fi

# awk compares strings, so the dots of the version are not patterns. Blank lines are held
# back until text follows, which trims the trailing ones.
notes=$(awk -v want="## $version" '
  /^## / { if (inside) exit; inside = ($0 == want); next }
  inside {
    if ($0 ~ /^[[:space:]]*$/) { if (started) held = held "\n"; next }
    printf "%s", held; held = ""; started = 1; print
  }
' "$file")

if [ -z "$notes" ]; then
  echo "release-notes: $file has no non-empty section '## $version'" >&2
  exit 1
fi
printf '%s\n' "$notes"
