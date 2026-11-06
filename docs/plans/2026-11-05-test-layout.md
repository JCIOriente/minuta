# Test layout — Implementation Plan

- Spec: [docs/specs/2026-11-05-test-layout.md](../specs/2026-11-05-test-layout.md)
- Date: 2026-11-05
- Status: in backlog

## Overview
T1 lays the shared foundation: `src/test_support/` with the two fakes and one `retry_busy`
helper, and every `#[path]` include pointed at it. Then fourteen move tasks run in parallel,
one per destination module, so no two of them own the same file. Each moves the white-box
tests of one or two `tests/` files into `src/<module>/`, and deletes the source file when it
is empty. The black-box crate comes last, in three steps, because `tests/cli.rs` and
`tests/cli/` cannot coexist (spec, "Interfaces"): T16 renames `tests/cli.rs` and creates
`tests/cli/` from `tests/integration/` and the small binary files. T17 moves the renamed file
in. T18 moves the binary halves that T14 and T15 leave in `tests/templates_pull.rs` and
`tests/config_init.rs`. T19 checks the end state and records it for sub-project B.

Baseline at `1c064a5`: 1,474 tests, counted with
`cargo test --all-features -- --list 2>/dev/null | grep -c ': test$'`.

Common rules for every move task (T2 to T15, T17, T18):
- **Where a test goes.** With the module of the function it calls; with the highest-level
  one when it calls several (spec, "Domain model"). A test file over 500 lines is split by
  behavior into `src/<module>/tests/<topic>.rs`, declared in `src/<module>/tests.rs` (or
  `tests/mod.rs`) with `mod <topic>;`. The module declares `#[cfg(test)] mod tests;`, or
  `#[cfg(all(test, unix))] mod tests;` when the source file was `#![cfg(unix)]` or uses
  `std::os::unix` at the top level.
- **File into directory.** A module that is one file (`src/llm/endpoint.rs`) and gets tests
  becomes `src/llm/endpoint/mod.rs` plus `src/llm/endpoint/tests*.rs`. The move commit
  changes no line of production code.
- **Existing inline tests.** A module that already has an inline
  `#[cfg(test)] mod tests { .. }` (`src/template/pull_cmd/mod.rs:321`,
  `src/tools/process.rs:367`, `src/tools/locate.rs:298`, `src/steps/join.rs:380`) cannot
  also declare a file module named `tests`. In those modules the inline block becomes
  `src/<module>/tests/mod.rs` with its body unchanged (only the `mod tests {` wrapper and one
  indentation level go), and the moved tests go in sibling files declared from that
  `mod.rs`. A unix gate from a former `#![cfg(unix)]` file goes on those sibling `mod` lines
  (`#[cfg(unix)] mod moved;`), never on the whole `tests` module, so the inline tests keep
  the gates they have today. Modules without an inline block declare `#[cfg(test)] mod
  tests;` (or `#[cfg(all(test, unix))]`) as above.
- **No dead helpers.** `cargo clippy --all-targets -D warnings` fails on unused test
  helpers. A helper that no remaining test uses is moved with its tests or deleted, in the
  same PR.
- **Two commits.** First commit: move, with only `use` lines, paths, `#[path]`/
  `include_str!` paths and indentation changed. Second commit: tidy ("move and tidy"):
  renames to the module's naming style, merging duplicate helpers, `super::*` instead of
  `minuta::` paths. No assertion may be weakened.
- **Counts.** The PR states the test count before and after with the command above. They
  match, or the PR lists each test that was merged or split and why.
- **Isolated crates and `tests/cli/` are not touched** except where a task says so.
- **ETXTBSY.** A moved test that writes an executable uses `crate::test_support::retry_busy`
  (T1). Prefer a committed fixture under `tests/fixtures/` when one already exists.
- **Re-exec by name.** A test that re-runs the test binary with `--exact <name>` must use
  the test's full path in the lib test binary (for example
  `tools::locate::tests::child_inspect_uses_path_env`).

## Contract
None. No production behavior changes. The library's public API stays as it is; sub-project
B narrows it.

## Impact
- **Affected call paths / consumers:** none at runtime. In the build, `src/` gains about
  15,000 lines of `#[cfg(test)]` code. They are compiled only by `cargo test` and checked by
  `make file-size`.
- **Data and migrations:** none.
- **Tests and downstream checks that must stay green:** `make ci` on Linux and macOS, with
  `MINUTA_REQUIRE_FFMPEG=1` in CI, so media tests can't skip silently. Every task also checks
  that the test count is preserved.
