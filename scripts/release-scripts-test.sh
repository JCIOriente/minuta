#!/usr/bin/env bash
# Tests for release-notes.sh and crate-published.sh. Each case runs the script in a temporary
# directory and asserts the exit code, stdout and stderr exactly. crate-published.sh runs
# against a fake `curl` first on PATH, so no case reaches the network.
# Every case runs under each bash found (the one on PATH and /bin/bash, bash 3.2 on macOS).
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT

shells=$(command -v bash)
if [ -x /bin/bash ] && ! [ /bin/bash -ef "$shells" ]; then
  shells="$shells /bin/bash"
fi

# The fake curl records its arguments and answers FAKE_STATUS with FAKE_BODY, or fails.
mkdir "$root/bin"
cat > "$root/bin/curl" <<'FAKE'
#!/usr/bin/env bash
out=""
for arg in "$@"; do
  if [ "$prev" = --output ]; then out=$arg; fi
  prev=$arg
done
printf '%s\n' "$@" > "$FAKE_ARGS"
[ "${FAKE_FAIL:-}" = 1 ] && exit 7
printf '%s' "${FAKE_BODY:-}" > "$out"
printf '%s' "$FAKE_STATUS"
FAKE
chmod +x "$root/bin/curl"

failures=0
cases=0

new_case() {
  case_name=$1
  case_dir="$root/$1"
  mkdir -p "$case_dir"
  cd "$case_dir"
  unset FAKE_STATUS FAKE_BODY FAKE_FAIL
}

# expect <script> <exit code> <stdout> <stderr> -- <args...>
expect() {
  local script=$1 code=$2 want_out=$3 want_err=$4 shell got out err
  shift 5
  for shell in $shells; do
    cases=$((cases + 1))
    got=0
    out=$(PATH="$root/bin:$PATH" FAKE_ARGS="$case_dir/args" "$shell" "$here/$script" "$@" 2> "$case_dir/stderr") || got=$?
    err=$(cat "$case_dir/stderr")
    if [ "$got" = "$code" ] && [ "$out" = "$want_out" ] && [ "$err" = "$want_err" ]; then
      echo "ok   $case_name ($shell)"
    else
      failures=$((failures + 1))
      echo "FAIL $case_name ($shell)"
      echo "  exit: expected $code, got $got"
      echo "  stdout expected: $want_out"; echo "  stdout actual:   $out"
      echo "  stderr expected: $want_err"; echo "  stderr actual:   $err"
    fi
  done
}

changelog() { printf '%s\n' "$@" > CHANGELOG.md; }

# --- release-notes.sh ---------------------------------------------------------------------

new_case notes_middle_section_stops_at_next_heading
changelog "# Changelog" "" "## 1.0.0" "" "### Added" "- one" "" "### Fixed" "- two" "" "## 0.9.0" "- old"
expect release-notes.sh 0 $'### Added\n- one\n\n### Fixed\n- two' "" -- 1.0.0

new_case notes_last_section_runs_to_end
changelog "# Changelog" "## 1.0.0" "- new" "## 0.9.0" "- a" "" "- b" "" ""
expect release-notes.sh 0 $'- a\n\n- b' "" -- 0.9.0

new_case notes_no_trailing_newline
printf '## 1.0.0\n- only' > CHANGELOG.md
expect release-notes.sh 0 '- only' "" -- 1.0.0

new_case notes_version_dots_are_literal
changelog "## 1x0x0" "- wrong" "## 1.0.0" "- right"
expect release-notes.sh 0 '- right' "" -- 1.0.0

new_case notes_prefix_version_is_not_a_match
changelog "## 1.0.01" "- other" "## 1.0.10" "- ten"
expect release-notes.sh 1 "" "release-notes: CHANGELOG.md has no non-empty section '## 1.0.0'" -- 1.0.0
expect release-notes.sh 1 "" "release-notes: CHANGELOG.md has no non-empty section '## 1.0.1'" -- 1.0.1

new_case notes_missing_section_fails
changelog "## 0.9.0" "- a"
expect release-notes.sh 1 "" "release-notes: CHANGELOG.md has no non-empty section '## 1.0.0'" -- 1.0.0

new_case notes_empty_section_fails
changelog "## 1.0.0" "" "   " "## 0.9.0" "- a"
expect release-notes.sh 1 "" "release-notes: CHANGELOG.md has no non-empty section '## 1.0.0'" -- 1.0.0

