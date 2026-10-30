# Job folder layout — Implementation Plan

- Spec: docs/specs/2026-10-30-job-folder-layout.md
- Date: 2026-10-30
- Status: in backlog

## Overview
One shared foundation first: a layout module that owns every path in the job folder, plus the move of minuta's own bookkeeping into `.minuta/` (T1). Then two parallel tasks build on it: media and transcript files into `assets/` with no single-source join (T2), and template outputs into `assets/` and `.minuta/derived/` (T3). T4 is independent: the social templates read the transcript instead of the summary. T5 updates the docs and adds the end-to-end listing test once the rest has merged.

## Contract
None. No CLI flag or config key changes. The file layout of the job folder changes (see the spec).

## Impact
- Affected call paths / consumers: every reader or writer of a job-folder path: `src/job/**`, `src/steps/**`, `src/llm/call.rs`, `src/generate.rs`, `src/chapters/mod.rs`, `src/fields/mod.rs`, `src/runner.rs`, and the tests that name those paths (`tests/cli.rs`, `tests/job.rs`, `tests/generate.rs`, `tests/integration/*`).
- Data and migrations: none. Old job folders are not read (ADR-0013). Paths recorded in `job.json` change, and an old `job.json` is never opened.
- Tests and downstream checks that must stay green: `make ci` after each task. Between T1 and T5 the repo may carry a mix of old and new paths only inside unmerged branches; each task merges with `make ci` green.
- Rollout and rollback: released in the next minor version, with a changelog note that old jobs are rebuilt on the next run and scripts reading the old paths must change. Rollback is a revert of the merge commits.

## Dependency graph
```
T1 ──> T2 ──> T5
T1 ──> T3 ──> T5
T4 ───────────> T5
```
Can start immediately in parallel: T1, T4. After T1: T2, T3 in parallel.

## Tasks

### T1: Layout module and `.minuta/` bookkeeping  (#185)
- Priority: 0
- Depends on: none
- Owns: `src/job/**`, `src/llm/call.rs`, `src/steps/probe.rs`, `tests/job.rs`
- Touches (read-only or additive): `src/steps/mod.rs`, `src/runner.rs` (call sites of the renamed constants only)
- Acceptance:
  - [ ] A layout module in `src/job/` returns the paths for: state file, probe file, call-cache dir, derived dir, raw whisper output, `transcript.json`, `audio.wav`, joined file and per-template json (`assets/<name>.json`). Folder names `assets` and `.minuta` are defined once.
  - [ ] `job.json`, `probe.json` and `llm-cache/` are written under `.minuta/`, and `Job::open` reads `.minuta/job.json`.
  - [ ] `write_atomic` creates a missing parent directory.
  - [ ] `Job::open` removes leftover `*.tmp` in the root, `assets/` and `.minuta/`.
  - [ ] A folder with a flat-layout `job.json` and no `.minuta/job.json` is treated as a new job and the run succeeds; the old files are left untouched.
  - [ ] `--force` removes `.minuta/` state together with the job's other artifacts.
  - [ ] `make ci` passes.
- Tests first: `layout_paths_are_under_minuta_and_assets`, `write_atomic_creates_missing_parent`, `open_cleans_tmp_in_all_tiers`, `flat_layout_job_is_treated_as_new`, `call_cache_lives_in_minuta`
- Notes: the constants `JOB_FILE`, `PROBE_FILE` are replaced by layout functions. Do not change step logic beyond path lookups.

### T2: Media and transcript files in `assets/`, no single-source join  (#187)
- Priority: 1
- Depends on: T1
- Owns: `src/steps/join.rs`, `src/steps/audio.rs`, `src/steps/transcribe.rs`, `src/steps/outputs.rs`, `tests/transcribe.rs`, `tests/media.rs`
- Touches (read-only or additive): `src/runner.rs`, `tests/cli.rs`, `tests/e2e.rs`, `tests/integration/pipeline.rs`
- Acceptance:
  - [ ] `transcript.json`, `audio.wav` and, for several sources, `joined.<ext>` are written to `assets/`.
  - [ ] With one source, no `joined.*` exists anywhere in the job folder, and `audio.wav` is extracted from the original file.
  - [ ] With several sources, `assets/joined.<ext>` exists and is the only joined copy.
  - [ ] The whisper raw output goes to `.minuta/` and is removed after the step.
  - [ ] `transcript.txt` and `captions.srt` stay at the top level.
  - [ ] A second unchanged run skips every step. Changing the source list re-runs the join.
  - [ ] `make ci` passes.
