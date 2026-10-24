# Core pipeline — Implementation Plan

- Spec: docs/specs/2026-10-23-core-pipeline.md
- Date: 2026-10-23
- Status: approved

## Overview
One Rust crate (`minuta`, library plus binary). The plan cuts it by module so tasks run in
parallel without sharing files. T1 creates the crate with every module file already declared
and empty, so later tasks only fill in their own files and never edit `lib.rs`, `Cargo.toml`
dependencies (T1 adds them all up front) or each other's modules.

Shared interfaces, fixed here so parallel tasks agree. Signatures may gain private helpers;
the public shape does not change without updating this plan.

```rust
// error.rs (T1)
pub enum Error { Usage(String), MissingTools(Vec<Missing>), Io(..), Tool{tool,code,stderr}, Parse(..), Config(..), Job(..), Interrupted }
pub type Result<T> = std::result::Result<T, Error>;

// tools.rs (T5)
pub fn run(argv: &[OsString], opts: RunOpts, on_line: &mut dyn FnMut(&str)) -> Result<Output>; // no shell
pub struct Toolchain { ffmpeg, ffprobe, whisper }          // resolved paths + version strings
pub fn preflight(cfg: &Config, need_model: bool) -> Result<Toolchain>; // lists ALL missing at once

// transcript.rs (T3)
pub struct Segment { start: f64, end: f64, text: String }
pub struct Transcript { language: String, segments: Vec<Segment> }
pub fn from_whisper_json(&str) -> Result<Transcript>; pub fn to_txt(&Transcript) -> String; pub fn to_srt(&Transcript) -> String;

// job/ (T4)
pub struct Job { dir, state }              // load/create, name derivation, job.json read/write
pub fn derive_name(first: &Path) -> String;
pub fn fingerprint(parts: &[&[u8]]) -> String;
pub fn write_atomic(path: &Path, f: impl FnOnce(&Path) -> Result<()>) -> Result<()>;
pub trait Step { const NAME: StepName; fn params(&self, ctx:&Ctx)->Vec<u8>; fn outputs(&self, ctx:&Ctx)->Vec<PathBuf>; fn run(&self, ctx:&Ctx)->Result<()>; }
pub fn plan_stale(job:&Job, current:&[(StepName, String)], force:Option<Force>) -> Vec<(StepName, Reason)>;
```

## Contract
none (no HTTP API). The public contracts are the CLI in the spec's Interfaces section and
the `job.json` and `transcript.json` schemas. T4 owns `job.json` (version 1), T3 owns
`transcript.json`.

## Impact
Greenfield; no existing consumers. The one hard-to-reverse surface is the `job.json` and
`transcript.json` formats, which later sub-projects build on (ADR-0003). Both carry a version
field or are minimal and additive. Rollback is deleting `minuta-out/`.

## Dependency graph
```
T1 ──┬─> T2 ─────────────────────┐
     ├─> T3 ──────────────┐      │
     ├─> T4 ──┬──────────┐│      │
     ├─> T5 ──┤          ││      │
     │        ├─> T7 ────┼┼──────┤
     │        └─> T8 <───┘│      ├─> T9 ──> T10
     └─> T6 ─────────────────────┘
(T8 also needs T3, T4, T5; T7 needs T4, T5)
```
Can start immediately after T1, in parallel: T2, T3, T4, T5, T6.
After T4 and T5: T7 and T8 (T8 also needs T3). After T2, T6, T7, T8: T9. Then T10.

## Suggested agent assignment
For `baton:parallel`; model and effort chosen by how much judgment the task needs.

| Task | Model | Effort | Why |
|---|---|---|---|
| T1 | sonnet | low | mechanical scaffolding |
| T2 | haiku | low | small, well-specified |
| T3 | sonnet | medium | parsing and formatting edge cases |
| T4 | opus | high | core invariants: staleness, atomicity |
| T5 | sonnet | medium | subprocess handling, preflight |
| T6 | sonnet | medium | download, checksum, atomic rename |
| T7 | sonnet | medium | ffmpeg decision logic |
| T8 | sonnet | medium | whisper integration |
| T9 | opus | high | integration, interrupt handling |
| T10 | sonnet | medium | fixtures and tests |

## Tasks