new_case notes_heading_only_at_end_fails
changelog "## 0.9.0" "- a" "## 1.0.0"
expect release-notes.sh 1 "" "release-notes: CHANGELOG.md has no non-empty section '## 1.0.0'" -- 1.0.0

new_case notes_unreleased_heading_is_not_the_version
changelog "## Unreleased" "- later" "## 0.9.0" "- a"
expect release-notes.sh 1 "" "release-notes: CHANGELOG.md has no non-empty section '## 1.0.0'" -- 1.0.0

new_case notes_explicit_file
printf '## 2.0.0\n- from other\n' > other.md
expect release-notes.sh 0 '- from other' "" -- 2.0.0 other.md

new_case notes_unreadable_file
expect release-notes.sh 2 "" "release-notes: nope.md: cannot read the changelog" -- 1.0.0 nope.md

new_case notes_bad_version
changelog "## 1.0.0" "- a"
expect release-notes.sh 2 "" "release-notes: version 'v1.0.0' is not X.Y.Z" -- v1.0.0
expect release-notes.sh 2 "" "release-notes: version '1.0' is not X.Y.Z" -- 1.0

new_case notes_no_args
expect release-notes.sh 2 "" "usage: release-notes.sh <X.Y.Z> [changelog]" --

# --- crate-published.sh -------------------------------------------------------------------

new_case crate_200_published_and_request_shape
export FAKE_STATUS=200 FAKE_BODY='{"version":{"crate":"minuta","num":"0.9.0","yanked":false}}'
expect crate-published.sh 0 published "" -- minuta 0.9.0
want=$'--silent\n--show-error\n--location\n--max-time\n30\n--user-agent\nminuta-release (https://github.com/JCIOriente/minuta)\n--output'
if [ "$(sed -n '1,8p' "$case_dir/args")" != "$want" ] \
  || [ "$(sed -n '10,11p' "$case_dir/args")" != $'--write-out\n%{http_code}' ] \
  || [ "$(tail -n 1 "$case_dir/args")" != "https://crates.io/api/v1/crates/minuta/0.9.0" ] \
  || [ "$(wc -l < "$case_dir/args" | tr -d ' ')" != 12 ]; then
  failures=$((failures + 1)); echo "FAIL $case_name: unexpected curl arguments"; cat "$case_dir/args"
fi
cases=$((cases + 1))

new_case crate_200_yanked_counts_as_published
export FAKE_STATUS=200 FAKE_BODY='{"version":{"num":"0.9.0","yanked":true}}'
expect crate-published.sh 0 published "" -- minuta 0.9.0

new_case crate_404_not_published
export FAKE_STATUS=404 FAKE_BODY='{"errors":[{"detail":"Not Found"}]}'
expect crate-published.sh 1 "not published" "" -- minuta 9.9.9

new_case crate_200_for_other_version_is_an_error
export FAKE_STATUS=200 FAKE_BODY='{"version":{"num":"0.9.01"}}'
expect crate-published.sh 2 "" "crate-published: crates.io answered 200 without version 0.9.0" -- minuta 0.9.0

new_case crate_200_dots_are_literal
export FAKE_STATUS=200 FAKE_BODY='{"version":{"num":"0x9x0"}}'
expect crate-published.sh 2 "" "crate-published: crates.io answered 200 without version 0.9.0" -- minuta 0.9.0

new_case crate_403_is_an_error
export FAKE_STATUS=403 FAKE_BODY='forbidden'
expect crate-published.sh 2 "" "crate-published: crates.io answered HTTP 403 for minuta 0.9.0" -- minuta 0.9.0

new_case crate_503_is_an_error
export FAKE_STATUS=503
expect crate-published.sh 2 "" "crate-published: crates.io answered HTTP 503 for minuta 0.9.0" -- minuta 0.9.0

new_case crate_request_failure_is_an_error
export FAKE_FAIL=1
expect crate-published.sh 2 "" "crate-published: request to crates.io failed" -- minuta 0.9.0

new_case crate_bad_arguments
expect crate-published.sh 2 "" "crate-published: version '1.0' is not X.Y.Z" -- minuta 1.0
expect crate-published.sh 2 "" "crate-published: crate name 'a/b' is not valid" -- a/b 1.0.0
expect crate-published.sh 2 "" "usage: crate-published.sh <crate> <X.Y.Z>" -- minuta

echo "release-scripts: $cases runs, $failures failed"
[ "$failures" -eq 0 ]
