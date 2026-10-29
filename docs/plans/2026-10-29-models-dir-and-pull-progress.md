# Models directory and download progress — Implementation Plan

- Spec: docs/specs/2026-10-29-models-dir-and-pull-progress.md
- Date: 2026-10-29
- Status: approved

## Overview
Five tasks in a chain that avoids file overlap. T1 (config keys) and T2 (catalog layout) are
independent and can run in parallel. T3 (CLI wiring) needs both. T4 (progress bar) needs T3
because both touch `src/cli.rs`, and T5 (docs) closes the work. Shared foundations (the config
key and the catalog subfolder) come first, so nobody builds on a moving target.

## Contract
none. No HTTP API. The CLI output of `models list`, `models pull` and `config` changes as the
spec describes, and T5 documents it.

## Impact (risky changes only)
- Affected call paths / consumers: `minuta models list|pull`, model resolution for `whisper.model` and `whisper.vad_model` in `src/runner.rs` (bare catalog names), `minuta config` output.
- Data and migrations: none in job folders. Models in `~/.cache/minuta/models` are ignored, not moved (spec non-goal). The VAD model moves from `<dir>/<file>` to `<dir>/vad/<file>`.
- Tests and downstream checks that must stay green: `tests/models.rs`, `tests/config.rs`, `tests/cli.rs`, `tests/e2e.rs`, `tests/integration/*` (they use the models directory through the runner).
- Rollout and rollback: ships in the next release. Rollback is a revert; no data changes, because no file is moved or deleted.

## Dependency graph
```
T1 ──┐
     ├──> T3 ──> T4 ──> T5
T2 ──┘
```
Can start immediately in parallel: T1, T2

## Tasks

### T1: Models directory config and env keys  (#issue once created)
- Priority: 0
- Depends on: none
- Owns: `src/config.rs`, `tests/config.rs`
- Touches (read-only or additive): `src/cli.rs` (no edits; the `config` printer is in T3)
- Acceptance:
  - [ ] `MINUTA_MODELS_DIR` is listed in `ENV_NAMES` and read into `Config::models_dir`
  - [ ] Precedence is env > `whisper.models_dir` in the config file > default `~/.minuta/models`
  - [ ] Each `Setting` records the source (`Env`, `File` or `Default`)
  - [ ] A leading `~/` expands to the home directory; a `~` with no home directory is `Error::Config` naming `whisper.models_dir`
  - [ ] An empty value counts as unset
  - [ ] An unknown key under `[whisper]` is still rejected (`deny_unknown_fields`)
- Tests first: `models_dir_defaults_under_home_minuta`, `models_dir_env_beats_file`, `models_dir_file_used_when_no_env`, `models_dir_tilde_expands`, `models_dir_empty_value_is_unset`, `models_dir_without_home_is_config_error`
- Notes: follow the `pick`/`pick_opt` pattern used for `whisper.model`. `Config::resolve` takes an injected env closure, so tests need no real home directory; inject the home path through a parameter, not `BaseDirs` directly.

### T2: Catalog subfolders and resolution  (#issue once created)
- Priority: 0
- Depends on: none
- Owns: `src/models.rs` (catalog, `resolve`, `list`, and the directory helpers), `tests/models.rs`
- Touches (read-only or additive): none
- Acceptance:
  - [ ] Whisper entries resolve to `<dir>/<file>`; VAD entries to `<dir>/vad/<file>`
  - [ ] `list(dir)` checks the same paths as `resolve` and reports `installed` only for a regular file at that path
  - [ ] A path that is an existing file is returned as is by `resolve`; an unknown name is `Error::Usage` listing the catalog names (unchanged)
  - [ ] `default_dir()` returns `~/.minuta/models` (replaces `~/.cache/minuta/models`) and no code reads the old path
  - [ ] `pull` creates `<dir>/vad/` when needed and leaves the rest of the download behavior as is
- Tests first: `whisper_model_resolves_flat`, `vad_model_resolves_into_vad_subfolder`, `list_reports_vad_under_vad_subfolder`, `default_dir_is_minuta_models`, `old_cache_folder_is_ignored`, `pull_creates_vad_subfolder`
- Notes: add a `subdir` derived from `ModelKind` in one place, not per entry. Keep `ModelSpec`'s fields otherwise unchanged so `CATALOG` stays a const.