### T1: Crate skeleton, error type and CI  (#issue once created)
- Priority: 0
- Depends on: none
- Owns: `Cargo.toml`, `rust-toolchain.toml`, `rustfmt.toml`, `.gitignore`, `src/main.rs`, `src/lib.rs`, `src/error.rs`, empty `src/{cli,config,tools,transcript,models,progress,runner}.rs`, empty `src/job/{mod,fingerprint,state}.rs`, empty `src/steps/{mod,probe,join,audio,transcribe,outputs}.rs`, `.github/workflows/ci.yml`
- Touches (read-only or additive): `Makefile`
- Acceptance:
  - [ ] `make ci` passes on an otherwise empty crate
  - [ ] `lib.rs` declares every module listed above; no later task needs to edit `lib.rs` or the `[dependencies]` table
  - [ ] `Error` has the variants in the shared interfaces and maps to exit codes 1, 2, 130 via a `fn exit_code(&self) -> u8`
  - [ ] `.github/workflows/ci.yml` runs `make ci` on `ubuntu-latest` and `macos-latest`
  - [ ] `minuta --version` prints the crate version
- Tests first: `exit_code_usage_is_2`, `exit_code_interrupted_is_130`, `exit_code_other_is_1`
- Notes: dependencies up front: clap (derive), serde, serde_json, toml, sha2, thiserror, indicatif, ctrlc, directories, ureq (rustls), tempfile (dev), assert_cmd (dev), predicates (dev).

### T2: Config loading and precedence  (#issue once created)
- Priority: 1
- Depends on: T1
- Owns: `src/config.rs`, `tests/config.rs`
- Touches (read-only or additive): none
- Acceptance:
  - [ ] keys `out_dir`, `whisper.bin`, `whisper.model`, `whisper.lang`, `ffmpeg.bin`, `ffprobe.bin` load from TOML
  - [ ] precedence is flags > `MINUTA_*` env > config file > defaults, checked per key
  - [ ] `MINUTA_CONFIG` overrides the file path; a missing file yields defaults; a malformed file is `Error::Config` naming the file and line
  - [ ] `config path` and `config show` data functions return the resolved path and the merged config with each value's source
  - [ ] defaults: `out_dir=./minuta-out`, `whisper.lang=auto`
- Tests first: `flag_beats_env`, `env_beats_file`, `file_beats_default`, `missing_file_uses_defaults`, `malformed_file_names_line`, `minuta_config_env_overrides_path`
- Notes: expose `Config::resolve(flags: Overrides, env: &dyn Fn(&str)->Option<String>, file: Option<&str>)` so tests need no real environment.

### T3: Transcript schema, whisper parsing, TXT and SRT  (#issue once created)
- Priority: 1
- Depends on: T1
- Owns: `src/transcript.rs`, `tests/transcript.rs`, `tests/fixtures/whisper/*.json`
- Touches (read-only or additive): none
- Acceptance:
  - [ ] `from_whisper_json` parses `whisper-cli -oj` output (offsets in ms) into seconds and reads the detected language
  - [ ] segments in the result are ascending and non-overlapping; overlapping input is clamped, and a segment with `end < start` is dropped with a warning
  - [ ] zero segments yields an empty `Transcript`, not an error
  - [ ] `to_srt` uses `HH:MM:SS,mmm`, 1-based indices, blank line between cues, and passes UTF-8 (accents, CJK) through unchanged
  - [ ] `to_txt` is one segment per line, trimmed, no timestamps
  - [ ] `transcript.json` serializes as `{ "language": "...", "segments": [{"start","end","text"}] }`
- Tests first: `parses_offsets_to_seconds`, `reads_language`, `overlap_is_clamped`, `inverted_segment_dropped`, `empty_segments_ok`, `srt_timestamp_format`, `srt_over_one_hour`, `srt_preserves_utf8`, `txt_one_line_per_segment`, `json_roundtrip`
- Notes: capture a real whisper JSON (any language) as a fixture; do not hand-write the shape from memory.

### T4: Job state, naming, fingerprints and staleness  (#issue once created)
- Priority: 0
- Depends on: T1
- Owns: `src/job/**`, `tests/job.rs`
- Touches (read-only or additive): none
- Acceptance:
  - [ ] `derive_name` strips media extensions (`mp4 mkv mov m4v webm mp3 wav m4a flac`, case-insensitive), so `clase-4.mp4` and `--name clase-4.mp4` give `clase-4`
  - [ ] `job.json` has `version: 1`; loading another version is `Error::Job` naming that version and is never migrated
  - [ ] a step is stale if its fingerprint differs or any artifact is missing; a stale step makes every later step stale; the reason is stored in `steps.<name>.reason`
  - [ ] `--force` with no value makes all steps stale; `--force <step>` makes that step and later ones stale
  - [ ] source comparison uses size+mtime first and a content hash only on mismatch; unchanged content with a changed mtime stays cached
  - [ ] `write_atomic` leaves no output at the destination if the closure fails; leftover `*.tmp` files are removed when a job is opened
  - [ ] same name with a different source set is an error unless the caller passes `allow_replace`
  - [ ] `status: done` is never written before every artifact exists