- Tests first: `single_source_has_no_joined_file`, `several_sources_join_into_assets`, `audio_is_extracted_from_the_original_for_one_source`, `second_run_skips_all_steps`, `transcript_txt_and_srt_stay_at_root`
- Notes: `Mode::Link` and `link_or_copy` are removed with their tests. The fingerprint of the audio step must still change when the source changes.

### T3: Template outputs in `assets/` and implicit outputs in `.minuta/derived/`  (#188)
- Priority: 1
- Depends on: T1
- Owns: `src/generate.rs`, `src/chapters/mod.rs`, `src/fields/mod.rs`, `tests/generate.rs`, `tests/integration/fields.rs`, `tests/integration/chapters.rs`, `tests/integration/templates.rs`
- Touches (read-only or additive): `src/runner.rs`, `src/template/graph.rs`, `tests/cli.rs`
- Acceptance:
  - [ ] A requested template writes `<name>.md` at the top level and `<name>.json` (fields, chapters) in `assets/`.
  - [ ] A template that runs only as a dependency of a requested one writes `.md` and `.json` to `.minuta/derived/`, and the dependent reads them there.
  - [ ] Requesting a template that has `derived/` copies writes the normal outputs and removes the copies.
  - [ ] `-t youtube` leaves no `chapters.*` at the top level or in `assets/`.
  - [ ] Stale detection still skips unchanged templates, including after a template moves from implicit to requested.
  - [ ] `make ci` passes.
- Tests first: `requested_template_writes_md_at_root_and_json_in_assets`, `implicit_dependency_writes_to_derived`, `requesting_a_derived_template_moves_its_outputs`, `youtube_run_leaves_no_chapters_at_root`, `unchanged_templates_are_skipped_after_move`
- Notes: output paths are declared in `src/generate.rs:117`. Keep `<name>.md` rendering unchanged.

### T4: Social templates read the transcript  (#186)
- Priority: 2
- Depends on: none
- Owns: `src/template/builtin/linkedin.md`, `src/template/builtin/instagram.md`, `src/template/builtin/youtube.md`, `templates/*.md`, `templates/index.toml`, `tests/templates.rs`, `tests/templates_catalog.rs`, `tests/pull_catalog.rs`
- Touches (read-only or additive): none
- Acceptance:
  - [ ] Every built-in and catalog template with `input: summary` uses `input: transcript`, and its prompt text says "transcript" where it referred to the summary.
  - [ ] `templates/index.toml` hashes match the changed files.
  - [ ] `minuta run <file> -t linkedin -t instagram -t youtube` produces no `summary.*` (checked with the fake LLM).
  - [ ] `-t summary` still produces `summary.md`.
  - [ ] `make ci` passes.
- Tests first: `social_builtins_read_transcript`, `catalog_templates_declare_transcript_input`, `catalog_hashes_match_files`, `social_run_creates_no_summary`
- Notes: `docs/template-briefs/` describe the intended inputs; update the briefs that say summary. Check the catalog hash test for how hashes are computed before editing the index.

### T5: Docs and end-to-end layout test  (#189)
- Priority: 2
- Depends on: T2, T3, T4
- Owns: `README.md`, `CONTEXT.md`, `docs/common-issues.md`, `tests/integration/layout.rs`
- Touches (read-only or additive): `docs/install.md`, `docs/template-briefs/**`, `tests/integration/main.rs`
- Acceptance:
  - [ ] README job-folder section, the opening example and every path it names match the new layout; the template table shows `transcript` as the input of the social templates.
  - [ ] CONTEXT.md: **Job** and **Call cache** use new paths; **Assets folder** and **State folder** added.
  - [ ] An integration test runs `-t linkedin -t youtube -t instagram` with one source and asserts the exact listings of the root, `assets/` and `.minuta/` from the spec, and a two-source run asserts `assets/joined.*`.
  - [ ] `make ci` passes.
- Tests first: `layout_single_source_social_run`, `layout_two_sources_has_joined_in_assets`
- Notes: `docs/specs/**` and ADRs of earlier work are history; do not edit them.

## Coverage check
| Spec requirement | Task(s) |
|---|---|
| `.minuta/` holds job.json, probe.json, llm-cache, raw whisper | T1, T2 |
| `assets/` holds transcript.json, audio.wav, template json | T2, T3 |
| No joined file for one source; single copy for several | T2 |
| Top level shows only documents of requested templates | T3, T5 |
| Social templates read transcript; no summary unless asked | T4 |
| Implicit dependency outputs in `.minuta/derived/`, moved on request | T3 |
| tmp cleanup in all tiers; old flat job treated as new; `--force` | T1 |
| Re-run skips steps; `--force` redoes | T1, T2, T3 |
| Docs and CONTEXT.md match | T5 |
