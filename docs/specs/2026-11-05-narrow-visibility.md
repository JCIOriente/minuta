# Narrow visibility — Spec

- Status: approved
- Date: 2026-11-05
- Owner: Arnold Gandarillas Castillo
- Related: [test layout](2026-11-05-test-layout.md) (sub-project A, done: #286-#304), [src file size limit](2026-11-03-src-file-size-limit.md) (its follow-up "narrow `pub` to `pub(crate)`")

## TL;DR
- Only what something outside the library uses stays public: `cli` and `error` for
  `src/main.rs`, plus a small `#[doc(hidden)] pub mod testing` facade that re-exports single
  items for the three isolated test crates. Every other module becomes private and every
  other item `pub(crate)` or narrower.
- `src/lib.rs` starts with `#![warn(unreachable_pub)]`, so `make ci` (clippy `--all-targets -D
  warnings`, Makefile:10) rejects a new `pub` in the library that nothing outside the crate
  can reach. The lint is a crate attribute, not a `[lints]` table in `Cargo.toml`, so
  `tests/cli/` is not asked to narrow its helpers.
- With items no longer exported, rustc's `dead_code` lint reports what nothing uses. Each such
  item is deleted, or moved under `#[cfg(test)]` when only tests use it.
- No behavior change. The binary and its tests behave exactly as before.

## Change in a nutshell
Today `src/lib.rs` is 17 `pub mod` lines, and rustc treats every `pub` item in them as API,
so it never reports one as unused. After this work:

```rust
// src/lib.rs
#![warn(unreachable_pub)]
pub mod cli;
pub mod error;
#[doc(hidden)]
pub mod testing; // single-item re-exports for tests/claude.rs, tests/codex.rs, tests/proxy_env.rs

mod chapters;
mod config;
// ... every other module private
```

A measurement on `5b2d1ea` (all modules private except `cli` and `error`, `cargo fix` for
`unreachable_pub`) narrowed about 330 `pub` items in about 50 files and left 12 dead-code
warnings, for example `llm::fake::ScriptedBackend` (used only by unit tests), `models::pull`
(used only by `tests/proxy_env.rs`), `DEFAULT_LLM`, `Ollama::with_timeout`, `CallCache::dir`
and `Origin::host`.

## Problem
Sub-project A moved every white-box test into `src/`, so `tests/` no longer needs most of the
library. But all 17 modules are still `pub`, so rustc can't see that an item is unused.
Unused code costs review and maintenance time and hides the real API.

## Goals / Success criteria
- `src/lib.rs` declares `pub mod cli;`, `pub mod error;` and `#[doc(hidden)] pub mod
  testing;`; every other module is private.
- `src/testing.rs` re-exports exactly the items listed under "Domain model" and no module of
  the crate (`grep -n 'pub use' src/testing.rs` shows no bare module path). Its module doc
  says it exists only for `tests/claude.rs`, `tests/codex.rs` and `tests/proxy_env.rs`.
  `grep -rn 'minuta::' tests/` lists only `minuta::error` and `minuta::testing` paths.
- For every type re-exported by `testing`, each `pub` method or field is called from
  `tests/claude.rs`, `tests/codex.rs` or `tests/proxy_env.rs` (the PR lists the pairs); all
  others are `pub(crate)` or narrower.
- `src/lib.rs` has `#![warn(unreachable_pub)]` and `Cargo.toml` has no `[lints]` table;
  `cargo clippy --all-targets --all-features -- -D warnings` passes. The items of
  `src/test_support/fake_http.rs` and `fake_ollama.rs` are `pub(crate)` (the lint also runs
  on the library's test target, where their `pub` is unreachable; `pub(crate)` compiles
  unchanged where the isolated crates and `tests/cli/common.rs` include the same files by
  `#[path]`).
- `cargo build --lib` and `cargo clippy --all-targets` report no `dead_code` warning and the
  crate has no new `#[allow(dead_code)]`. The three existing ones stay: the file-level ones on
  `src/test_support/fake_http.rs` and `fake_ollama.rs` (spec A) and the field-level one on
  `Envelope::version` in `src/chapters/render.rs:47`, which serde needs for
  `deny_unknown_fields`.
- Every removed item is listed in its PR with the reason (unused, or test-only and moved under
  `#[cfg(test)]`).
- Test count unchanged except for tests of deleted code, each listed in the PR.
- `make ci` green on Linux and macOS.

## Non-goals
- Changing behavior, CLI output or error messages.
- Making the isolated crates unit tests (spec A keeps them isolated by design).
- Reorganizing modules beyond visibility.
- Public API documentation for crates.io: the library is internal.

## Users & permissions
n/a (no runtime change).

## Tenancy & localization
n/a.

## Domain model
- **Public surface.** `cli` and `error`, used by `src/main.rs` (`error` stays public because
  `main.rs` uses it, and the isolated crates keep `use minuta::error::Error`); `testing`, used
  by the three isolated test crates. Nothing else.
- **`testing` facade.** `src/testing.rs` re-exports items, never a module of the crate. Its
  contents are exactly: `pub use crate::llm::call::{call, CallCache, Policy}; pub use
  crate::llm::Backend; pub use crate::template::pull::{fetch_catalog, Origin}; pub use
  crate::tools::{run_capture, RunOpts};` and submodules that keep today's call sites readable
  and avoid the `check` name collisions: `pub mod claude { pub use crate::llm::claude::{check,
  Claude}; }`, `pub mod codex { pub use crate::llm::codex::{check, Codex}; }`, `pub mod
  endpoint { pub use crate::llm::endpoint::check; }`, `pub mod ollama { pub use
  crate::llm::ollama::check; }`, `pub mod models { pub use crate::models::{pull, ModelKind,
  ModelSpec, Pulled}; }`. `error::Error` is not in the facade: `minuta::error` stays public.
  The isolated crates then import `minuta::testing::{call, CallCache, Policy}`,
  `minuta::testing::claude::{check, Claude}`, `minuta::testing::Backend`,
  `minuta::testing::{run_capture, RunOpts}`, `minuta::testing::codex::{check, Codex}`,
  `minuta::testing::{endpoint, ollama}`, `minuta::testing::models::{self, ModelKind,
  ModelSpec, Pulled}` and `minuta::testing::{fetch_catalog, Origin}`.
- **Visibility inside the crate.** Default to the narrowest that compiles: private, then
  `pub(super)`, then `pub(crate)` (conventions, "A child module already sees every item of its
  parent module").

## Interfaces
- `minuta::cli` and `minuta::error`: unchanged for `src/main.rs`.
- `minuta::testing`: new, doc-hidden; replaces direct `minuta::llm::...`, `minuta::models::...`,
  `minuta::template::pull::...` and `minuta::tools::...` paths in the isolated crates.

## Behavior & edge cases
- **Item used only by an isolated crate** (for example `models::pull`): it stays and is
  re-exported through `testing`, with a comment naming its test user, unless the test can use
  the production entry point instead (`models::pull_with_progress`) without asserting less;
  then the item is deleted.
- **Method or field of a type the facade re-exports** (`CallCache`, `Origin`, `Claude`,
  `Codex`, `RunOpts`, `ModelSpec`, `Policy`, `Pulled`, `ModelKind`): `pub` keeps it
  reachable, so neither lint reports it. Only the methods and fields an isolated crate uses
  stay `pub`; every other one becomes `pub(crate)` by hand (for example `CallCache::dir`,
  `Origin::host`), then `cargo build --lib` is run again so `dead_code` can judge them. The PR
  lists these hand narrowings.
- **Item used only by unit tests**: `#[cfg(test)]`, or moved to the module's tests or
  `src/test_support/`. `llm::fake` becomes `#[cfg(test)] pub(crate) mod fake;` (all its users
  are under `#[cfg(test)]`), and the "Not cfg(test): integration tests need it" comment at
  `src/llm/mod.rs:14` goes.
- **Truly unused item**: deleted, with its tests if any (listed in the PR).
- **`#[allow(dead_code)]`**: not added. A field read only by `Debug` output counts as unused
  and is removed or used. A field that a wire format requires (`Envelope::version`) keeps its
  existing allow with the comment that says why.

## Data & migrations
None.

## Testing strategy
- `make ci` (fmt, clippy `--all-targets -D warnings`, which includes the crate-level
  `unreachable_pub`, file-size, all tests) on Linux and macOS, plus `MINUTA_REQUIRE_FFMPEG=1`
  in CI as today.
- `cargo fix` is a starting point, not the mechanism. It rewrites a grouped `pub use a::{x,
  y}` as a whole, so a group that mixes facade items with unreachable ones
  (`src/tools/mod.rs:11` holds `run_capture`/`RunOpts` next to `run`/`Output`) is split by hand
  first, or the facade fails with E0364. `cargo fix --lib` also deletes imports only tests use
  (`Source` in `src/job/mod.rs:21`, `Encoder`/`joined_path` in `src/steps/mod.rs:8`), and
  `cargo fix --all-targets` applies inconsistent fixes across the lib and lib-test targets.
  Use `cargo fix --lib`, restore test-only imports, then build with `--all-targets`.
- The PR that deletes code lists each item and why; reviewers check no deleted item had a
  caller outside `#[cfg(test)]`.
- Test count before and after, with the same command as spec A.

## Decisions
- Public surface `cli` + `error` + doc-hidden `testing` facade of single items: this spec (see
  FAQ). `error` stays public, unlike spec A's TL;DR wording ("every module except `cli`"),
  because `src/main.rs` uses it.
- Review gate: a Fable reviewer, to whom the maintainer delegated decisions, confirmed the
  facade design and required five precision changes (item-only re-exports; hand narrowing of
  re-exported types' members; crate-level lint instead of a `[lints]` table; the third
  existing `allow(dead_code)`; corrected item names and counts), all written in above.
- No new glossary terms.

## FAQ
### Why a `testing` facade instead of keeping `llm`, `models`, `template` and `tools` `pub`?
Keeping those four modules public keeps every `pub` item in them invisible to `dead_code`:
the measurement found 1 dead item with them public and 12 with them private. The facade
exports only the items the isolated crates use.

### Why re-export items and not modules?
A re-exported module keeps every `pub` item inside it reachable, so neither lint reports
them. With `pub use crate::llm::{endpoint, ollama}` the measurement kept `Ollama::new`,
`Ollama::with_timeout`, `endpoint::connect` and others public and silent.

### Why not move the isolated tests into `src/` too?
They set environment variables for the whole process (spec A, "Behavior & edge cases"). In
the shared unit-test binary that races with every other test.

### Why `unreachable_pub` as a lint, not a one-time cleanup?
Without it, new code drifts back to `pub` and dead code hides again. The lint makes the rule
mechanical.

### Why a crate attribute and not `[lints.rust]` in `Cargo.toml`?
A `[lints]` table applies to every target of the package, including `tests/cli/` and the
`#[path]` copies of the fakes, which `make ci` would reject (486 warnings in the measurement).
The rule is about the library's API.

## Open questions
None.

## Implementation status
Updated by the PRs that implement this spec. Plan:
[docs/plans/2026-11-05-narrow-visibility.md](../plans/2026-11-05-narrow-visibility.md) (#330),
tasks #328, #329.
- [x] T1: `testing` facade, test-only items under `cfg(test)`, measured dead code removed
      (#331, task #328)
- [x] T2: private modules, `#![warn(unreachable_pub)]`, 347 narrowings, facade members
      narrowed by hand (#332, task #329)

End state: only `cli`, `error` and the doc-hidden `testing` facade are public;
`cargo build --lib` reports no `dead_code`; the three existing `allow(dead_code)` remain.
Test count 1,481 -> 1,478 (deleted with their code: `default_llm_parses`,
`default_dir_is_minuta_models`, `origin_github_host`). The facade's `models` submodule
re-exports `pull_with_progress` instead of `pull` (Behavior & edge cases).
