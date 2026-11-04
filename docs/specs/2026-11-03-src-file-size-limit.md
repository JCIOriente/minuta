# Source file size limit — Spec

- Status: approved
- Date: 2026-11-03
- Owner: Arnold Gandarillas Castillo
- Related: [conventions](../conventions.md), [core pipeline](2026-10-23-core-pipeline.md)

## TL;DR
- No `.rs` file under `src/` may have more than 500 lines. Eleven files do today, so the
  limit lands first as a ratchet, and a stacked series of refactor PRs brings every file
  under it.
- `make ci` runs the check through a new `file-size` target. It fails on a new file over the
  limit, on a listed file that grew, on a listed file that is now within the limit but still
  listed, and on an entry for a file that does not exist.
- Each split PR keeps behavior unchanged. Its first commit only moves code. Its second commit
  applies the idiom fixes from the review checklist.
- Every `minuta::` path that `tests/` or `src/main.rs` uses keeps resolving. Narrowing `pub`
  to `pub(crate)` is a separate follow-up.
- Each PR in the stack merges only with the user's explicit permission.

## Change in a nutshell
A contributor who pushes a file over the limit sees this in `make ci`:

```text
$ make ci
...
./scripts/check-file-size.sh
src/llm/new_backend.rs: 612 lines, limit 500; split it into modules
file-size: 1 violation
make: *** [file-size] Error 1
```

The files that are over the limit today are listed with their current counts in
`scripts/file-size-allowlist.txt` (excerpt, 2 of 11 entries):

```text
# path                      max lines
src/runner.rs               1651
src/llm/chunk.rs            1383
```

The general rule: every `.rs` file under `src/` has at most 500 lines. A file may stay over
the limit only while it is on the allowlist, and only at or below the count recorded there.
The allowlist shrinks to no entries as the series lands.

## Problem
Eleven files under `src/` are over 500 lines. The largest, `src/runner.rs`, is 1,651 lines
and covers template resolution, preflight, the job folder, staleness and step execution in one
module (its module doc says so: `src/runner.rs:1-2`). Its unit tests alone are 667 lines
(`#[cfg(test)] mod tests` runs from `src/runner.rs:985` to the end of the file). `src/llm/chunk.rs` is 1,383 lines. Nothing stops
a new file from growing the same way. Long files are harder to review, test and change
safely.

## Goals / Success criteria
- `make ci` fails when a `.rs` file under `src/` exceeds 500 lines and is not on the
  allowlist.
- `make ci` fails when an allowlisted file grows above its recorded count.
- `make ci` fails when an allowlisted file has 500 lines or fewer but is still listed, and
  when an allowlist entry names a file that does not exist.
- Each of these four failures, plus the passing case, is covered by a test that `make ci` runs
  (see Testing strategy).
- At the end of the series the allowlist has no entries, and every `.rs` file under `src/` has
  at most 500 lines.
- After each split PR, `make ci` passes on Linux and macOS, and no test was lost. For each
  test binary the number of tests is unchanged, and every test function name from before is
  still present, possibly under a new module path (see Testing strategy).
- No file under `tests/` changes in the series.

## Non-goals
- Narrowing `pub` to `pub(crate)` in `src/lib.rs`. The integration tests use 14 module paths
  and `src/main.rs` uses a 15th (see Interfaces), so narrowing needs changes to the tests and
  its own spec.
- A size limit for `tests/`, `examples/`, `scripts/` or non-Rust files.
- Behavior changes, new features, or fixes for bugs found along the way. Each one goes to a
  GitHub issue, not into a split PR.
- Changing the 500-line value. It is one constant in the script and can change later.
- Merging any PR without the user's explicit permission.

## Users & permissions
| Role | Can | Cannot |
|---|---|---|
| Maintainer (the user) | Approve and merge each PR, one at a time. Add entries to the allowlist or raise them. | n/a |
| Agent working on the series | Open the stacked PRs, run `make ci`, answer review comments, lower or remove allowlist entries for files it split. Move an entry to a renamed path (for example `src/runner.rs` to `src/runner/mod.rs`) with an equal or lower count; this does not count as adding an entry. | Merge a PR without the user's explicit permission. Add an entry for any other file, or raise an entry, without the user's approval in the conversation. |

