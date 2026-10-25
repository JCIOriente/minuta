# minuta conventions

Project rules that override the baton guides. `baton:retro` appends lessons here.

- Local validation and CI both run `make ci`; change checks there, not in two places.

## Rust and tests
- Local toolchain is pinned to `stable` (`rust-toolchain.toml`) so `make ci` locally runs the same clippy as CI. Why: a lint added in newer clippy (`cloned_ref_to_slice_refs`) passed locally on 1.80 and failed CI on PR #19.
- Never test by writing a script and executing it from a parallel test thread without protection. Prefer a committed, already-executable fixture (`tests/fixtures/bin/whisper-cli`); otherwise hold a shared mutex across write and spawn, or retry on `ETXTBSY`. Why: Linux can fail `exec` of a file another thread's fork still holds open for writing (hit in T5 and T10).
- Do not assert the relative order of lines from two different streams (stdout vs stderr). They are read by separate threads. Assert per stream, or on set membership.
- External tools run only through `tools::run` with an argv array, never through a shell. Tests that spawn tools must be hermetic: temp `HOME`/XDG, explicit `MINUTA_CONFIG`, `env_clear()` for anything that must reach the real binary.

## Errors and I/O
- Never turn an I/O error into "absent" or a default: no `Path::exists()` for decisions (use `try_exists()`), no `.ok()?` or `unwrap_or_default()` on reads, no lossy `to_str()` on paths or env values. Surface the error with the path in the message. Why: this pattern was flagged in five different modules (T2, T4, T5, T6, T9) and each time hid a real failure.
- A step's output counts as done only after it is fully written: write to `<name>.tmp`, `sync_all`, rename (`job::write_atomic`). ffmpeg needs an explicit `-f` for `.tmp` outputs.
- Every step must be re-runnable: its `params` include the tool version and every input that can change its result (source list, model identity, language), so a change makes it stale.

## Process
- Tests for a table of cases (per-key config, per-name env var, per-extension muxer) need one assertion per case, not one representative case. Why: the tests reviewer returned CHANGES_REQUIRED for this on T2, T3 and T9; a swapped key or dropped name passed every test.
- Plan interfaces between parallel tasks must name the hooks later tasks need up front. Steps need `run_with(ctx, RunOpts, on_line)` for interrupt and progress; T7 had to be reworked after review because the plan only had `Step::run`.
