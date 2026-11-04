# Source file size limit — Implementation Plan

- Spec: [docs/specs/2026-11-03-src-file-size-limit.md](../specs/2026-11-03-src-file-size-limit.md)
- Date: 2026-11-03
- Status: in backlog

## Overview
T1 lands the guard: the check script, its tests, the allowlist with the eleven current
entries, the `make file-size` target, and the conventions rule. Then come eleven split tasks,
one per file, largest first. Each split task removes its file's allowlist entry, so after
T12 the allowlist has only its header comment.

The tasks form one stack, because every task edits `scripts/file-size-allowlist.txt`. Each
PR's base is the previous task's branch. They merge in order, T1 first, each only with the
user's explicit permission. The repo squash-merges, so after a merge the next branch is
rebased with `git rebase --onto main <merged branch>` and its PR is retargeted to `main`.

Common rules for every split task (T2 to T12):
- **Layout.** A split file `src/x.rs` becomes the directory `src/x/` with `mod.rs`. This
  matches how the repo already lays out directory modules (`src/llm/mod.rs`,
  `src/chapters/mod.rs`). Nested files such as `src/llm/chunk.rs` become `src/llm/chunk/mod.rs`.
  The allowlist entry for the old path is removed in the move commit.
- **Tests go with the code they test.** A test whose subject moved to `x/a.rs` moves into a
  `#[cfg(test)] mod tests` in `x/a.rs`, or into `x/a/tests.rs` if it would push `a.rs` over
  the limit. Test helpers that several test modules use go in `x/test_support.rs`
  (`#[cfg(test)] mod test_support;`, declared in `x/mod.rs`, with `pub(super)` helpers).
  Tests are never rewritten. Only their `use` lines, paths and indentation change.
- **Paths.** New submodules are private (`mod a;`, never `pub mod a;`). `mod.rs`
  re-exports every item that was `pub` in the old file and moved out
  (`pub use a::{Item, ...};`), so `crate::x::Item` and `minuta::x::Item` keep resolving.
  Items that were already `pub(crate)` are re-exported with `pub(crate) use`. Items that
  were private and are now called across files become `pub(super)`. A re-import that only
  tests need is `#[cfg(test)]`.
- **Budget.** Every new file has at most 450 lines after the move, which leaves room under
  the limit for small later changes. The line ranges below come from `origin/main` at
  f9cc674 and are approximate. Item boundaries decide.
- **Commits.** Commit 1 is the move plus the allowlist edit. Commit 2 is the idiom pass, or
  an empty commit that says the checklist found nothing.
- **Shared acceptance** (repeated in each task as "the split checks"):
  - [ ] `make ci` passes on commit 1 and on commit 2.
  - [ ] `git diff -M --color-moved=dimmed-zebra --color-moved-ws=allow-indentation-change
    HEAD~2 HEAD~1` shows only moved lines, `use` lines, `mod` lines, `pub use` lines and
    visibility changes.
  - [ ] The test-list files from the spec's command, before and after, are identical (same
    OS).
  - [ ] Commit 2 changes no behavior. Anything that would is filed as an issue and linked in
    the PR.
  - [ ] `git diff --stat origin/main -- tests/` is empty.
  - [ ] Every `.rs` file the task creates has at most 500 lines (the check enforces it).
  - [ ] The PR description lists the review checklist with a result for each item.

## Contract
None. No public path changes: every `minuta::` path used by `tests/` or `src/main.rs`
keeps resolving through `pub use`.

## Impact
- Affected call paths and consumers: internal `crate::` imports of the eleven modules, the
  integration tests (15 `minuta::` paths) and `src/main.rs`. Nothing outside this crate
  depends on the library.
- Data and migrations: none.
- Tests and checks that must stay green: all of `make ci` on Linux and macOS, including the
  interrupt tests in `src/runner.rs` and `tests/` that `handle_signals` (`src/main.rs:30`)
  feeds.
- Rollout and rollback: each PR is a pure move plus idiom fixes. A PR can be reverted on its
  own, but the later PRs in the stack build on it, so a revert of an early PR means rebasing
  the later ones.

