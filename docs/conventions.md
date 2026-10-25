# minuta conventions

Project rules that override the baton guides. `baton:retro` appends lessons here.

- Local validation and CI both run `make ci`; change checks there, not in two places.

## Rust and tests
- Local toolchain is pinned to `stable` (`rust-toolchain.toml`) so `make ci` locally runs the same clippy as CI. The pin does not update an installed `stable`: run `rustup update stable` before starting a batch of tasks, and first thing after a lint that fails only in CI. Why: a lint added in newer clippy (`cloned_ref_to_slice_refs`) passed locally on 1.80 and failed CI on PR #19; `needless_borrows_for_generic_args` passed locally on 1.95 and failed CI (1.99) on PR #75.
- Never test by writing a script and executing it from a parallel test thread without protection. Prefer a committed, already-executable fixture (`tests/fixtures/bin/whisper-cli`); otherwise hold a shared mutex across write and spawn, or retry on `ETXTBSY`. Why: Linux can fail `exec` of a file another thread's fork still holds open for writing (hit in T5 and T10).
- Do not assert the relative order of lines from two different streams (stdout vs stderr). They are read by separate threads. Assert per stream, or on set membership.
- Never use a sleep to order threads or to wait for a state, and no tight wall-clock bounds in tests; poll observable state under a generous deadline instead (see "Timing assertions" in docs/common-issues.md). Why: tests in #44 and #49 flaked or would have flaked on shared CI runners.
- External tools run only through `tools::run` (or `tools::run_capture`) with an argv array, never through a shell. Tests that spawn tools must be hermetic: temp `HOME`/XDG, explicit `MINUTA_CONFIG`, `env_clear()` for anything that must reach the real binary.

## Security
- Treat `./templates`, template frontmatter, upstream Markdown and every LLM or server response as untrusted. Sanitize control characters before printing (all of `char::is_control`: C0, DEL and C1, keeping only what the format needs such as `\n`/`\t`), confine symlinks in the project layer, validate identifiers with an allowlist. Why: reviewers found each of these as a real attack path in sub-project 2 (#45, #46, #51); see docs/common-issues.md.

## Errors and I/O
- Never turn an I/O error into "absent" or a default: no `Path::exists()` for decisions (use `try_exists()`), no `.ok()?` or `unwrap_or_default()` on reads, no lossy `to_str()` on paths or env values. Surface the error with the path in the message. Why: this pattern was flagged in five different modules (T2, T4, T5, T6, T9) and each time hid a real failure, and again in sub-project 2 (config, job, call cache, template discovery).
- A step's output counts as done only after it is fully written: write to `<name>.tmp`, `sync_all`, rename (`job::write_atomic`). ffmpeg needs an explicit `-f` for `.tmp` outputs.
- Every step must be re-runnable: its `params` include the tool version and every input that can change its result (source list, model identity, language), so a change makes it stale.

## Process
- Tests for a table of cases (per-key config, per-name env var, per-extension muxer) need one assertion per case, not one representative case. Why: the tests reviewer returned CHANGES_REQUIRED for this on T2, T3 and T9; a swapped key or dropped name passed every test.
- Assert exact values: literal wire names, whole messages, whole prompt strings. A test that only checks a prefix, a substring, a bare keyword, uniqueness or a round trip passes with the wrong value. Tests that pin behavior that already exists show their red run by temporarily mutating the value under test. Why: in sub-project 3 reviewers found this four times: field names checked only by round trip (#69), a parser message checked by prefix (#74), a repair prompt without its separators pinned (#72), and prompt requirements checked by bare words that would also match "Include links" (#76).
- Implementers report the failing test run before the fix (the red output), not just "tests pass". Why: in sub-project 2, half of the implementers reported writing tests and code together without seeing the tests fail, and reviewers later found tests that did not cover what they claimed (#49 concurrency limit, #46 `raw` field).
- A review fix that tightens a constraint (a time bound, an allowlist, a stricter writer) needs a test with real-world values before it is accepted. Why: three controller-requested fixes regressed in sub-project 2: a 100 ms bound flaked on CI (#48), a model-name allowlist rejected real Claude names like `opus[1m]` (#45), and inlining an atomic write dropped the directory fsync (#44).
- Plan interfaces between parallel tasks must name the hooks later tasks need up front. Steps need `run_with(ctx, RunOpts, on_line)` for interrupt and progress; T7 had to be reworked after review because the plan only had `Step::run`.