- **Rollout and rollback:** each task is one PR that reverts cleanly. Nothing depends on the
  old layout at runtime.

## Dependency graph
```
T1 ──> T5, T6, T7, T11, T12, T13, T14, T16
T2, T3, T4, T8, T9, T10, T15 (independent)
T16 ──> T17 ──> T18 <── T14, T15
T2..T18 ──> T19
```
Can start immediately in parallel: T1, T2, T3, T4, T8, T9, T10, T15

## Tasks

### T1: Shared test support in `src/test_support/`  (#286)
- Priority: 1
- Depends on: none
- Owns: `src/test_support/**`, `tests/support/**` (deleted)
- Touches (additive): `src/lib.rs` (`#[cfg(test)] mod test_support;`); the `#[path]` lines in
  `tests/endpoint.rs`, `tests/models.rs`, `tests/ollama.rs`, `tests/pull_core.rs`,
  `tests/proxy_env.rs`, `tests/templates_pull.rs`, `tests/cli.rs`,
  `tests/integration/common.rs`
- Acceptance:
  - [ ] `tests/support/fake_http.rs` and `fake_ollama.rs` are moved unchanged to
    `src/test_support/` (`git mv`). They still use only `std` and `serde_json`, never
    `crate::`, and keep `#![allow(dead_code)]`.
  - [ ] Every `#[path = "support/..."]` (and `"../support/..."`) in `tests/` points at
    `../src/test_support/...` (`../../src/test_support/...` from `tests/integration/`), and
    `tests/support/` no longer exists.
  - [ ] `src/test_support/mod.rs` declares both fakes and `pub(crate) fn retry_busy`, which
    retries an operation while it fails with ETXTBSY ("Text file busy"). It covers both an
    I/O error and an `Error::MissingTools` whose hint carries "Text file busy", like the
    `retry`/`busy_hint` pair in `tests/tools.rs:26-45`.
  - [ ] `src/test_support/mod.rs` has `pub(crate) const FIXTURES: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");` (as `tests/tools.rs:362` does)
    and `pub(crate) fn fixture(rel: &str) -> PathBuf`.
  - [ ] Test count is 1,474 plus the new tests below.
- Tests first: `retry_busy_retries_a_busy_io_error`, `retry_busy_retries_a_busy_missing_tools_hint`,
  `retry_busy_returns_other_errors_at_once`, `retry_busy_gives_up_after_its_attempts`,
  `fixture_points_into_tests_fixtures`
- Notes: the three retry copies are `tests/tools.rs:26`, `tests/media.rs:766` and
  `tests/transcribe.rs:98`. Read all three; `retry_busy` must serve every caller. The copies
  themselves stay until T12/T13 move those files.

### T2: Config tests into `src/config/`  (#287)
- Priority: 2
- Depends on: none
- Owns: `src/config/tests.rs`, `src/config/tests/**`, `tests/config.rs` (deleted)
- Touches (additive): `src/config/mod.rs` (`#[cfg(all(test, unix))] mod tests;`)
- Acceptance:
  - [ ] All 85 tests of `tests/config.rs` run as `config::tests::...`, split by behavior
    into files of at most 500 lines.
  - [ ] `tests/config.rs` is deleted. Test count unchanged.
- Tests first: none new (moved tests). Show the count before and after.
- Notes: `tests/config.rs:6` imports `std::os::unix::ffi::OsStringExt` at the top level,
  hence the unix gate. Helpers `no_env`, `env_with`, `write_config`, `config_err`,
  `assert_key`, `cases` go in `src/config/tests.rs` or a `tests/support.rs`.

### T3: Job tests into `src/job/`  (#288)
- Priority: 2
- Depends on: none
- Owns: `src/job/tests.rs`, `src/job/tests/**`, `tests/job.rs` (deleted)
- Touches (additive): `src/job/mod.rs`
- Acceptance:
  - [ ] All 86 tests of `tests/job.rs` run under `job::tests`, in files of at most 500
    lines. `call_cache_lives_in_minuta` stays with job (it is a layout test).
  - [ ] `tests/job.rs` is deleted. Test count unchanged.
- Tests first: none new.
- Notes: five tests are `#[cfg(unix)]`; keep those gates per test.