## Dependency graph
```
T1 ──> T2 ──> T3 ──> T4 ──> T5 ──> T6 ──> T7 ──> T8 ──> T9 ──> T10 ──> T11 ──> T12
```
Can start immediately: T1. The chain is linear because each task edits the allowlist and
the series is stacked by decision. The split work itself would not conflict.

## Tasks

### T1: File size guard: check script, tests, allowlist, make target  (#253)
- Priority: 1
- Depends on: none
- Owns: `scripts/check-file-size.sh`, `scripts/check-file-size-test.sh`,
  `scripts/file-size-allowlist.txt`, `Makefile`, `docs/conventions.md` (one new rule)
- Acceptance:
  - [ ] `make file-size` runs the test script, then the check on `src/`, and `make ci` is
    `fmt clippy file-size test`.
  - [ ] The allowlist lists exactly the eleven files with the counts from the spec, and
    `make file-size` passes on `origin/main`.
  - [ ] The test script covers every case in the spec's Testing strategy, one case each,
    asserting the exit code and the exact stderr line.
  - [ ] Red run shown: the test script fails before `check-file-size.sh` exists.
  - [ ] Mutation shown: replacing the `awk` count with `wc -l` makes the
    no-trailing-newline case fail. Changing `-gt 500` to `-ge 500` makes the 500-line case
    fail.
  - [ ] The script passes `bash -n`. It runs under `/bin/bash` 3.2 on macOS and under bash
    on Ubuntu (CI matrix).
  - [ ] No file under `src/` or `tests/` changes.
- Tests first: `scripts/check-file-size-test.sh` cases `exactly_500_passes`,
  `over_501_fails`, `no_trailing_newline_counts`, `listed_grew_fails`,
  `listed_at_its_count_passes`, `listed_shrank_over_limit_passes`,
  `listed_within_limit_fails`, `missing_entry_fails`, `two_offenders_both_reported`,
  `each_rule_reported_in_one_run`, `nested_rs_counted`, `non_rs_ignored`, `nested_src_dir`,
  `header_only_allowlist_passes`, `empty_allowlist_passes`, `comments_and_blanks_ignored`,
  `tabs_separate_fields`, `last_entry_without_newline_read`, `missing_allowlist_errors`,
  `missing_src_errors`, and one case per malformed form, pattern `malformed_*_errors`
  (`field_count_one`, `field_count_three`, `count_not_a_number`, `count_leading_zero`,
  `count_within_limit`, `path_not_rs`, `path_outside_src`, `duplicate_path`), plus
  `bad_src_form_errors_*` (`dot_slash`, `trailing_slash`, `absolute`).
- Notes: build fixtures under `mktemp -d` and remove them with a `trap`. Run the check with
  `cd "$fixture" && FILE_SIZE_SRC=src FILE_SIZE_ALLOWLIST=allow.txt "$script"`. Sort the
  file list (`find ... | LC_ALL=C sort`) so the violation order is stable. Do not expand a
  possibly empty array under `set -u` (bash 3.2). Read the allowlist into newline-separated
  strings instead of arrays.

### T2: Split `src/runner.rs` (1651)  (#254)
- Priority: 1
- Depends on: T1
- Owns: `src/runner.rs` → `src/runner/{mod,preflight,pipeline,templates,test_support}.rs`
  plus test files as needed; the `src/runner.rs` allowlist entry.
- Split:
  - `mod.rs`: `Options`, `ForceArg`, `run`, `Resolved`, `from_flag`, `resolve`, `deps_of`,
    `resolve_force` (lines 1-340), with their tests (`chain`, `force*`, `deps_of_*`,
    `catalog_of`, `read_input_from`, `distinct_backends_*`, `off_machine_for`, `notice_for_*`).
  - `preflight.rs`: `Ready`, `preflight`, `locate_bin`, `llm_status`, `invalid_status`,
    `inspect`, `tool_paths`, `bin_path`, model and VAD path functions, `with_pull_hint`
    (341-604), with their tests (`default_binaries_*`, `configured_paths_*`, `*_model_*`,
    `pull_hint_*`, `preflight_without_templates_*`).
  - `pipeline.rs`: `Execute` and its impls, `probed_seconds`, `Runnable`, `Pipeline`,
    `interrupted_by`, `detail` (605-812, 962-984), with their tests (`probed_*`,
    `join_and_audio_*`, `transcribe_reports_*`, `run_one*`, `spinner_*`, `media_seconds_*`,
    the interrupt and signal tests).
  - `templates.rs`: `TemplatePhase`, `concurrency`, `detected_language` (813-961), with
    `concurrency_defaults_per_backend` and `endpoint_concurrency_defaults_to_one`.
  - `test_support.rs`: the shared helpers `config`, `probed_job` and any other helper used
    by more than one test module.
