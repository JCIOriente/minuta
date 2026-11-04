# Source file size limit — Spec

- Status: draft
- Date: 2026-11-03
- Owner: Arnold Gandarillas Castillo
- Related: [conventions](../conventions.md), [core pipeline](2026-10-23-core-pipeline.md)

## TL;DR
- No Rust file under `src/` may exceed 500 lines. Eleven files do today, so the limit lands
  first as a ratchet and a stacked series of refactor PRs brings every file under it.
- `make ci` runs a check. It fails on a new file over the limit, on a listed file that grew,
  and on a listed file that is now under the limit but still listed.
- Each split PR is behavior-preserving. Its first commit only moves code; its second commit
  applies idiom fixes. Each commit can be reviewed on its own.
- Public module paths that `tests/` uses keep working. Narrowing `pub` to `pub(crate)` is out
  of scope and becomes a follow-up spec.
- Each PR in the stack merges only with the user's explicit permission.

## Change in a nutshell
A contributor who pushes a file over the limit sees this in `make ci`:

```text
$ make ci
scripts/check-file-size.sh
error: src/llm/new_backend.rs has 612 lines (limit 500); split it into modules
file-size check failed
```

The known offenders are listed with their current counts in `scripts/file-size-allowlist.txt`:

```text
# path                      max lines
src/runner.rs               1651
src/llm/chunk.rs            1383
```

Generalized: every `.rs` file under `src/` has at most 500 lines. A file may stay over the
limit only while it is on the allowlist, and only at or below the count recorded there. The
allowlist shrinks to empty as the series lands.

## Problem
Eleven files under `src/` are over 500 lines. The largest, `src/runner.rs`, is 1,651 lines
and covers preflight, the job folder, template resolution and step execution in one module
(its module doc says so: `src/runner.rs:1-2`). `src/llm/chunk.rs` is 1,383 lines. Nothing
stops a new file from growing the same way, and long files are harder to review, test and
change safely.

## Goals / Success criteria
- `make ci` fails when a `.rs` file under `src/` exceeds 500 lines and is not on the
  allowlist. Shown by a red run against a fixture before the check is trusted.
- `make ci` fails when an allowlisted file grows above its recorded count.
- `make ci` fails when an allowlisted file is at or under 500 lines but still listed, and
  when an allowlist entry names a file that does not exist.
- At the end of the series the allowlist is empty and every file under `src/` is at most
  500 lines.
- After each PR, `cargo test --all-features` passes on Linux and macOS, and the list of test
  names matches the previous PR's list (`cargo test -- --list`), so no test was lost.
- No file under `tests/` changes in the series.

## Non-goals
- Narrowing `pub` to `pub(crate)` in `src/lib.rs`. Tests use 14 module paths (see Interfaces),
  so this needs its own spec.
- A size limit for `tests/`, `examples/`, or non-Rust files.
- Behavior changes, new features, or bug fixes found along the way. They go to an issue, not
  into a split PR.
- Changing the 500-line value. It is one constant in the script and can change later.
- Merging any PR without the user's explicit permission.

## Users & permissions
| Role | Can | Cannot |
|---|---|---|
| Maintainer | Approve each PR and merge it, one at a time, when the user says so. Edit the allowlist. | Merge a PR the user has not approved. |
| Agent working on the series | Open the stacked PRs, run `make ci`, answer review comments. | Merge, or add a file to the allowlist without the user's approval in the conversation. |

## Tenancy & localization
n/a. minuta is a local CLI with no accounts, organizations or locales.

## Domain model
Terms used here are added to `CONTEXT.md`.
- **Size limit**: 500 lines for each `.rs` file under `src/`, counted as the number of lines
  in the file (the last line counts even without a trailing newline). Counted with
  `awk 'END{print NR}'`, not `wc -l`, so the last line is not dropped.
- **Size allowlist**: `scripts/file-size-allowlist.txt`. One line per over-limit file:
  its path and the most lines it may have.
- **Ratchet**: an allowlisted file may get smaller, and its entry must then be lowered to
  match. It may not grow. Once it is at or under the limit, its entry must be removed.

## Interfaces
- `scripts/check-file-size.sh`: no arguments; reads `src/` and the allowlist. Exits 0 when
  every rule holds, and non-zero after printing one line per violation. An environment
  variable `FILE_SIZE_SRC` overrides the directory, so tests can point it at a fixture.
- `make ci` gains a `file-size` target that runs the script. It is part of `ci` next to
  `fmt`, `clippy` and `test`, so local runs and CI run the same check.
