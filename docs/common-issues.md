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