## Tenancy & localization
n/a. minuta is a local CLI with no accounts, organizations or locales.

## Domain model
These terms are defined in `CONTEXT.md`.
- **Size limit**: 500 lines for each `.rs` file under `src/`. Lines are counted with
  `awk 'END{print NR}'`, so a last line without a trailing newline still counts. `wc -l`
  would drop it.
- **Size allowlist**: `scripts/file-size-allowlist.txt`. One line per over-limit file: its
  path and the most lines it may have.
- **Ratchet**: the rule for allowlisted files. An entry may only go down: when a listed file
  shrinks, its entry is lowered to the new count in the same commit. A file may not grow
  above its entry. Once the file has 500 lines or fewer, its entry is removed.

## Interfaces
- `scripts/check-file-size.sh` takes no arguments. It reads two environment variables:
  - `FILE_SIZE_SRC`: the directory to scan. Default `src`.
  - `FILE_SIZE_ALLOWLIST`: the allowlist file. Default `scripts/file-size-allowlist.txt`.

  Both are resolved against the current directory. `FILE_SIZE_SRC` must be a relative path
  with no leading `./` and no trailing `/` (`src`, or `src/llm` in a fixture). Any other form
  is an error (exit 2), so file paths always print in one form. Allowlist paths are relative
  to the current directory and begin with the value of `FILE_SIZE_SRC` (`src/runner.rs`).
  Files are checked in byte order (`LC_ALL=C sort`), so the violation lines always come in
  the same order.
  The script exits 0 when every rule holds. Otherwise it prints one line per violation to
  stderr, then a summary line, `file-size: 1 violation` or `file-size: <n> violations`, and
  exits 1. The script exits 2, after printing the reason, for a missing or unreadable
  allowlist or a malformed entry. It never treats a bad allowlist as an empty one.
  It runs on the bash 3.2 and BSD tools that ship with macOS, as well as on Linux. That means
  no associative arrays, no GNU-only flags, and no expansion of a possibly empty array under
  `set -u`, because bash 3.2 reports `"${a[@]}"` of an empty array as unbound. An allowlist
  with no entries is the end state of the series, so it must work.
- `scripts/file-size-allowlist.txt`: one entry per line, `<path> <max lines>`, separated by
  one or more spaces or tabs. Blank lines and lines starting with `#` are ignored. An entry is
  malformed (exit 2) if it has more or fewer than two fields, a count that is not a positive
  integer, a count of 500 or less, a path that does not end in `.rs`, a path that does not
  start with `FILE_SIZE_SRC/`, or a path already listed.
- `scripts/check-file-size-test.sh`: the script's tests. It builds fixture trees in a
  temporary directory and runs the check against each one, using the two variables above.
- `Makefile`: a new `file-size` target runs the test script, then the check on the real
  `src/`. `ci` becomes `fmt clippy file-size test`, so local runs and CI (which runs
  `make ci`, `.github/workflows/ci.yml:27`) run the same check.
- Module paths that must keep resolving after every split:
  - used by the integration tests: `minuta::job`, `minuta::error`, `minuta::llm`,
    `minuta::template`, `minuta::steps`, `minuta::tools`, `minuta::transcript`,
    `minuta::fields`, `minuta::config`, `minuta::chapters`, `minuta::models`,
    `minuta::progress`, `minuta::generate`, `minuta::config_init`
  - used by `src/main.rs`: `minuta::cli`

  When a split moves an item that is reachable through one of these paths, the module it
  came from keeps the path with a `pub use` re-export. For example, `minuta::llm::chunk::X`
  still resolves after `X` moves to `llm/chunk/plan.rs`. New submodules are declared with
  private `mod`, never `pub mod`, so a split adds no new public path (no
  `minuta::llm::chunk::plan::X`) for the follow-up to narrow.

## Behavior & edge cases
Each violation names the file and both numbers:
- A new file over the limit: `<path>: <n> lines, limit 500; split it into modules`.
  A file with exactly 500 lines passes, and one with 501 fails.
- A listed file that grew: `<path>: <n> lines, allowlist allows <max>; it may only shrink`.
- A listed file that shrank but is still over the limit is not a violation. The
  entry should still be lowered to the new count in the same commit (Ratchet). A reviewer
  enforces that, not the script, because a stale higher number only lets the file grow back
  up to it.
