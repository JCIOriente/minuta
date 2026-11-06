# Test layout — Spec

- Status: approved
- Date: 2026-11-05
- Owner: Arnold Gandarillas Castillo
- Related: [src file size limit](2026-11-03-src-file-size-limit.md) (its follow-up "narrow `pub` to `pub(crate)`"), [conventions](../conventions.md)

## TL;DR
- Tests are sorted by what they drive. A test that calls a library function is a unit test in
  `src/`, next to the module it tests. A test that runs the `minuta` binary is a black-box
  test in one crate, `tests/cli/`.
- The few tests that change process-global state (environment variables) keep a process of
  their own: they stay as top-level files in `tests/`.
- Shared test doubles (`fake_http`, `fake_ollama`) live once, in `src/test_support/`, and the
  `tests/` crates that need them (the isolated crates and `tests/cli/`) include the same files
  by path.
- No behavior change and no test lost: every PR shows the test count before and after.
- This is sub-project A of two. It lets sub-project B (a later spec) make every module except
  `cli` private, narrow `pub` to `pub(crate)` and turn on `unreachable_pub`, so rustc's
  `dead_code` lint can find unused code.

## Change in a nutshell
Today `tests/config.rs` is a crate of its own that calls the library:

```rust
// tests/config.rs (today)
use minuta::config::{Config, Overrides};
#[test]
fn env_overrides_file() { /* Config::resolve(...) */ }
```

After this work the same test sits in `src/config/tests/resolve.rs` (behind
`#[cfg(test)] mod tests;` in `src/config/mod.rs`) and uses `super::*`. Nothing outside the
crate needs `minuta::config` any more.

The resulting tree:

```text
src/
  config/
    mod.rs              #[cfg(test)] mod tests;
    tests.rs            mod resolve; mod ...;  (or tests/mod.rs)
    tests/              unit tests of config, split by behavior, each file <= 500 lines
  test_support/         #[cfg(test)]: fake_http.rs, fake_ollama.rs, fixture paths
tests/
  cli/                  ONE crate: every test that runs the minuta binary
    main.rs
    common.rs
    pipeline.rs, chapters.rs, fields.rs, templates.rs, layout.rs, config_init.rs, ...
  claude.rs             isolated: sets FAKE_CLAUDE_* with set_var
  codex.rs              isolated: sets FAKE_CODEX_* with set_var
  proxy_env.rs          isolated: sets proxy variables for the whole process
  worktrees_clean.rs    tests scripts/worktrees-clean.sh, not the product
  fixtures/             unchanged
```

## Problem
`tests/` has 24 top-level files and `tests/integration/`. Every top-level file is its own
crate. Most of them are white-box tests: they call library functions directly
(`tests/config.rs` has 85 tests on `Config::resolve`, `tests/job.rs` has 86 on `Job`). For
those calls to compile, `src/lib.rs` exports 17 `pub mod`s, and the tests and `src/main.rs`
use 15 of them. While an item is reachable from outside the crate, rustc cannot report it as
dead code, so unused code in `src/` (407 `pub` items) goes unnoticed. The many crates also
link the library and its dependencies once each.

## Goals / Success criteria
- Every test that calls library functions and does not change process-global state is a
  unit test under `src/`, in the module of the function it tests.
- Every test that runs the `minuta` binary is in the single crate `tests/cli/`, and that
  crate uses no `minuta::` item.
- The top-level files in `tests/` are exactly: `claude.rs`, `codex.rs`, `proxy_env.rs`,
  `worktrees_clean.rs`. `tests/integration/` and `tests/support/` no longer exist.
- `fake_http.rs` and `fake_ollama.rs` exist once, in `src/test_support/`.
- The number of tests (count command in "Testing strategy") is the same before and after
  each PR, or the PR states which tests were merged as duplicates or split, and why.
- `make ci` passes, including the 500-line limit for the new files under `src/`.
- After this work, `minuta::` paths outside the library crate are used only by `src/main.rs`
  (`cli`) and the three isolated crates. The list is written in this spec's Implementation status for
  sub-project B.

## Non-goals
- Narrowing `pub`, making modules private, `unreachable_pub`, removing dead code: that is
  sub-project B, with its own spec.
- Changing production code. If a test cannot move without a change in `src/` outside
  `#[cfg(test)]` code, it stays where it is and the case goes to an issue.
- Making weak tests stronger. Gaps found while moving go to issues.
- Switching the test runner (for example to cargo-nextest).

## Users & permissions
n/a (developer tooling; no runtime behavior changes).

## Tenancy & localization
n/a.

## Domain model
Three kinds of test, decided by what the test drives:

| Kind | Where | Drives | May use |
|---|---|---|---|
| Unit test | `src/<module>/tests.rs` or `src/<module>/tests/*.rs` | library functions | anything in the crate (`super::*`, `crate::test_support`) |
| Black-box test | `tests/cli/` (one crate) | the `minuta` binary, files on disk | `assert_cmd`, fixtures, its own helpers; no `minuta::` item |
| Isolated test | top-level `tests/<name>.rs` | library functions or a script, in a process of its own | `minuta::` items; only for tests that change process-global state, or test repo tooling |

Rules:
- A file that mixes kinds is split test by test.
- A single test that both runs the binary and calls a library function is split. The part
  that checks files the binary wrote stays black-box and compares against a committed fixture
  under `tests/fixtures/` (for `config init`, the expected file). One unit test pins the
  library constant to that same fixture with `include_str!`, so the two cannot drift. The
  part that calls the library becomes a unit test. Neither half may assert less than the
  original test did. Today this applies to `tests/config_init.rs` (runs `minuta config init`,
  then calls `Config::resolve` and compares with `config_init::TEMPLATE`) and the binary test
  in `tests/templates_pull.rs`.
- A unit test goes with the function it calls. When it drives several modules, it goes with
  the highest-level one it calls (for example a test that calls `generate::run` and checks
  `job` files goes in `generate`).
- A module that is a single file (`src/foo.rs`) and gets tests becomes `src/foo/mod.rs` plus
  `src/foo/tests.rs`, as the file-size splits did.

## Interfaces
- `src/lib.rs` gains `#[cfg(test)] mod test_support;`. Its items are `pub(crate)`.
- `src/test_support/fake_http.rs` and `fake_ollama.rs` depend only on `std` and `serde_json`,
  as today, and never on `crate::`, so the same files compile inside the library and inside a
  test crate. They keep their `#![allow(dead_code)]` (each includer uses a subset). The
  isolated crates include them with `#[path = "../src/test_support/fake_http.rs"] mod
  fake_http;`, and `tests/cli/common.rs` with
  `#[path = "../../src/test_support/fake_ollama.rs"] pub(crate) mod fake_ollama;`, so there is
  one copy.
- Black-box tests do not use `minuta::job::layout`. They spell out the documented job layout
  (CONTEXT.md, "Job", "Assets folder", "State folder") through one helper per path in
  `tests/cli/common.rs` (`state_file` -> `<job>/.minuta/job.json`, `template_json` ->
  `<job>/assets/<template>.json`, ...). If the layout changes, these tests must fail; that is
  intended. This replaces the Avoid line of "Old job folder shows stale files ..." in
  docs/common-issues.md, which the first PR that touches `tests/cli/` rewrites to: "In `src/`
  and in unit tests, build every job path with `job::layout`. Black-box tests in `tests/cli/`
  spell the documented layout out on purpose through one helper per path in
  `tests/cli/common.rs` (`state_file`, `template_json`, ...), so a layout change fails them."
- `tests/cli.rs` and `tests/cli/main.rs` cannot both exist: Cargo refuses two test targets
  named `cli`. The PR that creates `tests/cli/` moves every test out of `tests/cli.rs` and
  deletes it, or an earlier PR renames `tests/cli.rs` (for example to `tests/cli_bin.rs`).
- `tests/cli/main.rs` gates per module (`#[cfg(unix)] mod pipeline;`), not the whole crate:
  `tests/cli.rs` and `tests/integration/main.rs` are `#![cfg(unix)]` today, while
  `tests/config_init.rs`, `tests/templates_pull.rs` and `tests/version.rs` are not.
- `Cargo.toml` needs no `[[test]]` entries: `tests/cli/main.rs` and the top-level files are
  found by Cargo's default discovery.

## Behavior & edge cases
- **Process-global state.** `set_var`, `remove_var` and `set_current_dir` affect every thread
  in the process. A unit test must not call them. The three files that do (`claude.rs`,
  `codex.rs`, `proxy_env.rs`) stay isolated crates. A test found during the move that needs
  them also stays isolated.
- **ETXTBSY.** Unit tests share one process with every test that spawns a child, and
  `tests/cli/` merges crates that today each had their own process. The conventions rule
  applies: prefer the committed fixtures in `tests/fixtures/bin`. A test that still writes an
  executable uses the one `retry_busy` helper in `src/test_support/` (the three copies of the
  retry helper in `tests/tools.rs`, `tests/media.rs` and `tests/transcribe.rs` merge into
  it). In `tests/cli/`, one `spawn_lock` in `common.rs` is held across every write of an
  executable and every spawn of the binary (the `Env` helper takes it), so the lock also
  covers the former `tests/integration/` tests.
- **Fixture paths.** Unit tests reach fixtures through `env!("CARGO_MANIFEST_DIR")`, so the
  working directory does not matter.
- **Platform gates.** A file-level `#![cfg(unix)]` becomes `#[cfg(all(test, unix))]` on the
  test module. A test that skips when ffmpeg is missing keeps that skip.
- **Duplicate helpers.** Helpers that exist in several test files may be merged into
  `src/test_support/` or the module's test file while moving. Tests may be renamed to the
  module's naming style ("move and tidy"). Assertions are not weakened.