- Tests first: `derive_name_strips_extension`, `derive_name_case_insensitive`, `unknown_version_rejected`, `changed_fingerprint_marks_stale`, `stale_propagates_to_later_steps`, `missing_artifact_marks_stale`, `force_all`, `force_step_and_later`, `mtime_change_same_content_stays_cached`, `write_atomic_failure_leaves_nothing`, `tmp_cleaned_on_open`, `name_conflict_different_sources`
- Notes: depend only on the `Step` trait shape in this plan. Do not run external tools here.

### T5: Process runner, toolchain preflight and doctor data  (#issue once created)
- Priority: 0
- Depends on: T1
- Owns: `src/tools.rs`, `tests/tools.rs`
- Touches (read-only or additive): none
- Acceptance:
  - [ ] `run` takes an argv array and never invokes a shell; a file name `a b"$(touch pwned)'.mp4` is passed intact and creates nothing
  - [ ] `run` streams stdout and stderr lines to `on_line` and returns the exit status; non-zero is `Error::Tool` with the last stderr lines
  - [ ] `preflight` reports every missing item (ffmpeg, ffprobe, whisper-cli, and the model when `need_model`) in one `Error::MissingTools`, each with an install hint
  - [ ] tool versions are captured as strings for fingerprints and `doctor`
  - [ ] an interrupt flag set by the caller kills the child and returns `Error::Interrupted` within 1 second
- Tests first: `argv_with_shell_metacharacters_is_safe`, `streams_lines`, `nonzero_exit_is_tool_error`, `preflight_lists_all_missing`, `version_captured`, `interrupt_kills_child`
- Notes: tests use small scripts on `PATH` in a temp dir, never the real tools.

### T6: Model catalog and download  (#issue once created)
- Priority: 2
- Depends on: T1
- Owns: `src/models.rs`, `tests/models.rs`
- Touches (read-only or additive): none
- Acceptance:
  - [ ] catalog lists `tiny base small medium large-v3` with Hugging Face ggml URLs and pinned SHA-256 values taken from the upstream repository, with the source noted in a comment
  - [ ] `pull` writes to a temp file in the models dir and renames only after the checksum matches
  - [ ] a wrong checksum is rejected with the message `checksum mismatch for <name>` and leaves no file in the models dir
  - [ ] `pull` of an already valid model does nothing; `list` shows installed and available models with sizes
  - [ ] model name or path resolution: an existing path is used as is, a catalog name resolves to the models dir
- Tests first: `bad_checksum_rejected_and_cleaned`, `good_download_renamed`, `already_present_noop`, `resolve_path_vs_name`
- Notes: tests serve bytes from a local `TcpListener`; the base URL is injectable. Models dir: `~/.cache/minuta/models`.

### T7: Media steps — probe, join, audio  (#issue once created)
- Priority: 1
- Depends on: T4, T5
- Owns: `src/steps/probe.rs`, `src/steps/join.rs`, `src/steps/audio.rs`, `tests/media.rs`
- Touches (read-only or additive): `src/steps/mod.rs` (add `pub use` lines only)
- Acceptance:
  - [ ] `probe` records codec, resolution, fps and duration per source; a source with no audio stream fails with `no audio stream in <file>`
  - [ ] a single source: `joined.<ext>` is a hard link, falling back to a copy
  - [ ] several sources with matching codec, resolution and fps join by stream copy; any mismatch re-encodes; inputs keep the given order
  - [ ] re-encode uses VideoToolbox when `ffmpeg -encoders` lists it, else libx264
  - [ ] `audio` produces mono 16 kHz WAV
  - [ ] each step's `params` changes when its relevant inputs change (encoder choice, source list)
- Tests first: `probe_reads_streams`, `no_audio_rejected`, `single_source_links`, `matching_sources_stream_copy`, `mismatched_sources_reencode`, `join_preserves_order`, `audio_is_mono_16k`, `encoder_choice_falls_back_to_x264`
- Notes: fixtures from `ffmpeg -f lavfi` (`sine`, `testsrc`), generated in test setup, not committed.

