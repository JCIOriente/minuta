# Pull curated templates — Implementation Plan

- Spec: docs/specs/2026-10-30-templates-pull.md (ADR-0011)
- Date: 2026-10-30
- Status: in progress (T1-T5 merged; T6 and T7 in progress)

## Overview
Cut along new files so tasks do not collide. A pure core first (catalog, manifest, states), then
the network and write layer on top of it, then the CLI. The repo-side catalog and the persona
briefs are independent and run in parallel with the code. The curated templates come last because
they need the pull command to be verified by hand. No new dependency: `ureq`, `toml`, `sha2` and
`serde` are already in `Cargo.toml`, and the checklist is a numbered prompt on stderr.

## Contract
New public surface: `minuta templates pull` and its flags (T3), `templates/index.toml` `version = 1`
(T1 defines the format, T4 creates the file), and `templates/.pulled.toml` `version = 1` (T1).

## Impact
- Affected call paths: `templates` subcommand enum and dispatch in `src/cli.rs`. Template discovery
  must keep ignoring `.pulled.toml` and `.bak` files (T3 test); `discover.rs` itself is not changed.
- Data and migrations: none. The manifest is created on first pull, so nothing exists to migrate.
- Must stay green: `tests/templates.rs`, `tests/cli.rs`, `tests/config_init.rs`, `make ci`.
- Rollout and rollback: the command is additive. Revert the PRs to remove it. Pulled files are
  ordinary templates and stay usable.

## Dependency graph
```
T1 ──> T2 ──> T3 ──> T7
T1 ──> T4 ──┐
T5 ─────────┴──> T6   (T6 also needs T3)
```
Can start immediately in parallel: T1, T5.

## Tasks

### T1: Catalog, manifest and states  (#160)
- Priority: 0
- Depends on: none
- Owns: `src/template/catalog.rs`, `tests/pull_catalog.rs`
- Touches (read-only or additive): `src/template/mod.rs` (one `pub mod catalog;` line), `src/error.rs` (only if a variant is needed)
- Acceptance:
  - [ ] `Catalog::parse(text)` reads `version = 1` and `[[template]]` entries (`name`, `description`, `platform`, `tags`, `input`, `sha256`); an empty list is valid
  - [ ] it rejects: invalid TOML, `version` other than 1, a name that fails `valid_stem`, a duplicate name, a `sha256` that is not 64 lowercase hex, unknown keys
  - [ ] `Manifest::parse` and `Manifest::to_toml` round-trip; an unreadable or invalid manifest text yields an empty manifest plus a warning string
  - [ ] `state(local_hash, manifest_hash, catalog_hash) -> State` returns exactly the four states of the spec table, including "no manifest entry" -> `Modified`
  - [ ] `select(&Catalog, names | all | filter)` returns entries; filter matches name, platform or tag, case-insensitive; unknown name is a usage error listing available names
- Tests first: `catalog_parses_valid`, `catalog_rejects_bad_version`, `catalog_rejects_duplicate_name`, `catalog_rejects_bad_name`, `catalog_rejects_bad_sha`, `catalog_rejects_unknown_key`, `catalog_empty_is_valid`, `manifest_round_trip`, `manifest_invalid_is_empty_with_warning`, `state_new`, `state_up_to_date`, `state_update_available`, `state_modified_when_local_differs`, `state_modified_when_no_manifest_entry`, `select_by_names`, `select_unknown_name_lists_available`, `select_filter_matches_name_platform_tag`
- Notes: pure functions, no I/O, no network. Follow the parsing style of `src/config.rs` (`deny_unknown_fields`) and `valid_stem` in `src/template/mod.rs`.