- `scripts/file-size-allowlist.txt`: the format above. Lines starting with `#` are comments.
- Module paths used by the integration tests, which must keep resolving after every split:
  `minuta::job`, `minuta::error`, `minuta::llm`, `minuta::template`, `minuta::steps`,
  `minuta::tools`, `minuta::transcript`, `minuta::fields`, `minuta::config`,
  `minuta::chapters`, `minuta::models`, `minuta::progress`, `minuta::generate`,
  `minuta::config_init`. `src/main.rs` also uses `minuta::cli`. When a split moves an item
  that is reachable through one of these paths, the parent module keeps the path with a
  `pub use` re-export.

## Behavior & edge cases
- A file of exactly 500 lines passes. A file of 501 lines fails.
- A file that is listed and now at or under 500 lines fails with "remove it from the
  allowlist", so the list stays honest.
- A listed file that grows fails and reports the recorded count and the new count.
- An allowlist entry for a file that no longer exists fails.
- Only `.rs` files under `src/`, including subdirectories, are counted.
- A split that renames a file moves its allowlist entry in the same PR. A PR that leaves the
  old name listed fails the check.
- No generated Rust files exist. If one is added later, it needs its own decision before it
  goes on the allowlist.

## Data & migrations
None. The allowlist starts with the eleven current files and their current counts.

## Refactor series
The series is stacked: each PR's base is the previous PR's branch. Merges happen top-down,
each after the user's permission.

1. **PR 1, guard.** Adds the script, the allowlist with the eleven current entries, the
   `file-size` make target, and the size rule in `docs/conventions.md`. No code under `src/`
   changes.
2. **PRs 2 to n, one file or one concern per PR, largest first:** `runner.rs`,
   `llm/chunk.rs`, `llm/call.rs`, `chapters/validate.rs`, `tools.rs`, `progress.rs`,
   `cli.rs`, `config.rs`, `generate.rs`, `fields/validate.rs`, `template/pull_cmd.rs`.
   The largest files probably need several PRs each; the plan sets the split. Each PR
   lowers or removes its allowlist entries.
3. **Last PR, close.** The allowlist is empty and the check passes with it empty.

Within a split PR:
- **Commit 1, move:** the code moves to its new modules. Only paths, `use` lines, module
  declarations and visibility may change. No logic changes.
- **Commit 2, idiom:** fixes from the review checklist below. Commit 2 is separate so a
  reviewer can check the move on its own.

Review checklist per PR, in order:
1. Correctness: the move changed no behavior. The tests in the PR's area still assert the
   same values.
2. Idiom, from the Rust guide in the baton plugin: no `unwrap()` or `expect()` added on
   fallible paths, errors keep their source chain, no `#[allow(clippy::...)]` added, items
   are no more visible than they need to be (`pub(crate)` for crate-internal items), and
   every new file is within the size limit (at most 500 lines).

## Testing strategy
- **Check script:** a red run before the check is trusted. Run the script against a fixture
  directory with a 501-line file, and against a fixture with a 500-line file. The first must
  fail and the second must pass. Then the rules for growth, stale entries and missing files
  are each shown failing once. These red runs go in PR 1's description.
- **Each split PR:** `make ci` on Linux and macOS (the CI matrix), and the test-name list
  unchanged from the previous PR. The Linux-only and macOS-only tests are covered by the
  matrix.
- **Behavior:** the integration tests in `tests/` run unchanged. Runner changes touch signal
  handling and the interrupt path (`handle_signals` in `src/main.rs`), so those tests are
  checked explicitly in the `runner.rs` PRs.
- **Idiom:** the review checklist above, per PR.

## Decisions
- **The limit is recorded in `docs/conventions.md`, not an ADR.** It is a project rule that
  is easy to change and no user depends on it.
- **A ratchet, not a plain limit.** A plain limit would fail until all eleven splits land,
  which would block unrelated work.
- **Public paths stay `pub` in this series.** Narrowing needs changes to the tests and is a
  separate decision with its own spec.
- **New terms** (size limit, size allowlist, ratchet) are added to `CONTEXT.md`.

## FAQ
### Why not clippy's `too_many_lines`?
It counts lines per function (default 100), so it cannot see a file made of many functions.
The file size check covers a different problem.

### Why 500 and not 300 or 800?
500 sits in the range commonly used for Rust modules: the files that are clearly too big
(1,651 and 1,383 lines) are well past it, and most files that are readable are under it. It
is a judgment call, not a measured optimum. It is one constant in the script and can change
in one place if the series shows it is wrong.

## Open questions
None at approval time.

## Implementation status
Updated by the PRs that implement this spec (link the plan and issues once they exist).
- [ ] PR 1: guard (script, allowlist, make target, conventions rule)
- [ ] PRs 2 to n: split the eleven files, largest first
- [ ] Last PR: empty allowlist, check passes