- Acceptance: the split checks; `runner::{run, Options, ForceArg, llm_status, inspect,
  tool_paths}` and `crate::runner::preflight` resolve as before. Also the interrupt tests
  (`preset_interrupt_runs_no_step`, `interrupt_during_the_last_step_is_reported`,
  `only_signal_deaths_count_as_interrupts`, `signal_death_without_flag_is_reported_as_is`)
  pass and are named in the PR.
- Tests first: none new. The existing tests are the regression suite. The test-list
  comparison proves none was lost.
- Notes: `preflight` is a function and becomes a module name too. Keep the function at
  `crate::runner::preflight` via `pub(crate) use preflight::preflight;`. If the clash is
  awkward, name the module `checks.rs` instead.

### T3: Split `src/llm/chunk.rs` (1383)  (#255)
- Priority: 1
- Depends on: T2
- Owns: `src/llm/chunk.rs` → `src/llm/chunk/{mod,pack,test_support}.rs` plus test files;
  its allowlist entry.
- Split:
  - `mod.rs`: constants, `transcript_text`, `block_line`, `estimate_tokens`,
    `language_instruction`, `Request`, `Event`, `Generated`, the type aliases, `generate`,
    `generate_with`, `template_error`, `fits`, `input_budget`, `run_all` (1-246, 389-444).
  - `pack.rs`: `split_inputs`, `part_units`, `join_notes`, `pack`, `Packer`,
    `split_at_whitespace` (247-388), with `chunks_never_exceed_budget`,
    `overlong_line_split_at_whitespace`, `huge_single_line_splits_quickly`.
  - Tests (937 lines today) split by subject: `chunk/tests_text.rs` (transcript text,
    estimate and language tests), `chunk/tests_map.rs` (fit, chunk order, concurrency,
    errors, events), `chunk/tests_reduce.rs` (reduce, merge rounds, final block, finish).
    Shared helpers (`seg`, `transcript`, `numbered_lines`, `request`, `fits`, `part_index`,
    `is_final`, `map_calls`, `chunk_total`, `boom`, `run_with_block`) go in
    `test_support.rs`.
- Acceptance: the split checks; every `pub` item of `minuta::llm::chunk` resolves at the
  same path.
- Tests first: none new (see T2).

### T4: Split `src/llm/call.rs` (891)  (#256)
- Priority: 2
- Depends on: T3
- Owns: `src/llm/call.rs` → `src/llm/call/{mod,clean,cache}.rs` plus test files; its
  allowlist entry.
- Split:
  - `mod.rs`: `Policy`, `call`, `Answer`, `call_json`, `Sleeper`, `call_with`, `Decision`,
    `cached_call`, `non_empty`, `delay`, `sleep_interruptibly`, with the retry, backoff and
    JSON tests.
  - `cache.rs`: `with_path`, `TMP_COUNTER`, `CallCache` and its impl, `cache_key`, `key`,
    `json_cache_key`, with the cache-key, read, write, rename and `clean_tmp` tests.
  - `clean.rs`: `clean`, `backtick_run`, `bare_run`, `unwrap_fence` (331-372), with the
    `clean_*` tests.
- Acceptance: the split checks; `minuta::llm::call::{call, call_json, clean, CallCache,
  Policy, Answer}` resolve.

### T5: Split `src/chapters/validate.rs` (888)  (#257)
- Priority: 2
- Depends on: T4
- Owns: `src/chapters/validate.rs` → `src/chapters/validate/{mod,snap,test_support}.rs`
  plus test files; its allowlist entry.