### T2: Fetch, plan, verify, write  (#161)
- Priority: 0
- Depends on: T1
- Owns: `src/template/pull.rs`, `tests/pull_core.rs`, `tests/support/fake_http.rs`
- Touches (read-only or additive): `src/template/mod.rs` (one `pub mod pull;` line)
- Acceptance:
  - [ ] `fetch_catalog(base)` and `fetch_file(base, name)` use HTTPS (plain HTTP only for `127.0.0.1` when the base comes from `MINUTA_TEMPLATES_URL`), connect/read timeouts like `src/models.rs`, caps of 1 MiB (catalog) and 256 KiB (file); an over-cap body is an error
  - [ ] `plan(dest, catalog_selection, manifest) -> Vec<Item>` hashes local files and sets each item's state; the destination being a symlink or a non-directory is an error naming the path (`try_exists`)
  - [ ] `apply(plan, fetched, force)` verifies every file (SHA-256 equals the catalog, `Template::parse` ok) before the first write; any failure writes nothing and names the file
  - [ ] writes use temp file + rename; `Modified` items are skipped, or with `force` first saved to `NAME.md.bak` (the item is left untouched if the backup fails)
  - [ ] the manifest is rewritten after the files, with entries changed only for files written in this run
  - [ ] remote strings in errors are passed through `sanitize`
- Tests first: `pull_new_file_written_and_in_manifest`, `pull_update_available_applied_without_force`, `pull_modified_skipped_without_force`, `pull_force_writes_bak_then_overwrites`, `pull_force_backup_failure_keeps_file`, `pull_hash_mismatch_writes_nothing`, `pull_unparsable_file_writes_nothing`, `pull_all_or_nothing_across_files`, `pull_oversized_body_rejected`, `pull_http_error_names_url`, `pull_dest_symlink_rejected`, `pull_manifest_only_updated_for_written`, `pull_invalid_manifest_treated_as_modified`
- Notes: reuse the `ureq` agent setup from `src/models.rs:206`, the temp+rename from `src/generate.rs`, the `Written` idea from `src/config_init.rs:67`. The fake server follows `tests/support/fake_ollama.rs`: `TcpListener`, `Connection: close`, one thread per connection, a map of path -> reply.

### T3: `templates pull` command  (#162)
- Priority: 0
- Depends on: T2
- Owns: `tests/templates_pull.rs`
- Touches (read-only or additive): `src/cli.rs` (`TemplatesCommand::Pull`, dispatch arm, handler beside `templates_show`)
- Acceptance:
  - [ ] flags `[NAME...] --all --filter TEXT --list --to DIR --force --yes --ref REF` with the conflicts of the spec; every flag has a one-line `--help`
  - [ ] `--list` fetches only the catalog, prints the table with states to stderr, writes nothing
  - [ ] the prompt names host, ref, each file with its state, and the destination; `y` proceeds, anything else exits 0 with "cancelled"; `--yes` skips it
  - [ ] without a terminal and without `--yes`: usage error, nothing written, no file fetched
  - [ ] with a terminal and no selection: numbered checklist on stderr (`1,3`, `a` for all, empty to cancel)
  - [ ] nothing to do (all up to date or modified without `--force`): no prompt, a state line per file, exit 0
  - [ ] stdout holds only the written paths, one per line
  - [ ] after a pull, `minuta templates list` shows the template with layer `project`, and a `.pulled.toml` or `.bak` file next to the templates does not appear as invalid
- Tests first: `pull_list_prints_states_and_writes_nothing`, `pull_by_name_with_yes`, `pull_all`, `pull_filter`, `pull_filter_no_match_is_usage_error`, `pull_without_tty_and_yes_refuses`, `pull_to_dir`, `pull_prompt_lists_host_ref_files_dest`, `pull_prompt_no_cancels`, `pull_nothing_to_do_no_prompt`, `pull_stdout_only_paths`, `pull_then_templates_list_shows_project_layer`, `pull_conflicting_flags_rejected`, `pull_ref_flag_changes_url`
- Notes: a pseudo-terminal is not needed; test the checklist parsing as a unit function and the TTY gate through an injectable `is_terminal` bool, as `src/progress.rs:30` does. Use `MINUTA_TEMPLATES_URL` (undocumented) for the base in tests.