### T4: Generate tests into `src/generate/`  (#289)
- Priority: 2
- Depends on: none
- Owns: `src/generate/tests.rs`, `src/generate/tests/**`, `tests/generate.rs` (deleted)
- Touches (additive): `src/generate/mod.rs`
- Acceptance:
  - [ ] All 71 tests of `tests/generate.rs` run under `generate::tests`, in files of at
    most 500 lines.
  - [ ] `tests/generate.rs` is deleted. Test count unchanged.
- Tests first: none new.
- Notes: uses `crate::llm::fake::ScriptedBackend` (stays `pub` and doc-hidden until
  sub-project B) and `tests/fixtures/whisper/class.json`.

### T5: Endpoint tests into `src/llm/endpoint/`  (#294)
- Priority: 2
- Depends on: T1
- Owns: `src/llm/endpoint.rs` -> `src/llm/endpoint/mod.rs` (moved unchanged),
  `src/llm/endpoint/tests*`, `tests/endpoint.rs` (deleted)
- Acceptance:
  - [ ] `src/llm/endpoint.rs` becomes `src/llm/endpoint/mod.rs` with no production line
    changed (`git diff -M` shows a pure rename plus the `mod tests;` line).
  - [ ] All 86 tests run under `llm::endpoint::tests`, using
    `crate::test_support::{fake_http, fake_ollama}`. Files of at most 500 lines.
  - [ ] `tests/endpoint.rs` is deleted. Test count unchanged.
- Tests first: none new.
- Notes: `llm_err`, `no_interrupt` and `schema` are duplicated with `tests/ollama.rs` and
  `schema` also with `src/llm/call/test_support.rs:26`. Merging them across modules is
  optional; if done, the shared copy goes in `src/llm/test_support.rs`, which this task then
  owns, and this task adds `#[cfg(test)] mod test_support;` to `src/llm/mod.rs` (a touch no
  other task makes). T6 depends on nothing here: it keeps its own copies.

### T6: Ollama tests into `src/llm/ollama/`  (#295)
- Priority: 2
- Depends on: T1
- Owns: `src/llm/ollama.rs` -> `src/llm/ollama/mod.rs`, `src/llm/ollama/tests*`,
  `tests/ollama.rs` (deleted)
- Acceptance:
  - [ ] Pure rename of `src/llm/ollama.rs` plus `mod tests;`.
  - [ ] All 46 tests run under `llm::ollama::tests`, using `crate::test_support` fakes.
  - [ ] `tests/ollama.rs` is deleted. Test count unchanged.
- Tests first: none new.

### T7: Models tests into `src/models/`  (#296)
- Priority: 2
- Depends on: T1
- Owns: `src/models.rs` -> `src/models/mod.rs`, `src/models/tests*`, `tests/models.rs`
  (deleted)
- Acceptance:
  - [ ] Pure rename plus `mod tests;`. All 29 tests run under `models::tests`, using
    `crate::test_support::fake_http`.
  - [ ] `tests/models.rs` is deleted. Test count unchanged.
- Tests first: none new.

### T8: Transcript tests into `src/transcript/`  (#290)
- Priority: 3
- Depends on: none
- Owns: `src/transcript.rs` -> `src/transcript/mod.rs`, `src/transcript/tests.rs`,
  `tests/transcript.rs` (deleted)
- Acceptance:
  - [ ] Pure rename plus `mod tests;`. All 13 tests run under `transcript::tests`.
  - [ ] The `include_str!` paths point at the fixtures from the new location, e.g.
    `include_str!("../../tests/fixtures/whisper/sample.json")` in `src/transcript/tests.rs`.
  - [ ] `tests/transcript.rs` is deleted. Test count unchanged.
- Tests first: none new.

### T9: Template parse and discover tests into `src/template/`  (#291)
- Priority: 2
- Depends on: none
- Owns: `src/template/tests.rs`, `src/template/tests/**` (parse and render tests),
  `src/template/discover.rs` -> `src/template/discover/mod.rs`,
  `src/template/discover/tests*`, `tests/templates.rs` (deleted)
- Touches (additive): `src/template/mod.rs` (`#[cfg(test)] mod tests;`)
- Acceptance:
  - [ ] The 57 parse and render tests run under `template::tests`. The 48 discover and
    built-in tests (including `mod symlinks` and `mod hard_links`, still `#[cfg(unix)]`)
    run under `template::discover::tests`. Files of at most 500 lines.
  - [ ] `tests/templates.rs` is deleted. Test count unchanged.
