#!/usr/bin/env bash
# Tests for check-file-size.sh. Each case builds a fixture tree in a temporary
# directory, runs the check there, and asserts the exit code and the exact stderr.
# Every case runs under each bash found (the one on PATH and /bin/bash, which is
# bash 3.2 on macOS), because the check must work on both.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
check="$here/check-file-size.sh"
root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT

shells=$(command -v bash)
if [ -x /bin/bash ] && [ "$shells" != /bin/bash ]; then
  shells="$shells /bin/bash"
fi

failures=0
cases=0

# new_case <name>: start a fresh fixture tree with an empty src/ and enter it.
new_case() {
  case_name=$1
  case_dir="$root/$1"
  mkdir -p "$case_dir/src"
  cd "$case_dir"
}

# lines <path> <n>: write a file of n lines, each ending in a newline.
lines() {
  mkdir -p "$(dirname "$1")"
  awk -v n="$2" 'BEGIN { for (i = 1; i <= n; i++) print "x" }' > "$1"
}

# allow <line>...: write the allowlist, one argument per line.
allow() {
  printf '%s\n' "$@" > allow.txt
}

# expect <exit code> [<stderr line>...]: run the check in the current fixture with
# FILE_SIZE_SRC=${src_arg:-src} and compare. Stdout must stay empty.
expect() {
  local code=$1 expected actual out got shell
  shift
  expected=$(printf '%s\n' "$@")
  for shell in $shells; do
    cases=$((cases + 1))
    got=0
    out=$(FILE_SIZE_SRC=${src_arg:-src} FILE_SIZE_ALLOWLIST=allow.txt \
      "$shell" "$check" 2> "$case_dir/stderr") || got=$?
    actual=$(cat "$case_dir/stderr")
    if [ "$got" = "$code" ] && [ "$actual" = "$expected" ] && [ -z "$out" ]; then
      echo "ok   $case_name ($shell)"
    else
      failures=$((failures + 1))
      echo "FAIL $case_name ($shell)"
      echo "  exit: expected $code, got $got"
      echo "  stderr expected:"; printf '%s\n' "$expected" | sed 's/^/    /'
      echo "  stderr actual:"; printf '%s\n' "$actual" | sed 's/^/    /'
      [ -z "$out" ] || { echo "  stdout (expected none):"; printf '%s\n' "$out" | sed 's/^/    /'; }
    fi
  done
  src_arg=""
}

header='# path max lines'
src_arg=""

new_case exactly_500_passes
lines src/a.rs 500
allow "$header"
expect 0

new_case over_501_fails
lines src/a.rs 501
allow "$header"
expect 1 "src/a.rs: 501 lines, limit 500; split it into modules" "file-size: 1 violation"

new_case no_trailing_newline_counts
lines src/a.rs 500
printf 'x' >> src/a.rs
allow "$header"
expect 1 "src/a.rs: 501 lines, limit 500; split it into modules" "file-size: 1 violation"

new_case listed_grew_fails
lines src/a.rs 601
allow "src/a.rs 600"
expect 1 "src/a.rs: 601 lines, allowlist allows 600; it may only shrink" "file-size: 1 violation"

new_case listed_at_its_count_passes
lines src/a.rs 600
allow "src/a.rs 600"
expect 0

new_case listed_shrank_over_limit_passes
lines src/a.rs 550
allow "src/a.rs 600"
expect 0

new_case listed_within_limit_fails
lines src/a.rs 500
allow "src/a.rs 600"
expect 1 "src/a.rs: 500 lines, within the limit; remove it from the allowlist" \
  "file-size: 1 violation"

new_case missing_entry_fails
allow "src/gone.rs 600"
expect 1 "src/gone.rs: listed in the allowlist but does not exist" "file-size: 1 violation"

new_case two_offenders_both_reported
lines src/b.rs 501
lines src/a.rs 502
allow "$header"
expect 1 "src/a.rs: 502 lines, limit 500; split it into modules" \
  "src/b.rs: 501 lines, limit 500; split it into modules" "file-size: 2 violations"