### T4: Repo catalog and consistency test  (#163)
- Priority: 1
- Depends on: T1
- Owns: `templates/index.toml`, `tests/templates_catalog.rs`
- Acceptance:
  - [ ] `templates/index.toml` exists with `version = 1` and no entries yet
  - [ ] the test loads `templates/index.toml` and fails when: an entry has no file, a file's SHA-256 differs (the message prints the expected hash and the `shasum -a 256` command), a file does not parse with `Template::parse`, `name` differs from the file stem, or a `templates/*.md` file has no entry
  - [ ] the test passes with the empty catalog and with a fixture catalog in a temp dir
- Tests first: `repo_catalog_parses`, `catalog_check_flags_missing_file`, `catalog_check_flags_hash_mismatch_with_hint`, `catalog_check_flags_unparsable_template`, `catalog_check_flags_name_stem_mismatch`, `catalog_check_flags_file_without_entry`
- Notes: expose the check as a function over a directory so the fixture cases do not touch the real folder.

### T5: Persona briefs  (#164)
- Priority: 1
- Depends on: none
- Owns: `docs/template-briefs/**`
- Acceptance:
  - [ ] one brief per initial template (instagram, youtube, linkedin, tiktok, x-thread, newsletter, blog-post, podcast-notes, meeting-minutes, action-items) with: persona and expertise, audience, platform rules and numeric limits, structure, tone, what to never invent, `input:` chain, and 2 good/bad output examples
  - [ ] a short `README.md` there explains how to use a brief with Codex or another agent
  - [ ] **the owner approves the briefs** before T6 starts (PR review)
- Tests first: none (documentation); `make ci` must stay green
- Notes: limits must match the real platform rules and the existing built-in `limits:` in `src/template/builtin/`.

### T6: Curated templates  (#166)
- Priority: 2
- Depends on: T3, T4, T5
- Owns: `templates/*.md`, `templates/index.toml` (entries)
- Acceptance:
  - [ ] each of the 10 templates is drafted from its approved brief with `codex exec` at high reasoning effort; if the `codex` CLI is missing the PR says Claude subagents drafted them
  - [ ] a second, independent agent reviews each draft against its brief
  - [ ] each template parses, has the catalog entry with the right SHA-256, and `make ci` (the T4 test) passes
  - [ ] each template ran with `minuta run -t NAME` on a sample transcript and the output follows the brief; results are in the PR description
  - [ ] a manual `minuta templates pull` of the set from a branch ref works in a scratch folder
- Tests first: the T4 consistency test is the check; add the sample-run notes to the PR
- Notes: one PR for the batch. Entries are append-only in `index.toml`, so drafts done in parallel by subagents merge cleanly.

### T7: Docs  (#165)
- Priority: 2
- Depends on: T3
- Owns: `README.md` (templates section), `CONTEXT.md`
- Touches (read-only or additive): `docs/install.md` if it lists commands
- Acceptance:
  - [ ] README documents the three `templates` commands, the pull flags, the four states, `.pulled.toml` and `.bak`, `--ref`, and says why templates are downloaded (anyone can contribute, no release needed) and that any file can simply be deleted
  - [ ] README explains how to contribute a template (file + catalog entry + test)
  - [ ] `CONTEXT.md` defines catalog and pull if it defines the other terms
  - [ ] the README customize recipe (`templates show NAME > ...`) is kept and cross-linked
- Tests first: none (documentation); `make ci` must stay green
- Notes: leave `CHANGELOG.md` to the release commit unless the repo convention says otherwise (check `docs/conventions.md`).

## Coverage check
| Spec requirement | Task(s) |
|---|---|
| `index.toml` catalog, fresh on each pull | T1, T2, T4 |
| Selection by names, `--all`, `--filter`, checklist, `--list` | T1, T3 |
| Default `./templates`, `--to`, `--ref` | T2, T3 |
| Confirmation prompt, `--yes`, no terminal refusal | T3 |
| Four states and manifest | T1, T2 |
| Hash + parse check, all-or-nothing, temp+rename, `.bak` | T2 |
| Caps, timeouts, HTTPS, sanitize | T2 |
| Stdout only paths | T3 |
| Repo consistency test | T4 |
| Curated content with Codex, briefs, review | T5, T6 |
| README, contribution guide, glossary | T7 |
| Offline use unchanged | no task touches the runner (checked by `tests/templates.rs` staying green) |
