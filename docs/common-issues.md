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
- Also seen: the ratio test `huge_single_line_splits_quickly` (threshold 8x) failed once while several cargo builds ran on the same machine during sub-project 3, then passed. A ratio is safer than an absolute bound but still not immune to a loaded machine. Fixed in #91 by comparing 64x the input (linear gives about 64, quadratic about 4000) against a limit of 1024, over 10x the highest value measured with twice as many busy loops as cores.
- Avoid: wait on observable state with a generous deadline (10–30 s) that panics with a clear message; order threads with gates or condvars, not sleeps; for "is it fast enough", compare a ratio (t(4N)/t(N)) instead of an absolute time; keep any absolute bound at 10x the expected value.

## Untrusted text from `./templates` or a server reaches prompts or the terminal
- Symptom: a cloned repo's `./templates` pulls a local file into a prompt, or prints escape sequences that rewrite the terminal.
- Cause: the project layer is attacker-controlled. Seen as symlink escapes (`templates/x.md -> ~/.ssh/id_rsa`, then `templates -> .` with `summary.md -> .env`, #46), raw control characters in `templates list` and in error messages (#51), and unvalidated model names echoed in hints (#45).
- Avoid: in the project layer, refuse a symlinked templates dir and require each file's canonical target inside it; pass every untrusted string printed to a terminal through the CLI's `sanitize` (errors go through `render_error`, which does this); validate identifiers with an allowlist and print rejected input with `{:?}`.

## A test reaches a real local service
- Symptom: tests pass, but `cargo test` sends requests to the developer's real Ollama (or another daemon on a default port).
- Cause: a new code path reads a default URL from config that the existing test environment never overrode (`doctor` started probing `127.0.0.1:11434` in #51). A "closed port" helper that binds port 0 and drops the listener can also hand that port to a parallel fake server.
- Avoid: every test environment sets every service URL explicitly (`MINUTA_OLLAMA_URL`), to a fake or to a never-served address like `http://127.0.0.1:9`; never use bind-then-drop to find a closed port.
