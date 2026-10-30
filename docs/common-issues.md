# Common issues

## Relative output directory doubles artifact paths
- Symptom: every step fails with "finished without writing" when run with the default `./minuta-out`, while tests pass.
- Cause: `Ctx.dir` was relative and artifact paths already carried that prefix, so joining them onto the job dir doubled it. Tests used absolute `TempDir` paths.
- Avoid: canonicalize the job dir in `Job::open`; add a test that uses a relative out dir.

## Tool installed by a package manager is newer or older than the code assumes
- Symptom: passes locally, fails in CI (or the reverse) with lints or flags you have never seen.
- Cause: different toolchain or tool versions. Check `rust-toolchain.toml` and `minuta doctor` output first.
- Avoid: pin the toolchain; record tool versions in step `params`.

## `whisper-cli --vad` does nothing useful without a VAD model
- Symptom: VAD "enabled" but hallucinated lines remain, or whisper errors about a missing VAD model.
- Cause: `--vad` needs `--vad-model <path>` (a Silero ggml model); there is no default.
- Avoid: only pass `--vad` together with `-vm`; tracked in issue #22.

## Timing assertions that pass locally and flake on CI
- Symptom: a test fails on a GitHub runner with "assertion failed: elapsed < 100ms" (or a concurrency peak of 1 instead of N) and passes on rerun.
- Cause: shared runners pause threads for tens to hundreds of milliseconds; fixed sleeps and tight wall-clock bounds assume a quiet machine. Seen in #44 (100 ms interrupt bound, fixed in #48) and #49 (sleep-based concurrency and first-error tests).
- Also seen: the ratio test `huge_single_line_splits_quickly` (threshold 8x) failed once while several cargo builds ran on the same machine during sub-project 3, then passed. A ratio is safer than an absolute bound but still not immune to a loaded machine. Fixed in #91 by comparing 64x the input (linear gives about 64, quadratic about 4000) against a limit of 1024, over 10x the highest value measured with twice as many busy loops as cores, and by running each split on a worker thread under one 30 s deadline so a regression fails instead of hanging.
- Avoid: wait on observable state with a generous deadline (10–30 s) that panics with a clear message; order threads with gates or condvars, not sleeps; for "is it fast enough", compare a ratio over a large size multiple (for example t(64N)/t(N)) instead of an absolute time, so that a limit with 10x headroom over linear still stays well below quadratic, and run the timed work under one generous deadline so a regression fails instead of hanging (#91); keep any absolute bound at 10x the expected value.

## Untrusted text from `./templates` or a server reaches prompts or the terminal
- Symptom: a cloned repo's `./templates` pulls a local file into a prompt, or prints escape sequences that rewrite the terminal.
- Cause: the project layer is attacker-controlled. Seen as symlink escapes (`templates/x.md -> ~/.ssh/id_rsa`, then `templates -> .` with `summary.md -> .env`, #46), raw control characters in `templates list` and in error messages (#51), and unvalidated model names echoed in hints (#45).
- Avoid: in the project layer, refuse a symlinked templates dir and require each file's canonical target inside it; pass every untrusted string printed to a terminal through the CLI's `sanitize` (errors go through `render_error`, which does this); validate identifiers with an allowlist and print rejected input with `{:?}`.

## Config or templates ignored after upgrade
- Symptom: `minuta config show` reports every value as `(default)` after upgrading, or a custom template no longer appears in `templates list`.
- Cause: the config file and user templates moved from `~/.config/minuta/` to `~/.minuta/` (#149, #150). Nothing is migrated, so the old files are ignored.
- Avoid: move them once; the README "Upgrading" note has the commands. `minuta config path` shows where the config file is read from.

## A test reaches a real local service
- Symptom: tests pass, but `cargo test` sends requests to the developer's real Ollama (or another daemon on a default port).
- Cause: a new code path reads a default URL from config that the existing test environment never overrode (`doctor` started probing `127.0.0.1:11434` in #51). A "closed port" helper that binds port 0 and drops the listener can also hand that port to a parallel fake server.
- Avoid: every test environment sets every service URL explicitly (`MINUTA_OLLAMA_URL`), to a fake or to a never-served address like `http://127.0.0.1:9`; never use bind-then-drop to find a closed port.

## A write into a folder from an untrusted clone follows a planted symlink
- Symptom: a command that writes into `./templates` (or any folder a cloned repo can fill) overwrites a file outside it, for example `~/.zshrc`.
- Cause: the temp file next to the target is written with `fs::write(<file>.tmp)`, which follows a symlink an attacker committed at that name. `job::write_atomic` only renames; what its callback writes to the temp path decides. Job folders are hardened (#175): `job::write_atomic` removes whatever sits at `<file>.tmp` before the step writes (`remove_file` removes a symlink itself and a failure to remove is an error naming the path; no `create_new`, because ffmpeg creates its own output), and `clean_tmp` removes symlinks and other non-directory `*.tmp` entries without following them. A step writer that writes a temp file on its own instead of going through `write_atomic` would bring the hole back. The one writer outside `write_atomic` is the LLM call cache (`src/llm/call.rs`), whose temp name is unique per process and call (`<key>.<pid>-<n>.tmp`) and which is cleared at the start of a run, so there is no fixed name to plant. It is not hardened against a symlinked `llm-cache` directory itself. `./templates` is exposed by design.
- Avoid: in such folders, remove a stale temp file first, create the new one with `create_new` (O_EXCL), then rename; refuse a symlinked folder, manifest or target. `write_file` in `src/template/pull.rs` does this, and `tests/pull_core.rs` plants a symlink at `<file>.tmp`, `.pulled.toml.tmp` and `<file>.bak.tmp` and checks a victim file is untouched.