- A listed file now within the limit: `<path>: <n> lines, within the limit; remove it from
  the allowlist`.
- An entry for a missing file: `<path>: listed in the allowlist but does not exist`.
- Only `.rs` files under `FILE_SIZE_SRC`, including subdirectories, are counted.
- A split that renames a file, such as `src/runner.rs` becoming `src/runner/mod.rs`, moves its
  allowlist entry in the same commit. If the old name is left listed, the check fails because
  that file no longer exists.
- No generated Rust files exist. If one is added later, putting it on the allowlist needs its
  own decision first.

## Data & migrations
None. The allowlist starts with the eleven current files and their current counts.

## Refactor series
The series is stacked: each PR's base is the previous PR's branch.

1. **PR 1, guard.** Adds the check script, its tests, the allowlist with the eleven current
   entries, the `file-size` target, and the size rule in `docs/conventions.md`. No file under
   `src/` changes.
2. **Split PRs, one file or one concern per PR, largest file first:** `runner.rs`,
   `llm/chunk.rs`, `llm/call.rs`, `chapters/validate.rs`, `tools.rs`, `progress.rs`,
   `cli.rs`, `config.rs`, `generate.rs`, `fields/validate.rs`, `template/pull_cmd.rs`. The
   largest files may need more than one PR each. The plan sets the exact splits. Each PR
   lowers or removes its files' allowlist entries. The PR that removes the last entry leaves
   the allowlist with only its header comment, and the check passes with no entries.

Merging: PRs merge in stack order, PR 1 first, each only after the user's explicit permission.
The repo squash-merges, so after a PR merges, the next PR is rebased onto `main`
(`git rebase --onto main <merged branch>`) and retargeted to `main` before it merges.

Within a split PR:
- **Commit 1, move:** code moves to its new modules. Only paths, `use` lines, module
  declarations, `pub use` re-exports and visibility may change. Moving a `#[cfg(test)]`
  module, or splitting one, counts as a move, and so does the indentation change when a
  test module's body leaves `mod tests { }` for its own file. Moved tests import what they
  test from its new module (`use super::*` in the new file, or `use super::pack::*`). A
  re-import that only tests need is `#[cfg(test)]`, because an unused import fails clippy
  under `-D warnings`. These count as `use` lines. No logic changes. The allowlist update
  goes in this commit, so `make ci` passes on it.
- **Commit 2, idiom:** changes from the review checklist below, made only to code the PR
  moved, that leave behavior unchanged. Examples: narrowing visibility that commit 1 widened
  more than needed, replacing a hand-written loop with an equivalent iterator, removing a
  needless clone. A fix that would change behavior, even replacing an `unwrap` that can panic
  with a returned error, is a bug fix. It goes to an issue, not into commit 2. Commit 2 also
  updates the allowlist if line counts changed. If the checklist finds nothing, commit 2 is
  an empty commit (`git commit --allow-empty`) whose message says so, so every PR has the
  same two-commit shape. The repo squash-merges, so the two commits exist only for review on
  the PR. `main` gets one commit per PR.

Review checklist per PR, in order:
1. Correctness: behavior is unchanged. In commit 1,
   `git diff --color-moved=dimmed-zebra --color-moved-ws=allow-indentation-change` shows
   no added or removed logic, only moved blocks plus `use`, `mod`, `pub use` and visibility
   lines. The moved tests assert the same values.
2. Idiom, from the baton Rust guide and `docs/conventions.md`:
   - no `unwrap()` or `expect()` added on fallible paths
   - errors keep their source chain
   - no `#[allow(clippy::...)]` added
   - items are no more visible than they need to be: anything newly made visible for the
     split is `pub(super)` or `pub(crate)`, not `pub`, unless it is reachable through one of
     the public paths above
   - every new file is within the size limit

