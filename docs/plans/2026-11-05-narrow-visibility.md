# Narrow visibility — Implementation Plan

- Spec: [docs/specs/2026-11-05-narrow-visibility.md](../specs/2026-11-05-narrow-visibility.md)
- Date: 2026-11-05
- Status: in backlog

## Overview
Two tasks. T1 holds every judgement call (the facade, test-only items, the dead code the
measurement found) and is green with the modules still public. T2 is the atomic part: private
modules, the lint and the ~330 narrowings it forces, which `make ci` checks mechanically.
Squash merge keeps the two apart in history, so each PR is reviewed on its own.

## Contract
`minuta::testing` (new, doc-hidden) for the three isolated test crates, owned by T1;
`minuta::cli` and `minuta::error` unchanged.

## Impact
- **Affected call paths / consumers:** `src/main.rs` (unchanged imports), `tests/claude.rs`,
  `tests/codex.rs`, `tests/proxy_env.rs` (imports move to `minuta::testing` in T1). No runtime
  behavior change.
- **Data and migrations:** none.
- **Tests and downstream checks that must stay green:** `make ci` on Linux and macOS with
  `MINUTA_REQUIRE_FFMPEG=1`; the test count, except tests of deleted code (listed).
- **Rollout and rollback:** two PRs, each reverts cleanly (T2 first if both).

## Dependency graph
```
T1 ──> T2
```
Can start immediately: T1

## Tasks

### T1: Facade, test-only items, measured dead code (modules still public)  (#328)
- Priority: 2
- Depends on: none
- Owns: `src/testing.rs` (new), `src/lib.rs` (adds `#[doc(hidden)] pub mod testing;` only),
  `tests/claude.rs`, `tests/codex.rs`, `tests/proxy_env.rs` (imports), `src/tools/mod.rs`
  (split line 11), `src/llm/mod.rs` (`fake` under `cfg(test)`), `src/test_support/fake_http.rs`,
  `src/test_support/fake_ollama.rs`, and the files of the measured dead items
- Acceptance:
  - [ ] `src/lib.rs` gains only the `#[doc(hidden)] pub mod testing;` line.
  - [ ] `src/testing.rs` is exactly the facade in the spec's "Domain model" (item re-exports
    plus the `claude`, `codex`, `endpoint`, `ollama`, `models` submodules), with a module doc
    naming its three users. `grep -n 'pub use' src/testing.rs` shows no bare module path.
  - [ ] `grep -rn 'minuta::' tests/` lists only `minuta::error` and `minuta::testing` paths.
  - [ ] The grouped `pub use` at `src/tools/mod.rs:11` is split so `run_capture`/`RunOpts` sit
    apart from `run`/`Output`.
  - [ ] `src/test_support/fake_http.rs` and `fake_ollama.rs` items are `pub(crate)`.
  - [ ] `llm::fake` is `#[cfg(test)] pub(crate) mod fake;` and the "Not cfg(test)" comment is
    gone.
  - [ ] Each dead item measured on `5b2d1ea` is deleted or, when only unit tests use it
    (`Ollama::with_timeout`), moved under `#[cfg(test)]`; `models::pull` stays (used by
    `tests/proxy_env.rs`) unless the test can use `pull_with_progress` without asserting less.
    Each item is listed in the PR with its reason and the `grep` showing no non-test caller.
  - [ ] `make ci` passes; test count unchanged except tests of deleted code (listed).
- Tests first: none new (no behavior change).
- Notes:
  - The measured dead items on `5b2d1ea` (12): `DEFAULT_LLM`, `CallCache::dir`,
    `llm::fake::ScriptedBackend` and its four associated items (handled by `cfg(test)`),
    `Ollama::with_timeout`, `models::default_dir`, `models::default_dir_for`, `models::pull`,
    a `steps::join` associated `new`, the `discover` field `layer`, `Origin::host`, the
    `Output` field `status` (`src/tools/process/mod.rs:31`, no reader). Re-run the build to
    confirm.
  - These items are dead today; rustc just can't see it while their modules are `pub`. Prove
    each with `grep` over `src/` and `tests/`.

### T2: Private modules, `#![warn(unreachable_pub)]`, narrowings  (#329)
- Priority: 2
- Depends on: T1
- Owns: `src/lib.rs`, and every `src/**/*.rs` whose visibility changes
- Acceptance:
  - [ ] `src/lib.rs` starts with `#![warn(unreachable_pub)]` and declares only `pub mod cli;`,
    `pub mod error;` and `#[doc(hidden)] pub mod testing;` as public; every other module is
    private. `Cargo.toml` has no `[lints]` table.
  - [ ] For each type the facade re-exports or that appears in a re-exported signature
    (`OllamaInfo`, `Missing`, `Problem`), every `pub` method or field is used by an isolated
    crate; all others are `pub(crate)` or narrower. The PR lists the used pairs and
    the hand narrowings.
  - [ ] No `dead_code` warning from `cargo build --lib` or `cargo clippy --all-targets`; no new
    `#[allow(dead_code)]` (the three existing ones stay). Any dead item the lint reveals beyond
    T1's list is deleted or moved under `#[cfg(test)]`, listed in the PR with its reason.
  - [ ] Test count unchanged, or reduced only by tests of deleted code (listed).
  - [ ] `make ci` passes; `MINUTA_REQUIRE_FFMPEG=1` locally if ffmpeg is installed.
- Tests first: none new; the compiler and lints are the check.
- Notes:
  - Order: private modules + `#![warn(unreachable_pub)]`, then `cargo fix --lib`, restoring
    test-only imports it deletes (`Source` in `src/job/mod.rs:21`, `Encoder`/`joined_path` in
    `src/steps/mod.rs:8`); then hand narrowing of re-exported types' members; then
    `cargo build --lib` again so `dead_code` judges the newly narrowed members.
  - Use `cargo fix --lib`, not `--all-targets` (it applied inconsistent fixes across the lib
    and lib-test targets: 40 E0364 errors in the measurement). Build with `--all-targets`
    after.

## Coverage check
| Spec requirement | Task(s) |
|---|---|
| Facade of single items, exact contents | T1 |
| Isolated crates import via `minuta::testing` | T1 |
| Fakes `pub(crate)` | T1 |
| `llm::fake` under `cfg(test)` | T1 |
| Measured dead items removed / `cfg(test)`, listed | T1 |
| `models::pull` stays unless `pull_with_progress` suffices | T1 |
| Only `cli`, `error`, `testing` public | T2 |
| Crate-level `unreachable_pub`, no `[lints]` table | T2 |
| Re-exported types' members narrowed by hand | T2 |
| No `dead_code` warnings; three existing allows only | T2 (dead items only the lint reveals) |
| Test count; `make ci` green on Linux and macOS | T1, T2 |