### T8: Transcribe and outputs steps  (#issue once created)
- Priority: 1
- Depends on: T3, T4, T5
- Owns: `src/steps/transcribe.rs`, `src/steps/outputs.rs`, `tests/transcribe.rs`
- Touches (read-only or additive): `src/steps/mod.rs` (add `pub use` lines only)
- Acceptance:
  - [ ] whisper is invoked with `--vad`, `-oj`, the resolved model and `-l <lang>` (`auto` passes `-l auto`)
  - [ ] `params` includes model path identity, language and whisper version, so changing any makes the step stale
  - [ ] `transcript.json` is written atomically from the parsed result; zero segments warns and still writes empty outputs
  - [ ] `outputs` renders `transcript.txt` and `captions.srt` from `transcript.json` only (never from whisper's raw file)
  - [ ] whisper failure leaves no `transcript.json`
- Tests first: `whisper_args_include_vad_and_json`, `lang_change_changes_params`, `model_change_changes_params`, `empty_transcript_still_writes_outputs`, `whisper_failure_leaves_no_artifact`, `outputs_derive_from_transcript_json`
- Notes: use a fake `whisper-cli` script that prints a fixture JSON from T3.

### T9: CLI, runner, progress and interrupt  (#issue once created)
- Priority: 1
- Depends on: T2, T6, T7, T8
- Owns: `src/cli.rs`, `src/runner.rs`, `src/progress.rs`, `src/main.rs`
- Touches (read-only or additive): none
- Acceptance:
  - [ ] subcommands `run`, `transcribe`, `doctor`, `models list|pull`, `config path|show` with the flags in the spec; usage errors exit 2
  - [ ] `run` executes probe→join→audio→transcribe→outputs through `plan_stale`; cached steps print `cached` and execute nothing
  - [ ] preflight runs before any step and reports all missing items at once
  - [ ] stdout carries only the job folder path; progress (step counter, timing, live ffmpeg/whisper progress) goes to stderr; `--quiet` and `NO_COLOR` or a non-terminal stderr disable extras
  - [ ] Ctrl-C kills the child, removes temp files, exits 130 and marks no step done
  - [ ] `doctor` prints tool versions and model status, exit 1 if anything is missing
  - [ ] `transcribe` behaves identically to `run`
- Tests first: `usage_error_exits_2`, `second_run_all_cached`, `stdout_is_only_job_path`, `preflight_before_first_step`, `quiet_suppresses_progress`, `doctor_exit_code_when_missing`
- Notes: heavy tests belong to T10; keep these as `assert_cmd` tests with fake tools on `PATH`.

### T10: Integration suite, fixtures and opt-in e2e  (#issue once created)
- Priority: 2
- Depends on: T9
- Owns: `tests/integration/**`, `tests/fixtures/bin/**`, `tests/e2e.rs`
- Touches (read-only or additive): `.github/workflows/ci.yml` (add ffmpeg install and the e2e job only)
- Acceptance:
  - [ ] full run on lavfi fixtures with a fake `whisper-cli` produces all five artifacts with valid contents
  - [ ] second identical run reports every step `cached`
  - [ ] changing `--lang` reruns `transcribe` and `outputs` only, with the reason recorded in `job.json`
  - [ ] changing a source file reruns from `probe` onward
  - [ ] a run killed during `transcribe` and rerun completes, with no `.tmp` leftovers
  - [ ] a file name with spaces, quotes and `$(…)` runs and executes nothing
  - [ ] name conflict with a different source set fails; `--name` resolves it
  - [ ] `MINUTA_E2E=1` runs real whisper with the `tiny` model on a short clip
  - [ ] CI installs ffmpeg on both operating systems and passes on both
- Tests first: `full_run_produces_artifacts`, `rerun_is_cached`, `lang_change_reruns_transcribe_only`, `source_change_reruns_all`, `killed_step_resumes`, `hostile_filename_is_inert`, `name_conflict_errors`, `e2e_real_whisper_tiny`
- Notes: the e2e test is skipped unless `MINUTA_E2E=1`.

## Coverage check
| Spec requirement | Task(s) |
|---|---|
| Goal 1: five artifacts from `run` | T7, T8, T9, T10 |
| Goal 2: transcript schema and ordering | T3, T8 |
| Goal 3: second run all cached | T4, T9, T10 |
| Goal 4: changed lang/model/source reruns | T4, T8, T10 |
| Goal 5: kill and resume, no partial output | T4, T5, T9, T10 |
| Goal 6: preflight lists all missing | T5, T9 |
| Goal 7: job name extension stripping | T4 |
| Goal 8: macOS and Linux CI | T1, T10 |
| Config and precedence | T2 |
| Model download with checksum | T6 |
| Join: stream copy vs re-encode, encoder choice | T7 |
| No-audio source error | T7 |
| Zero-segment warning, empty outputs | T3, T8 |
| Atomic writes, tmp cleanup | T4 |
| Name conflict rule | T4, T10 |
| Unknown `job.json` version | T4 |
| Ctrl-C and exit 130 | T1, T5, T9 |
| stdout/stderr split, NO_COLOR, quiet | T9 |
| `doctor`, `models`, `config` commands | T2, T5, T6, T9 |
| Shell-injection safety | T5, T10 |
| SRT/TXT formats, UTF-8 | T3 |