- Split: code is 198 lines and stays mostly in `mod.rs`. `snap`/`snap_start` go to `snap.rs`
  with the `snap_*` and `first_moves_to_zero` tests. The rule tests
  (`violation_rule_*`, `timing_rules_*`, `structural_skips_timing`, `order_skips_duration`)
  go to `validate/tests_rules.rs`. The boundary and robustness tests (`*_boundary*`,
  `same_start_lasts_zero`, `title_normalized`, `extra_keys_ignored`, `no_panic_*`,
  `accepts_*`) go to `validate/tests_edges.rs`. Helpers go to `test_support.rs`.
- Acceptance: the split checks; `minuta::chapters::validate::parse_answer` resolves.

### T6: Split `src/tools.rs` (880)  (#258)
- Priority: 2
- Depends on: T5
- Owns: `src/tools.rs` → `src/tools/{mod,process,locate}.rs` plus test files; its
  allowlist entry.
- Split:
  - `process.rs`: `RunOpts`, `Output`, `Piece`, `Out`, `Line`, `piece`, `pump`,
    `ProgressLine`, `progress_line`, `clock_seconds`, `tool_name`, `spawn`, `interrupted`,
    `kill_*`, `drain`, `Tail`, `wait_status*`, `run`, `run_capture` (15-419), with the
    tail, progress-line and `try_wait_error_kills_child` tests.
  - `locate.rs`: `ToolPaths`, `ToolInfo`, `Report`, `Toolchain`, `is_executable`,
    `find_in`, `probe_version`, `locate`, `locate_tool`, `install_hint`, `inspect`,
    `not_a_file`, `preflight` (420-699), with `hanging_version_probe_times_out`.
  - `mod.rs`: module doc, submodule declarations and `pub use` re-exports only.
  - If `process.rs` exceeds 450 lines, move `progress_line` and `clock_seconds` with their
    tests to `tools/progress_line.rs`.
- Acceptance: the split checks; every `pub` and `pub(crate)` item of `tools` resolves at
  the same path (`tools::run`, `tools::run_capture`, `tools::progress_line`, ...).

### T7: Split `src/progress.rs` (827)  (#259)
- Priority: 2
- Depends on: T6
- Owns: `src/progress.rs` → `src/progress/{mod,render,templates,pull}.rs` plus test files;
  its allowlist entry.
- Split:
  - `mod.rs`: `Mode`, `Progress`, its `Drop`, `step_line`, `template_detail`,
    `format_duration` (1-206, 279-294), with the mode, step-line and template tests.
  - `render.rs`: `event_text`, `render_line`, `ffmpeg_percent`, `clock`, `sanitize`,
    `latest_update` (213-278), with the `ffmpeg_stats_*`, `whisper_*`, `latest_update_*`,
    `event_text_*`, `repair_event_text` and `raw_tool_line_is_sanitized` tests.
  - `templates.rs`: `TemplatesReport` (295-354), with the `*templates_bar*` tests.
  - `pull.rs`: `human_size`, `pull_start_line`, `pull_finish_line`, `PullReport` (627-732)
    with the `pull_report_tests` module.
- Acceptance: the split checks; `minuta::progress::{Mode, Progress, TemplatesReport,
  PullReport, human_size, pull_start_line, pull_finish_line, format_duration,
  template_detail}` resolve.

### T8: Split `src/cli.rs` (753)  (#260)
- Priority: 2
- Depends on: T7
- Owns: `src/cli.rs` → `src/cli/{mod,commands,templates}.rs` plus test files; its
  allowlist entry.
- Split:
  - `mod.rs`: the clap types (`Cli`, `Command`, `ModelsCommand`, `pull_name_help`,
    `ConfigCommand`, `TemplatesCommand`, `PullArgs`, `RunArgs`, `PipelineArgs`), `execute`,
    `render_error`, `sanitize`, `single_line` (1-229, 544-558), with the argument-parsing
    and `render_error`/`sanitize`/`single_line` tests.
  - `commands.rs`: `Templates`, `run`, `doctor`, `models_*`, `config_*`, `source_name`
    (230-376, 559-567), with `sizes`, `models_pull_help_*` and `missing_tools_*`.
  - `templates.rs`: `templates_list`, `templates_show`, `pull_origin`,
    `known_templates`, `templates_pull`, `layer_name` (377-543), with the `pull_*` tests.
