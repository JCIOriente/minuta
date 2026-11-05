#!/usr/bin/env bash
# Size limit for Rust sources: no .rs file under src/ may have more than 500 lines,
# unless it is on the allowlist, and then only up to the count recorded there
# (the ratchet). Spec: docs/specs/2026-11-03-src-file-size-limit.md
#
# FILE_SIZE_SRC        directory to scan (default src)
# FILE_SIZE_ALLOWLIST  allowlist file (default scripts/file-size-allowlist.txt)
#
# Exit 0 when every rule holds, 1 after printing each violation, 2 when the input
# itself is bad. Runs on bash 3.2 (macOS): no associative arrays, and no expansion
# of a possibly empty array under `set -u`.
set -euo pipefail
set -f

LIMIT=500
src=${FILE_SIZE_SRC:-src}
allowlist=${FILE_SIZE_ALLOWLIST:-scripts/file-size-allowlist.txt}

fatal() {
  echo "file-size: $*" >&2
  exit 2
}

# One spelling of the directory, so allowlist paths compare as plain strings.
case "$src" in
  /* | ./* | */ | . | ..)
    fatal "FILE_SIZE_SRC must be a relative path without a leading ./ or a trailing / (got '$src')"
    ;;
esac
[ -d "$src" ] || fatal "$src: not a directory"
{ [ -f "$allowlist" ] && [ -r "$allowlist" ]; } || fatal "$allowlist: cannot read the allowlist"

# Allowlist entries as newline-separated "<path> <max>" lines.
entries=""
lineno=0
while IFS= read -r line || [ -n "$line" ]; do
  lineno=$((lineno + 1))
  # Split on spaces and tabs; globbing is off (set -f).
  # shellcheck disable=SC2086
  set -- $line
  [ $# -gt 0 ] || continue
  case "$1" in '#'*) continue ;; esac
  where="$allowlist:$lineno"
  [ $# -eq 2 ] || fatal "$where: expected '<path> <max lines>'"
  path=$1
  max=$2
  case "$max" in
    '' | 0* | *[!0-9]*) fatal "$where: max lines must be a positive integer (got '$max')" ;;
  esac
  # More digits than this would overflow `[ -gt ]`; no source file comes close.
  [ ${#max} -le 9 ] || fatal "$where: max lines $max is too large"
  [ "$max" -gt "$LIMIT" ] || fatal "$where: max lines $max is within the limit ($LIMIT); remove the entry"
  case "$path" in *.rs) ;; *) fatal "$where: $path is not a .rs file" ;; esac
  case "$path" in "$src"/*) ;; *) fatal "$where: $path is not under $src/" ;; esac
  if printf '%s' "$entries" | awk -v p="$path" '$1 == p { found = 1 } END { exit !found }'; then
    fatal "$where: $path is listed twice"
  fi
  entries="$entries$path $max
"
done < "$allowlist"

violations=0
report() {
  echo "$1" >&2
  violations=$((violations + 1))
}

# Byte order, so the violation lines always come in the same order.
files=$(find "$src" -type f -name '*.rs' | LC_ALL=C sort) || fatal "$src: cannot list files"
listed_seen=""
while IFS= read -r file; do
  [ -n "$file" ] || continue
  # awk counts a last line without a trailing newline; wc -l would not.
  lines=$(awk 'END { print NR }' "$file") || fatal "$file: cannot read"
  max=$(printf '%s' "$entries" | awk -v p="$file" '$1 == p { print $2 }')
  if [ -z "$max" ]; then
    if [ "$lines" -gt "$LIMIT" ]; then
      report "$file: $lines lines, limit $LIMIT; split it into modules"
    fi
    continue
  fi
  listed_seen="$listed_seen$file
"
  if [ "$lines" -le "$LIMIT" ]; then
    report "$file: $lines lines, within the limit; remove it from the allowlist"
  elif [ "$lines" -gt "$max" ]; then
    report "$file: $lines lines, allowlist allows $max; it may only shrink"
  fi
done <<EOF
$files
EOF

while read -r path _; do
  [ -n "$path" ] || continue
  if ! printf '%s' "$listed_seen" | grep -qxF -- "$path"; then
    report "$path: listed in the allowlist but does not exist"
  fi
done <<EOF
$entries
EOF

if [ "$violations" -eq 0 ]; then
  exit 0
fi
if [ "$violations" -eq 1 ]; then
  echo "file-size: 1 violation" >&2
else
  echo "file-size: $violations violations" >&2
fi
exit 1