## Testing strategy
- **Check script (test first):** `scripts/check-file-size-test.sh` is written before the
  check and is shown failing first, because the check does not exist yet. It has one case per
  rule. Each case builds a fixture tree and asserts both the exit code and the exact output
  line:
  - a 500-line file and no allowlist entry: passes
  - a 501-line file and no entry: fails
  - a 501-line file whose last line has no trailing newline: fails (pins the `awk` count)
  - a listed file above its count: fails
  - a listed file below its count but over 500: passes
  - a listed file at 500: fails with the "remove it" message
  - an entry for a missing file: fails
  - two offending files: both are reported, and the summary says 2
  - a nested `.rs` file over the limit: fails; a non-`.rs` file over the limit: passes
  - a missing allowlist file and a malformed entry: exit 2
  - comments and blank lines in the allowlist: ignored
  - an allowlist with only its header comment and a tree within the limit: passes (the end
    state, and the bash 3.2 empty-array case)
  - each malformed-entry form from Interfaces (wrong field count, non-numeric count, count of
    500 or less, non-`.rs` path, path outside `FILE_SIZE_SRC`, duplicate path, tabs as
    separators accepted), and a bad `FILE_SIZE_SRC` form: exit 2

  `make ci` runs it, so the check's behavior is tested on every run. It lives under
  `scripts/`, not `tests/`, which keeps `tests/` unchanged.
- **Each split PR:** `make ci` locally and on the CI matrix (Linux and macOS). The split PRs
  move code and add no behavior, so they add no new tests. The existing tests are the
  safety net that TDD (`requireTdd`) asks for, and the test-loss check below proves none was
  dropped. Before and after the PR, on the same OS, run:

  ```sh
  cargo test --all-features -- --list 2>&1 \
    | awk '/Running|Doc-tests/ {bin=($2=="unittests")?$3:$2}
           /: test$/ {n=$0; sub(/: test$/,"",n); sub(/.*::/,"",n); print bin, n}' \
    | LC_ALL=C sort > tests-<before|after>.txt
  ```

  The two files must be identical. Each line is a test binary (the `Running` header, which
  cargo prints to stderr, hence `2>&1`; for unit tests the source path, `src/lib.rs` or
  `src/main.rs`) plus the final segment of a test name (the function name without its module
  path). On `origin/main` at f9cc674 this gives 1,463 lines on macOS. There are no doc tests,
  whose names would include line numbers. Moving a test changes its module path, so full names are not
  compared. Tests behind `cfg(target_os)` differ between Linux and macOS, so both lists come
  from the same OS. The CI matrix covers the other OS.
- **Behavior:** the integration tests in `tests/` run unchanged. The `runner.rs` PRs also
  touch the interrupt path that `handle_signals` (`src/main.rs:30`) feeds, so their
  descriptions list the interrupt and resume tests that ran.
- **Idiom:** the review checklist above, for each PR.

## Decisions
- **The limit is recorded in `docs/conventions.md`, not an ADR.** It is a project rule that
  is easy to change, and no user depends on it.
- **A ratchet, not a plain limit.** A plain limit would fail until all eleven splits land,
  which would block unrelated work.
- **Public paths stay `pub` in this series.** Narrowing needs changes to the tests, so it is a
  separate decision with its own spec.
- **The script's tests are a shell script under `scripts/`.** `tests/` must not change. A
  Rust test would also have to shell out to the script anyway.
- **New terms** (size limit, size allowlist, ratchet) are added to `CONTEXT.md`.

## FAQ
### Why not clippy's `too_many_lines`?
It counts lines per function (default 100), so it cannot catch a file made of many small
functions. The file size check covers a different problem.

### Why 500 and not 300 or 800?
It is a judgment call, not a measured optimum. 500 sits in the range commonly used for Rust
modules. The files that are clearly too big (1,651 and 1,383 lines) are far past it. Most
files that read comfortably are under it. It is one constant in the script and can change in
one place if the series shows it is wrong.

### Do unit tests inside `src/` count?
Yes. The limit covers every `.rs` file under `src/`, including files that hold only
`#[cfg(test)]` code. A test module moves with the code it tests, into the new submodule's
file or its own `tests.rs` next to it. A test module that would still be over 500 lines is
split by subject too. For example, `runner.rs`'s 667 lines of tests are spread across the
files its code moves to.

## Open questions
None at approval time.

## Implementation status
Updated by the PRs that implement this spec. Plan: [docs/plans/2026-11-03-src-file-size-limit.md](../plans/2026-11-03-src-file-size-limit.md).
- [ ] PR 1: guard (check script and its tests, allowlist, make target, conventions rule)
- [ ] Split PRs: the eleven files, largest first, until the allowlist has no entries
