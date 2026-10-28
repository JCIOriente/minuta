# Core pipeline (sub-project 1 of 6) — Spec

- Status: approved
- Date: 2026-10-23
- Owner: Arnold Gandarillas Castillo
- Related: docs/brief.md, ADR-0001, ADR-0002, ADR-0003

## TL;DR
- `minuta run <files…>` turns one or more recordings into `transcript.json`, `transcript.txt`
  and `captions.srt` in a job folder, entirely locally (ffmpeg + whisper.cpp).
- Reruns reuse finished steps; a step reruns only when its fingerprint changes or `--force`
  asks for it.
- The CLI uses subcommands (`run`, `transcribe`, `doctor`, `models`, `config`), not the
  brief's `-cy` shorthand (ADR-0001).
- This is the first of six sub-projects. Templates, LLM backends, chapters, edit plan and
  publishing are specified separately.

## Change in a nutshell
```
$ minuta run clase-4a.mp4 clase-4b.mp4
[1/5] probe       0.3s
[2/5] join        stream copy   12.4s
[3/5] audio       4.1s
[4/5] transcribe  language=es  412 segments  2m31s
[5/5] outputs     0.1s
minuta-out/clase-4a/
  job.json  joined.mp4  audio.wav  transcript.json  transcript.txt  captions.srt
```
Running the same command again prints each step as `cached` and finishes in under a second.
`--force transcribe` redoes `transcribe` and `outputs`. The result today is a bash script
that only produces lesson Markdown; after this change the transcript is a first-class,
timestamped, reusable artifact that every later sub-project builds on.

## Problem
The existing `orientate-clases` pipeline is a hard-coded bash script. Its transcript has no
timestamps, a rerun trusts any existing file (so stale output survives changed settings), and
a failed step can leave partial files. Every later feature (chapters, captions, templates,
publishing) needs a reliable, timestamped transcript and a job folder with trustworthy
caching.

## Goals / Success criteria
1. `minuta run a.mp4 b.mp4` produces `joined.<ext>`, `audio.wav`, `transcript.json`,
   `transcript.txt` and `captions.srt` in `./minuta-out/a/`.
2. `transcript.json` contains `language` and segments with `start`, `end` (seconds) and
   `text`; segments are ascending and non-overlapping (checked by a test).
3. A second identical run executes zero steps and reports each as `cached`.
4. Changing `--lang`, `--model`, or a source file makes `transcribe` (or the earliest affected
   step) and all later steps stale and rerun them; the reason is recorded in `job.json`.
5. Killing the process during any step and rerunning completes the job with correct output;
   no output file exists for an unfinished step.
6. Missing `ffmpeg`, `ffprobe`, `whisper-cli` or the model is reported all at once, before any
   work starts, with an install hint for each.
7. `-n clase-4.mp4` and file `clase-4.mp4` both yield job name `clase-4`.
8. Works on macOS and Linux; CI runs the test suite on both.

## Non-goals
- Templates, LLM backends, chunking, chapters (sub-projects 2-4).
- Edit plan, rendering cuts, caption remapping (5). Captions here come straight from
  segments.
- YouTube or any publisher (6).
- Windows, packaging beyond `cargo install`, GUI, server.
- Speaker diarization, translation, language detection beyond what whisper reports.

## Users & permissions
| Role | Can | Cannot |
|---|---|---|
| Local user | Run every command on files they can read; write to the output dir and `~/.config/minuta`, `~/.cache/minuta` | n/a: single-user tool with no accounts |

Security notes:
- External tools are run with an argv array, never through a shell, so file names cannot
  inject commands.
- Model downloads are verified against a SHA-256 pinned in minuta, written to a temp file and
  renamed only on success.
- Nothing leaves the machine in this sub-project. Recordings may be sensitive, so minuta adds
  no network calls other than `models pull`.

## Tenancy & localization
- Tenancy: n/a (single local user).
- Speech language: `auto` by default, or any whisper language code. The detected language is
  stored in `transcript.json`. Nothing assumes Spanish.
- Time zones: n/a. Timestamps are seconds from the start of the joined media.
- SRT timestamps use `HH:MM:SS,mmm`. Text is written as UTF-8 and never transliterated.
- minuta's own messages are English only.

## Domain model
See `CONTEXT.md` for terms.
- **Config** (`~/.config/minuta/config.toml`, overridable with `MINUTA_CONFIG`). Keys:
  `out_dir` (default `./minuta-out`), `whisper.bin`, `whisper.model`, `whisper.lang`
  (default `auto`), `ffmpeg.bin`, `ffprobe.bin`. Precedence: flags > `MINUTA_*` env >
  config > defaults.
- **Job** `./minuta-out/<name>/job.json`:
  ```json
  { "version": 1, "name": "clase-4", "sources": [{"path": "…", "size": 123, "mtime": 1}],
    "steps": { "probe": {"status": "done", "fingerprint": "…", "seconds": 0.3, "reason": null} } }
  ```
- **Steps**, in order: `probe` (ffprobe all sources), `join`, `audio` (mono 16 kHz WAV),
  `transcribe` (whisper-cli with `--vad`, JSON parsed into `Transcript`), `outputs` (txt and
  srt rendered from the transcript).
- **Transcript** `{ "language": "es", "segments": [{"start": 0.0, "end": 4.2, "text": "…"}] }`.
- Invariants:
  - `status: done` if and only if all of the step's artifacts exist.
  - A step's fingerprint covers its inputs, its parameters and the version string of the tool
    it runs, plus the fingerprints of earlier steps it depends on.
  - Job names never end in a media extension.