- Acceptance: the split checks; `src/main.rs` compiles unchanged against `minuta::cli`.

### T9: Split `src/config.rs` (670)  (#261)
- Priority: 3
- Depends on: T8
- Owns: `src/config.rs` → `src/config/{mod,file,endpoint}.rs`; its allowlist entry.
- Split:
  - `mod.rs`: defaults, `ENV_NAMES`, `Source`, `Setting`, `Overrides`, `Config`, path
    functions, `impl Config`, `expand_tilde`, `pick*`, `parse_checked`, `int_setting`.
  - `file.rs`: `FileConfig`, `LlmTable`, `OllamaTable`, `EndpointTable`, `Scalar`,
    `ToolTable`, `BinTable`, `read_file` (142-221, 643-670).
  - `endpoint.rs`: `Endpoint` and its impl, `endpoint`, `is_env_name`, `endpoint_host`
    (105-141, 493-569).
  - If `mod.rs` stays over 450 lines because `impl Config` is 235 lines, move the
    resolution helpers (`pick*`, `parse_checked`, `int_setting`) to `config/pick.rs`.
- Acceptance: the split checks; `minuta::config::{Config, Setting, Source, Overrides,
  Endpoint, default_path, default_path_in, env_snapshot, resolve_path, expand_tilde}`
  resolve.

### T10: Split `src/generate.rs` (605)  (#262)
- Priority: 3
- Depends on: T9
- Owns: `src/generate.rs` → `src/generate/{mod,files}.rs`; its allowlist entry.
- Split: `files.rs` takes the file helpers (`md_path`, `json_path`, `exists`,
  `read_transcript`, `remove_written`, `io_error`, `end_seconds`, `duration`, 542-605). If
  `mod.rs` is still over 450 lines, the JSON-output path of `impl TemplateRun`
  (`JsonOutput` and the methods that use it) moves to `generate/json.rs` as a second
  `impl TemplateRun` block.
- Acceptance: the split checks; `minuta::generate::{output_lang, backend_id, TemplateRun}`
  resolve.

### T11: Split `src/fields/validate.rs` (579)  (#263)
- Priority: 3
- Depends on: T10
- Owns: `src/fields/validate.rs` → `src/fields/validate/{mod,tests}.rs`; its allowlist
  entry.
- Split: the 162 lines of code stay in `mod.rs`. The tests (417 lines) move to
  `validate/tests.rs` (`#[cfg(test)] mod tests;`).
- Acceptance: the split checks; `crate::fields::validate::{parse_object, normalize}` and
  `minuta::fields::validate::parse_answer` resolve.

### T12: Split `src/template/pull_cmd.rs` (525)  (#264)
- Priority: 3
- Depends on: T11
- Owns: `src/template/pull_cmd.rs` → `src/template/pull_cmd/{mod,render}.rs`; its allowlist
  entry. This is the last entry, so the allowlist ends with only its header comment.
- Split: `render.rs` takes the text helpers (`md`, `file_name`, `prompt_note`,
  `result_text`, `table`, `numbered`, 185-293) with their tests. `mod.rs` keeps `Choice`,
  `Options`, `Terminal`, `run`, `parse_choice` and the rest.
- Acceptance: the split checks; `minuta::template::pull_cmd::{Choice, Options, Terminal,
  run, parse_choice}` resolve; the allowlist has no entries and `make file-size` passes.

## Coverage check
| Spec requirement | Task(s) |
|---|---|
| Check fails on a new file over 500 | T1 |
| Check fails on a listed file that grew | T1 |
| Check fails on a listed file within the limit | T1 |
| Check fails on an entry for a missing file | T1 |
| Each rule covered by a test that `make ci` runs | T1 |
| `make ci` runs the check through `file-size` | T1 |
| Size rule in `docs/conventions.md` | T1 |
| Allowlist starts with the eleven files | T1 |
| Every file at most 500 lines, allowlist empty | T2-T12 (T12 removes the last entry) |
| Public `minuta::` paths keep resolving | T2-T12 (`pub use`) |
| No test lost | T2-T12 (test-list comparison) |
| No change under `tests/` | T1-T12 |
| Two commits per split PR, review checklist | T2-T12 |
| Merge only with the user's permission | all |