- Tests first: none new.
- Notes: `social_run_creates_no_summary` also calls `template::graph::order`. It goes with
  discover, because that is the higher-level call. `graph.rs` and its inline tests are not
  touched. Do not create `src/template/tests/` files for catalog, pull or pull_cmd: T10,
  T11 and T14 own those modules.

### T10: Catalog tests into `src/template/catalog/`  (#292)
- Priority: 2
- Depends on: none
- Owns: `src/template/catalog.rs` -> `src/template/catalog/mod.rs`,
  `src/template/catalog/tests*`, `tests/pull_catalog.rs` and `tests/templates_catalog.rs`
  (both deleted)
- Acceptance:
  - [ ] Pure rename plus `mod tests;`. All 53 + 16 tests run under
    `template::catalog::tests`, in files of at most 500 lines. The two `#[should_panic]`
    stay.
  - [ ] The repo-content checks of `tests/templates_catalog.rs` (they read `templates/`
    through `CARGO_MANIFEST_DIR`) keep checking the real `templates/` folder.
  - [ ] Both source files are deleted. Test count unchanged.
- Tests first: none new.
- Notes: `entry_text` exists in both source files; merge it.

### T11: Pull tests into `src/template/pull/`  (#297)
- Priority: 2
- Depends on: T1
- Owns: `src/template/pull.rs` -> `src/template/pull/mod.rs`, `src/template/pull/tests*`,
  `tests/pull_core.rs` (deleted)
- Acceptance:
  - [ ] Pure rename plus `mod tests;`. All 79 tests run under `template::pull::tests`, using
    `crate::test_support::fake_http`, in files of at most 500 lines.
  - [ ] `tests/pull_core.rs` is deleted. Test count unchanged.
- Tests first: none new.

### T12: Tools tests into `src/tools/`  (#298)
- Priority: 2
- Depends on: T1
- Owns: `src/tools/process.rs` -> `src/tools/process/mod.rs`, `src/tools/process/tests*`,
  `src/tools/locate.rs` -> `src/tools/locate/mod.rs`, `src/tools/locate/tests*`,
  `tests/tools.rs` (deleted)
- Acceptance:
  - [ ] The 16 process tests run under `tools::process::tests` and the 16 locate tests
    (including the 2 `#[ignore]` children) under `tools::locate::tests`, all
    `#[cfg(unix)]` on their sibling `mod` lines. The 4 inline tests (`process.rs:367`,
    `locate.rs:298`) become `tests/mod.rs` unchanged and keep their current (ungated) `cfg`.
  - [ ] Writing and running executables goes through `crate::test_support::retry_busy`.
    The local `retry`/`busy_hint` copy is gone.
  - [ ] `run_child_test` re-runs the children with their full lib-test names.
    `inspect_falls_back_to_path_env` and `locate_tool_falls_back_to_path_env` still pass,
    and fail when the child's PATH is not honoured (mutation run in the PR).
  - [ ] `tests/tools.rs` is deleted. Test count unchanged.
- Tests first: none new.
- Notes: `FIXTURE_TOOLS` becomes `crate::test_support::fixture("tools")`.

### T13: Media and transcribe tests into `src/steps/`  (#299)
- Priority: 2
- Depends on: T1
- Owns: `src/steps/{probe,join,audio,transcribe,outputs}.rs` -> directory modules with
  `tests*`, `src/steps/tests.rs` (for `run_with_forwards_tool_output`), `tests/media.rs`
  and `tests/transcribe.rs` (both deleted)