new_case each_rule_reported_in_one_run
lines src/a.rs 501
lines src/b.rs 701
lines src/c.rs 400
allow "src/b.rs 700" "src/c.rs 600" "src/gone.rs 600"
expect 1 "src/a.rs: 501 lines, limit 500; split it into modules" \
  "src/b.rs: 701 lines, allowlist allows 700; it may only shrink" \
  "src/c.rs: 400 lines, within the limit; remove it from the allowlist" \
  "src/gone.rs: listed in the allowlist but does not exist" "file-size: 4 violations"

new_case nested_rs_counted
lines src/llm/deep/x.rs 501
allow "$header"
expect 1 "src/llm/deep/x.rs: 501 lines, limit 500; split it into modules" "file-size: 1 violation"

new_case non_rs_ignored
lines src/notes.md 900
lines src/a.rs.bak 900
allow "$header"
expect 0

new_case nested_src_dir
lines src/llm/a.rs 501
lines src/b.rs 900
allow "src/llm/a.rs 600"
src_arg=src/llm
expect 0

new_case header_only_allowlist_passes
lines src/a.rs 10
allow "$header"
expect 0

new_case empty_allowlist_passes
lines src/a.rs 10
: > allow.txt
expect 0

new_case comments_and_blanks_ignored
lines src/a.rs 550
allow "# a comment" "" "   " "src/a.rs 600" "#src/b.rs 700"
expect 0

new_case tabs_separate_fields
lines src/a.rs 550
lines src/b.rs 550
allow "$(printf 'src/a.rs\t600')" "src/b.rs     600"
expect 0

new_case last_entry_without_newline_read
lines src/a.rs 601
printf 'src/a.rs 600' > allow.txt
expect 1 "src/a.rs: 601 lines, allowlist allows 600; it may only shrink" "file-size: 1 violation"

new_case missing_allowlist_errors
expect 2 "file-size: allow.txt: cannot read the allowlist"

new_case missing_src_errors
rmdir src
allow "$header"
expect 2 "file-size: src: not a directory"

new_case malformed_field_count_one_errors
allow "$header" "src/a.rs"
expect 2 "file-size: allow.txt:2: expected '<path> <max lines>'"

new_case malformed_field_count_three_errors
allow "src/a.rs 600 extra"
expect 2 "file-size: allow.txt:1: expected '<path> <max lines>'"

new_case malformed_count_not_a_number_errors
allow "src/a.rs 6x0"
expect 2 "file-size: allow.txt:1: max lines must be a positive integer (got '6x0')"

new_case malformed_count_leading_zero_errors
allow "src/a.rs 0600"
expect 2 "file-size: allow.txt:1: max lines must be a positive integer (got '0600')"

new_case malformed_count_within_limit_errors
allow "src/a.rs 500"
expect 2 "file-size: allow.txt:1: max lines 500 is within the limit (500); remove the entry"

new_case malformed_path_not_rs_errors
allow "src/a.md 600"
expect 2 "file-size: allow.txt:1: src/a.md is not a .rs file"

new_case malformed_path_outside_src_errors
allow "lib/a.rs 600"
expect 2 "file-size: allow.txt:1: lib/a.rs is not under src/"

new_case malformed_duplicate_path_errors
lines src/a.rs 550
allow "src/a.rs 600" "src/a.rs 700"
expect 2 "file-size: allow.txt:2: src/a.rs is listed twice"

new_case bad_src_form_errors_dot_slash
allow "$header"
src_arg=./src
expect 2 "file-size: FILE_SIZE_SRC must be a relative path without a leading ./ or a trailing / (got './src')"

new_case bad_src_form_errors_trailing_slash
allow "$header"
src_arg=src/
expect 2 "file-size: FILE_SIZE_SRC must be a relative path without a leading ./ or a trailing / (got 'src/')"

new_case bad_src_form_errors_absolute
allow "$header"
src_arg="$case_dir/src"
expect 2 "file-size: FILE_SIZE_SRC must be a relative path without a leading ./ or a trailing / (got '$case_dir/src')"

echo "check-file-size: $cases runs, $failures failed"
[ "$failures" -eq 0 ]