## Data & migrations
None. Git history follows the moves through `git log --follow` on each new file as far as
the content allows. Each PR's description maps old test files to new places.

## Testing strategy
This work changes tests, not behavior, so the proof is:
- The test count before and after, in each PR, always measured with
  `cargo test --all-features -- --list 2>/dev/null | grep -c ': test$'`.
- `make ci` green (fmt, clippy with tests, file-size, all tests) on Linux and macOS CI.
- After the last task: `grep -rn 'minuta::' tests/` lists only the three isolated crates.
- For a moved test that drives a branch, the reviewer may ask for one mutation run to show it
  still fails when the code breaks (conventions, "Process").

## Decisions
- Kinds of test and where they live: this spec. Decided with the maintainer on 2026-11-05:
  split by what the test drives; isolated crates for process-global state; move and tidy.
- Review gate: a Fable reviewer, to whom the maintainer delegated decisions, required five
  precision changes (ETXTBSY rule, where the shared fakes are included, the
  common-issues.md layout rule, split rule for mixed single tests, the `cli` test-name clash),
  all written in above.
- Glossary: "Unit test", "Black-box test", "Isolated test" added to CONTEXT.md.

## FAQ
### Why move white-box tests into `src/` instead of keeping them in `tests/`?
A crate in `tests/` can only call `pub` items. While those items are `pub`, rustc cannot
report them as dead code, which is what sub-project B needs. Unit tests in `src/` can also
test private functions directly.

### Why one crate in `tests/cli/` instead of one file per area?
Each top-level file in `tests/` is linked as its own binary with the library and every
dependency. One crate links once. Cargo itself uses this layout (`tests/testsuite/`).
`tests/integration/` already started it.

### Why not a `test-api` cargo feature that keeps the white-box tests in `tests/`?
It needs a second build configuration, keeps the internal API reachable under that feature,
and a feature left on by mistake hides dead code again.

### Why not cargo-nextest, which runs each test in its own process?
It would let the env-changing tests become unit tests too, but adds a tool to every
contributor's setup, and plain `cargo test` would still race. Three isolated crates cost
less.

### Does `src/` grow?
Yes, by the moved tests: `tests/` has 21,805 lines today, of which about 15,000 move into
`src/` (the isolated crates keep 1,275, and the black-box tests about 5,000). They are compiled only for `cargo test` and
count toward the 500-line limit like any other file.

## Open questions
None.

## Implementation status
Updated by the PRs that implement this spec. Plan:
[docs/plans/2026-11-05-test-layout.md](../plans/2026-11-05-test-layout.md) (#305), tasks
#286-#304.
- [x] Shared test support in `src/test_support/` (#309, task #286)
- [x] White-box tests moved into `src/`: config #306, job #310, generate #307, transcript
      #311, template #312, catalog #319, pull #314, endpoint #316, ollama #317, models #318,
      tools #321, steps #322, pull command #320, config init #313 (tasks #287-#300)
- [x] Black-box crate `tests/cli/`: #323, #324, #325 (tasks #301-#303)
- [x] End state checked and recorded (task #304)

End state (checked by task #304):
- `ls tests/*.rs`: `claude.rs`, `codex.rs`, `proxy_env.rs`, `worktrees_clean.rs`.
  `tests/integration/` and `tests/support/` do not exist. `tests/cli/` is the one black-box
  crate and uses no `minuta::` item.
- Test count: 1,474 at the baseline (`1c064a5`), 1,481 at the end. The difference is +5 new
  `retry_busy`/`fixture` tests (#309) and +2 library halves of split `config init` tests
  (#313). No test was dropped.
- `minuta::` paths used outside the library crate, the starting point for sub-project B:
  - `src/main.rs`: `minuta::cli::{self, Cli}`, `minuta::error::{Error, Result}`
  - `tests/claude.rs`: `minuta::error::Error`, `minuta::llm::call::{call, CallCache,
    Policy}`, `minuta::llm::claude::{check, Claude}`, `minuta::llm::Backend`,
    `minuta::tools::{run_capture, RunOpts}`
  - `tests/codex.rs`: `minuta::error::Error`, `minuta::llm::call::{call, CallCache,
    Policy}`, `minuta::llm::codex::{check, Codex}`, `minuta::llm::Backend`
  - `tests/proxy_env.rs`: `minuta::llm::{endpoint, ollama}`, `minuta::models::{self,
    ModelKind, ModelSpec, Pulled}`, `minuta::template::pull::{fetch_catalog, Origin}`
  - `llm::fake` is still `pub` and `#[doc(hidden)]` (`src/llm/mod.rs`) though only unit
    tests use it now; sub-project B can make it `#[cfg(test)]`.
- Follow-ups filed during the moves: #308 (permission tests pass silently as root), #315
  (test hygiene notes on code that predates the moves).