## Interfaces
```
minuta run <files…> [--name N] [--out DIR] [--lang auto|xx] [--model PATH|NAME]
                    [--force [STEP]] [--quiet]
minuta transcribe <files…>        # same flags; identical to run in this sub-project
minuta doctor                     # report tools, versions, model; exit 1 if anything missing
minuta models list | pull <name>  # names: tiny base small medium large-v3 (ggml, Hugging Face)
minuta config path | show
```
- Exit codes: 0 success, 1 failure, 2 usage error, 130 interrupted.
- Progress goes to stderr (step counter, per-step timing, live ffmpeg and whisper progress).
  The job folder path goes to stdout, one line, so `cd $(minuta run …)` works.
- Color is disabled by `NO_COLOR` or when stderr is not a terminal.

## Behavior & edge cases
- **Preflight:** before the first step, check all required tools and the model. Report every
  missing item in one message with a hint (for example `brew install ffmpeg whisper-cpp`,
  `minuta models pull small`).
- **Name:** `--name`, else the first source's file stem with media extensions stripped. If
  `<out>/<name>` exists with a different source set, fail unless `--name` or `--force` is
  given.
- **Join:** with one source, `joined.<ext>` is a copy (hard link when possible). With several,
  use stream copy when codec, resolution and frame rate match, else re-encode (VideoToolbox
  on macOS when available, else libx264). Inputs are joined in the order given.
- **Audio:** a source with no audio stream fails at `probe`: `no audio stream in <file>`.
- **Transcribe:** whisper runs with `--vad`. Zero segments is a warning, and empty outputs are
  still written.
- **Atomic writes:** each artifact is written to `<name>.tmp` in the job folder and renamed.
  Leftover `.tmp` files are deleted at the start of a run.
- **Staleness:** a step is stale if its recorded fingerprint differs from the current one or
  an artifact is missing. A stale step makes all later steps stale. The reason is stored in
  `steps.<name>.reason`. Source files are compared by size and mtime first, then content hash.
- **`--force`:** with no value, every step is stale. With a step name, that step and all later
  ones are stale.
- **Interrupt:** Ctrl-C kills the child process, removes temp files, exits 130, and marks no
  step done.
- **Unknown `job.json` version:** fail with a message naming the version; never migrate
  silently.
- **Disk full or write error:** fail the step with the OS error; the previous good artifacts
  are kept.

## Data & migrations
New project, no existing data. `job.json` carries `version: 1` so later versions can detect
old folders (ADR-0003). The bash pipeline's job folders are not read or migrated.

## Testing strategy
- **Unit:** name derivation (including `clase-4.mp4`), fingerprint stability and sensitivity,
  staleness propagation, whisper JSON parsing into `Transcript`, segment-order invariant,
  SRT and TXT rendering, config precedence.
- **Integration** (tiny `ffmpeg lavfi` fixtures and a fake `whisper-cli` script on `PATH`):
  stream-copy join versus re-encode, single-source join, no-audio source error, full run,
  second run all cached, change of `--lang` reruns `transcribe` only and later steps, a
  killed step resumed, preflight listing several missing tools at once, name-conflict error.
- **Opt-in e2e:** `MINUTA_E2E=1` runs real whisper with the `tiny` model on a short clip.
- **Security:** a file name containing spaces, quotes and `$(…)` runs correctly and executes
  nothing; a model with a wrong checksum is rejected and not left in the cache.
- CI runs `make ci` on macOS and Linux.

## Decisions
- ADR-0001: Rust CLI with subcommands.
- ADR-0002: whisper.cpp as a subprocess.
- ADR-0003: per-step fingerprint cache in `job.json`.
- New terms in `CONTEXT.md`: Source, Transcript, Job, Step, Artifact, Fingerprint, Stale,
  Speech language.

## FAQ
### Why not build the generic template DAG now?
This sub-project is a straight line of five steps. A DAG engine would be speculative; sub-project
2 adds branching and can generalize the step runner then.

### Why does `minuta transcribe` exist if it equals `run`?
In sub-project 2 `run` gains `-t <template>` and produces documents; `transcribe` stays the
transcript-only command. Defining it now keeps scripts stable.

### Why download models in minuta rather than ask the user?
A first run on a new machine works after one `minuta models pull small`. The cost is network
and checksum code, limited to one module.

## Open questions
None at approval time.

## Implementation status
Plan: docs/plans/2026-10-23-core-pipeline.md. All tasks merged; deviations and follow-ups are in issue #22.
- [x] T1 crate skeleton, error type and CI (#1, PR #14)
- [x] T2 config loading and precedence (#5, PR #17)
- [x] T3 transcript schema, whisper parsing, TXT and SRT (#6, PR #15)
- [x] T4 job state, naming, fingerprints and staleness (#7, PR #19)
- [x] T5 process runner, toolchain preflight, doctor data (#8, PR #18)
- [x] T6 model catalog and download (#9, PR #16)
- [x] T7 media steps: probe, join, audio (#10, PR #21)
- [x] T8 transcribe and outputs steps (#11, PR #20)
- [x] T9 CLI, runner, progress and interrupt (#12, PR #23)
- [x] T10 integration suite, fixtures and opt-in e2e (#13, PR #24)

Known deviations from this spec (decided during implementation):
- Name conflicts: only `--force` replaces an existing job; `--name` picks another name.
- VAD is off until a VAD model is configured with `whisper.vad_model` (`MINUTA_WHISPER_VAD_MODEL`), a path or the catalog name `silero-v5.1.2` (`minuta models pull silero-v5.1.2`), because `whisper-cli --vad` needs `--vad-model` (#110).
- An explicit `MINUTA_CONFIG` pointing at a missing file is an error; unknown config keys are rejected.
- `derive_name` returns `Result<String>`; steps expose `run_with(ctx, RunOpts, on_line)` for interrupt and progress.
