# One home folder and `minuta config init` — Implementation Plan

- Spec: docs/specs/2026-10-29-config-home-and-init.md
- Date: 2026-10-29
- Status: in backlog

## Overview
Four tasks. T1 (config path) and T2 (templates path) touch different files and can run in
parallel. T3 (`config init`) needs T1 because it writes to the default config path. T4 (docs)
closes the work and needs all three so the README describes what shipped. The config key list
lives in `src/config.rs` and T3's template is checked against it, so T3 does not duplicate it.

## Contract
none. No HTTP API. The CLI gains `minuta config init [--force]`; T3 owns its help text and T4
documents it.

## Impact (risky changes only)
- Affected call paths / consumers: every command that loads config (`Config::load`), template discovery (`Dirs::standard`, used by `run`, `templates list|show`), `minuta config path|show`.
- Data and migrations: no file is moved, copied or deleted. Users with `~/.config/minuta/config.toml` or `~/.config/minuta/templates/*.md` will see those files ignored after the upgrade. This is accepted in the spec; T4 writes the migration note in the README and the release notes.
- Tests and downstream checks that must stay green: `tests/config.rs`, `tests/templates.rs`, `tests/cli.rs`, `tests/e2e.rs`, `tests/integration/*`. Each suite that writes a config or template into a temp home must point at `~/.minuta/` now; T1 and T2 update those.
- Rollout and rollback: ships in the next release. Rollback is a revert of the merge commits. No user data changes, because nothing is moved.

## Dependency graph
```
T1 ──> T3 ──> T4
T2 ────────────> T4
```
Can start immediately in parallel: T1, T2

## Tasks

### T1: Config file default path under `~/.minuta`  (#149)
- Priority: 0
- Depends on: none
- Owns: `src/config.rs`, `tests/config.rs`
- Touches (read-only or additive): none
- Acceptance:
  - [ ] `default_path()` returns `<home>/.minuta/config.toml`
  - [ ] `minuta config path` prints that path when `MINUTA_CONFIG` is unset
  - [ ] `MINUTA_CONFIG` still replaces the default path and must exist (existing rule)
  - [ ] A config at `<home>/.config/minuta/config.toml` is not read
  - [ ] The default-path code takes the home directory as a parameter so tests need no real home
- Tests first: `default_path_is_under_home_minuta`, `default_path_ignores_old_config_location`, `resolve_path_env_beats_default`, `missing_default_file_yields_defaults`
- Notes: `default_path()` currently calls `directories::BaseDirs` directly. Add `default_path_in(home: &Path) -> PathBuf` and have `default_path()` call it. Only the path changes; `Config::resolve` precedence is untouched.

### T2: User templates folder under `~/.minuta/templates`  (#150)
- Priority: 0
- Depends on: none
- Owns: `src/template/discover.rs`, `tests/templates.rs`
- Touches (read-only or additive): none
- Acceptance:
  - [ ] `Dirs::standard().user` is `<home>/.minuta/templates`
  - [ ] A template at `<home>/.config/minuta/templates/x.md` is not discovered
  - [ ] Layer order is unchanged: built-in, user, project (later wins by name)
  - [ ] Project `./templates` still overrides user templates of the same name
- Tests first: `user_templates_dir_is_under_home_minuta`, `old_user_templates_dir_is_ignored`, `user_layer_overridden_by_project_layer`
- Notes: `Dirs::standard` calls `BaseDirs` for the home. Add `Dirs::for_home(home: &Path, cwd: &Path)` and have `standard()` call it, so the test passes a temp home. Update the doc comment on `Dirs::user`.

### T3: `minuta config init [--force]`  (#151)
- Priority: 1
- Depends on: T1
- Owns: `src/config_init.rs` (new), `tests/config_init.rs` (new)
- Touches (read-only or additive): `src/cli.rs` (add the `Init { force }` variant to `ConfigCommand` and one match arm; no other edits), `src/lib.rs` (one `mod` line)
- Acceptance:
  - [ ] `minuta config init` creates the default config path and its parent folder, then prints the path it wrote
  - [ ] When the file exists, nothing changes and the output says so; exit 0
  - [ ] `--force` overwrites an existing file
  - [ ] `init` does not read or parse the existing file, so a malformed one does not block it
  - [ ] With `MINUTA_CONFIG` set, `init` writes to that path
  - [ ] The written template lists every key that `Config::entries()` reports, each commented out with its default value
  - [ ] The written template parses as TOML once the lines are uncommented
  - [ ] After `init`, `minuta config show` reports every value as `(default)` or `(unset)`
  - [ ] I/O failure exits 1 with the path in the message
- Tests first: `config_init_creates_file_and_parent`, `config_init_leaves_existing_file`, `config_init_force_overwrites`, `config_init_ignores_malformed_existing_file`, `config_init_honours_minuta_config`, `config_init_template_covers_every_key`, `config_init_template_parses_when_uncommented`
- Notes: keep the template text as a `const &str` in `src/config_init.rs`, next to the code, so `config_init_template_covers_every_key` can compare it with `Config::entries()`. The `[whisper]` section header is the only one not commented out. Follow the existing `config_path()` / `config_show()` functions in `src/cli.rs` for the handler shape. Use `std::fs::create_dir_all` for the parent and `OpenOptions::create_new` unless `--force`.

### T4: Docs for the new locations and `config init`  (#152)
- Priority: 2
- Depends on: T1, T2, T3
- Owns: `README.md`, `docs/install.md`, `CONTEXT.md`, `docs/common-issues.md`
- Touches (read-only or additive): `docs/specs/2026-10-29-config-home-and-init.md` (Implementation status checklist only)
- Acceptance:
  - [ ] README config path, templates path and the `mkdir`/`printf` setup steps use `~/.minuta/`
  - [ ] README table of commands lists `minuta config init [--force]`
  - [ ] README has a short "Upgrading" note: move `~/.config/minuta/templates/` to `~/.minuta/templates/`, and copy `~/.config/minuta/config.toml` to `~/.minuta/config.toml`
  - [ ] `CONTEXT.md` glossary: "Home folder" added; "Template" layer text updated
  - [ ] `docs/common-issues.md`: an entry for "config changes ignored after upgrade" pointing to the note
  - [ ] Spec implementation checklist ticked with PR numbers
- Tests first: none (docs). Check with `grep -rn '\.config/minuta' README.md docs CONTEXT.md` returning only the upgrade note and `MINUTA_CONFIG` references.
- Notes: the README's config section currently says the file is `~/.config/minuta/config.toml`; that line changes. Do not touch `docs/specs/*` other than the one spec.

## Coverage check
| Spec requirement | Task(s) |
|---|---|
| Config default path `~/.minuta/config.toml`; `config path` prints it | T1 |
| Templates default folder `~/.minuta/templates/`; layers unchanged | T2 |
| `config init` writes commented template, creates folder, prints path | T3 |
| `config init` does not overwrite; `--force` overwrites | T3 |
| `config init` ignores malformed file | T3 |
| `MINUTA_CONFIG` still overrides; `init` honours it | T1, T3 |
| Template lists every key, parses when uncommented | T3 |
| No migration; old files ignored | T1, T2 (tests), T4 (note) |
| Breaking change for templates documented | T4 |
| Testing: unit and integration per spec | T1, T2, T3 |