### T3: CLI and runner use the models directory  (#issue once created)
- Priority: 1
- Depends on: T1, T2
- Owns: `src/cli.rs` (the `models` handlers and the `config` printer), `tests/cli.rs`
- Touches (read-only or additive): `src/runner.rs` (only the line that calls `models::default_dir()`, around line 476, to call the resolved directory instead)
- Acceptance:
  - [ ] `models list` prints the header `models in <dir>` first, then the catalog lines as today
  - [ ] `models pull` uses the resolved directory and prints the same path to stdout as before
  - [ ] A bare catalog name in `whisper.model` or `whisper.vad_model` resolves into the resolved directory (runner)
  - [ ] `minuta config` prints `models_dir` with its source (env, config file or default)
  - [ ] With `MINUTA_MODELS_DIR` set, `models list` and `models pull` both use it
- Tests first: `models_list_prints_directory_header`, `models_pull_stdout_is_only_the_path`, `runner_resolves_bare_name_into_models_dir`, `config_prints_models_dir_and_source`, `env_models_dir_used_by_list_and_pull`
- Notes: `models::default_dir()` callers become the resolved value from T1; pass it down, do not re-read the environment in `cli.rs`. Keep the header and `config` line wording exact; T5 documents them.

### T4: Download progress bar for `models pull`  (#issue once created)
- Priority: 2
- Depends on: T2, T3
- Owns: `src/models.rs` (`download` and `pull` only), new progress helper in `src/progress.rs` (a `ModelProgress` type, additive), `tests/models.rs` (progress cases)
- Touches (read-only or additive): `src/cli.rs` (the `models_pull` call site only)
- Acceptance:
  - [ ] On an interactive stderr without `NO_COLOR`, `pull` draws a bar with percent, bytes/s and ETA, sized from the catalog's `size`
  - [ ] Otherwise `pull` prints one start line `pulling <name> (<size>) into <dir>` and one finish line (already present or downloaded and verified)
  - [ ] `--quiet` prints neither the bar nor the start and finish lines; errors still print
  - [ ] A failed download clears the bar before the error prints and removes the `.part` file
  - [ ] Stdout still carries only the resolved path
- Tests first: `pull_progress_reports_bytes_and_total`, `pull_plain_mode_prints_start_and_finish`, `pull_quiet_prints_nothing`, `pull_failure_removes_part_and_clears_bar`, `pull_stdout_is_only_path_with_progress`
- Notes: the byte sink is a trait or closure passed to `pull`, so tests can record calls without a terminal. The bar uses `indicatif` (already a dependency) and reuses `Mode::choose` from `src/progress.rs` for the terminal and `NO_COLOR` decision. Local HTTP server for tests: follow `tests/support/fake_ollama.rs`.

### T5: Docs for the models directory  (#issue once created)
- Priority: 3
- Depends on: T3, T4
- Owns: `README.md` (models and config sections only), `docs/adr/0010-models-directory-location.md` (status only), `docs/specs/2026-10-29-models-dir-and-pull-progress.md` (implementation status checklist)
- Touches (read-only or additive): `docs/install.md` (only if it names the old cache path)
- Acceptance:
  - [ ] README shows the default directory, the `MINUTA_MODELS_DIR` and `whisper.models_dir` overrides, and the `vad/` subfolder
  - [ ] README says where the old `~/.cache/minuta/models` folder is and that it can be deleted
  - [ ] No file in the repo still names `~/.cache/minuta/models` except the ADR and the spec's history
  - [ ] ADR-0010 status is `accepted`
  - [ ] Spec "Implementation status" lists T1 to T5 with links once issues exist
- Tests first: none (docs). Check with `grep -rn "\.cache/minuta" README.md docs` and `make ci`.
- Notes: keep the README's existing structure; add to the models section, do not rewrite it.

## Coverage check
| Spec requirement | Task(s) |
|---|---|
| Default directory `~/.minuta/models` | T2 (`default_dir`), T5 (docs) |
| Override order env > config > default | T1 |
| `whisper.models_dir` config key, `~` expansion | T1 |
| Whisper models flat, VAD in `vad/` | T2 |
| `models list` shows directory header | T3 |
| `models pull` stdout is only the path | T3, T4 (regression test) |
| Bare catalog names resolve into the directory | T3 |
| `minuta config` shows directory and source | T3 |
| Progress bar on interactive stderr; start/finish lines otherwise | T4 |
| `--quiet` hides bar and lines | T4 |
| Failure clears bar and removes `.part` | T4 |
| Old cache folder ignored | T2 (test), T5 (docs) |
| ADR-0010 and glossary | T5 (ADR status); glossary already committed |