- Touches (additive): `src/steps/mod.rs`
- Acceptance:
  - [ ] Each test runs under the step it drives (counts from the survey: probe 6, join 17,
    audio 5, transcribe 7 + 1, outputs 4, `steps::tests` 1). The 9 inline join tests
    (`join.rs:380`) become `src/steps/join/tests/mod.rs` unchanged (common rule "Existing
    inline tests"). `src/steps/join.rs` is 481 lines today, so it becomes a directory.
  - [ ] The `MINUTA_REQUIRE_FFMPEG` skip logic (`ffmpeg_available`) is one helper in
    `src/steps/test_support.rs`.
  - [ ] Writing executables (`fake_whisper`, the interrupt test) goes through
    `crate::test_support::retry_busy`. The local `retry` and `run_step` copies are gone.
  - [ ] `missing_ffmpeg_fails_when_required` and `missing_ffmpeg_skips_when_not_required`
    re-run `steps::probe::tests::probe_reads_streams` by its full name.
  - [ ] Both source files are deleted. Test count unchanged.
- Tests first: none new.
- Notes: one task, because both source files feed `steps::transcribe`.

### T14: Pull command tests into `src/template/pull_cmd/`  (#300)
- Priority: 2
- Depends on: T1
- Owns: `src/template/pull_cmd/tests*`. The library tests in `tests/templates_pull.rs`
  move out, and the file keeps only its 16 binary tests and their helpers.
- Touches (additive): `src/template/pull_cmd/mod.rs`
- Acceptance:
  - [ ] The 54 white-box tests run under `template::pull_cmd::tests`, in files of at most
    500 lines. The 3 inline tests at `mod.rs:321` become
    `src/template/pull_cmd/tests/mod.rs` unchanged.
  - [ ] Helpers the 16 remaining binary tests no longer use (`template_text`, `case`,
    `names`, `Dest`, ...) move with their tests or are deleted.
  - [ ] `tests/templates_pull.rs` keeps exactly the 16 binary tests. Its `catalog_text`
    helper uses `sha2` directly instead of `minuta::template::catalog::sha256_hex`, so the
    file uses no `minuta::` item.
  - [ ] Test count unchanged.
- Tests first: none new.

### T15: Config-init library tests into `src/config_init/`, with a pinned fixture  (#293)
- Priority: 2
- Depends on: none
- Owns: `src/config_init.rs` -> `src/config_init/mod.rs`, `src/config_init/tests.rs`,
  `tests/fixtures/config_init/config.toml` (new). The library tests in
  `tests/config_init.rs` move out, and the binary halves of the mixed tests stay there.
- Acceptance:
  - [ ] Pure rename plus `mod tests;`.
  - [ ] The 5 library-only tests run under `config_init::tests`.
  - [ ] `tests/fixtures/config_init/config.toml` is the exact text `minuta config init`
    writes. `config_init::tests::template_matches_the_committed_fixture` asserts
    `TEMPLATE == include_str!(<fixture>)`.
  - [ ] The 5 binary tests that compare the written file with `TEMPLATE`
    (`config_init_creates_file_and_parent` :109, `config_init_second_run_leaves_template`
    :158, `config_init_force_overwrites` :175, `config_init_force_creates_missing_file` :188,
    `config_init_honours_minuta_config` :221) compare it with
    `tests/fixtures/config_init/config.toml` read from disk instead; the pin test is their
    library half.
  - [ ] `config_init_creates_file_and_parent` also checks `config show` against `defaults()`
    (:124-133, the only other library call in the file). That assertion is split too: the
    black-box half compares `config show` stdout with a second committed fixture
    `tests/fixtures/config_init/show.txt` (the exact output after a fresh `config init`
    under a temp `HOME`); the unit half
    `config_init::tests::defaults_match_the_committed_show_fixture` renders
    `Config::resolve(Overrides::default(), &|_| None, None).entries()` as
    `key = value (default)` / `key = (unset)` and asserts equality with `include_str!` of
    that fixture. Neither half asserts less than the original.
  - [ ] Helpers the remaining binary tests no longer use (`defaults`, `uncommented`,
    `template_keys`, `is_setting`) move with their tests or are deleted.
  - [ ] `tests/config_init.rs` uses no `minuta::` item. The PR lists the split tests (the
    count rises by the number of new library halves plus the pin test).
- Tests first: `template_matches_the_committed_fixture`,
  `defaults_match_the_committed_show_fixture`

### T16: Create `tests/cli/` from `tests/integration/` and the small binary files  (#301)
- Priority: 1
- Depends on: T1
- Owns: `tests/cli/**` (new), `tests/integration/**` (moved), `tests/version.rs`,
  `tests/e2e.rs`, `tests/e2e_llm.rs` (moved), `tests/cli.rs` -> `tests/cli_bin.rs` (renamed
  only), `docs/common-issues.md` (the Avoid line of "Old job folder ...")
- Acceptance:
  - [ ] `tests/cli.rs` is renamed to `tests/cli_bin.rs` with no other change, so
    `tests/cli/` can exist.
  - [ ] `tests/integration/*` move to `tests/cli/` (`main.rs`, `common.rs`, `chapters.rs`,
    `fields.rs`, `layout.rs`, `pipeline.rs`, `templates.rs`), plus `version.rs`, `e2e.rs` and
    `e2e_llm.rs` as modules. `main.rs` gates per module (`#[cfg(unix)] mod pipeline;`), and
    `version` is ungated.
  - [ ] `tests/cli/common.rs` has `spawn_lock()`, held by `Env` across every write of an
    executable and every spawn of the binary. It also has one helper per job path
    (`state_file`, `template_json`, `implicit_md`, `assets_dir`) that spells out the
    documented layout. No file in `tests/cli/` uses a `minuta::` item.
  - [ ] `docs/common-issues.md`: the Avoid line reads as the spec's "Interfaces" section
    gives it.
  - [ ] Test count unchanged.
- Tests first: none new.

### T17: Move `tests/cli_bin.rs` into `tests/cli/`  (#302)
- Priority: 1
- Depends on: T16
- Owns: `tests/cli/*` modules for the former `tests/cli.rs` areas (run, interrupt, force,
  doctor, templates, endpoints, ...), `tests/cli_bin.rs` (deleted)
- Touches: `tests/cli/main.rs` (module lines), `tests/cli/common.rs` (merge the duplicate
  helpers: `stdout`/`stderr`, `tmp_files`, `ollama`/`ollama_with`, `NO_OLLAMA`,
  `FAKE_CLAUDE`/`FAKE_CODEX`, job-state readers, `pid_alive`)
- Acceptance:
  - [ ] All 90 tests run in `tests/cli/`. The new modules this task creates aim for at most
    500 lines each (a readability target; the enforced limit covers `src/` only).
  - [ ] Its `script`/`Env` writing goes through `common::spawn_lock`.
  - [ ] Every use of `minuta::job::layout` is replaced by the `common.rs` path helpers.
  - [ ] `tests/cli_bin.rs` is deleted. Test count unchanged.
- Tests first: none new.
- Notes: the largest task. If it runs long, split it into two PRs in a stack (move into
  modules with `spawn_lock` first, then helper merges and path helpers), not a new plan.

### T18: Move the binary halves of `templates_pull` and `config_init` into `tests/cli/`  (#303)
- Priority: 2
- Depends on: T14, T15, T17
- Owns: `tests/cli/templates_pull.rs`, `tests/cli/config_init.rs` (new),
  `tests/templates_pull.rs` and `tests/config_init.rs` (deleted)
- Touches: `tests/cli/main.rs`, `tests/cli/common.rs`
- Acceptance:
  - [ ] The 16 `templates pull` binary tests and the `config init` binary tests run in
    `tests/cli/`, using `common::Env` (or a documented variant) and `fake_http` included by
    path from `src/test_support/`.
  - [ ] Both source files are deleted. Test count unchanged.
- Tests first: none new.

### T19: Check the end state and record it for sub-project B  (#304)
- Priority: 3
- Depends on: T2-T18
- Owns: `docs/specs/2026-11-05-test-layout.md` (Implementation status only);
  `docs/templates.md:310`, `docs/templates.es.md:316`, `docs/common-issues.md:42`
  (references to moved test files)
- Acceptance:
  - [ ] `grep -rn 'tests/[a-z_]*\.rs' docs/ CONTEXT.md README.md` names only files that
    exist; the three references above name the new `src/.../tests` files.
  - [ ] `ls tests/*.rs` lists exactly `claude.rs`, `codex.rs`, `proxy_env.rs`,
    `worktrees_clean.rs`. `tests/integration/` and `tests/support/` do not exist.
  - [ ] `grep -rn 'minuta::' tests/` lists only the three isolated crates. The spec's
    Implementation status records that list and the `minuta::` paths `src/main.rs` uses.
  - [ ] The final test count is recorded next to the baseline of 1,474, with each
    difference explained by a task's PR.
- Tests first: none (a check task).

## Coverage check
| Spec requirement | Task(s) |
|---|---|
| White-box tests are unit tests in `src/` | T2-T15 |
| Binary tests in one crate `tests/cli/`, no `minuta::` | T16, T17, T18 (T14 and T15 strip `minuta::` from their binary halves) |
| Top-level `tests/` = the four isolated files; no `integration/`, `support/` | T1 (support), T16 (integration), T19 (check) |
| Fakes exist once in `src/test_support/`, included by path | T1 |
| One `retry_busy`; one `spawn_lock` in `tests/cli/common.rs` | T1, T12, T13; T16, T17 |
| Mixed single tests split with a pinned fixture | T15 (config init), T14 (templates pull) |
| `common-issues.md` Avoid line rewritten | T16 |
| `cli` name clash handled | T16 (rename), T17 (delete) |
| Test count preserved per PR | every task |
| `make ci` green incl. the 500-line limit | every task |
| `minuta::` users outside the library recorded for B | T19 |
