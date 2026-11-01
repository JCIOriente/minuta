# Codex backend — Implementation Plan

- Spec: docs/specs/2026-11-01-codex-backend.md
- Date: 2026-11-01
- Status: in backlog

## Overview
Cut along files so the early tasks run in parallel. Three independent foundations first: the
backend id (T1), the shared subprocess helpers pulled out of `claude.rs` (T2) and the
`codex.bin` config key (T4). The `Codex` backend builds on T1 and T2 (T3). Wiring into the
runner, `doctor` and the notice needs all of them (T5). Docs come last (T6). No new dependency.

## Contract
New public surface: backend ids `codex` and `codex:<model>` (T1), config key `codex.bin` and
env `MINUTA_CODEX_BIN` (T4). No file formats change.

## Impact
- Affected call paths: `BackendId` is matched in `src/llm/mod.rs` and `src/runner.rs` (`resolve`
  at ~345, `llm_status` at ~416); the compiler flags every non-exhaustive match once T1
  adds the variant, so T1 adds a temporary arm in the runner that returns a usage error (see T1 notes).
  T2 moves code that `claude` uses, so `tests/claude.rs` must stay green unchanged.
- Data and migrations: none. Call-cache keys already include the backend id.
- Must stay green: `tests/claude.rs`, `tests/ollama.rs`, `tests/config.rs`, `tests/config_init.rs`,
  `tests/cli.rs`, `make ci`.
- Rollout and rollback: additive. Revert the PRs to remove it.

## Dependency graph
```
T1 ──┐
T2 ──┼──> T3 ──> T5 ──> T6
T4 ──┴────────────┘   (T5 needs T1, T3, T4)
```
Can start immediately in parallel: T1, T2, T4.

## Tasks

### T1: Codex backend id and provider labels  (#203)
- Priority: 0
- Depends on: none
- Owns: `src/llm/mod.rs`
- Touches (read-only or additive): `src/runner.rs` (only the new match arms the compiler requires, returning an `Error::Usage` "codex backend is not wired yet"), `src/cli.rs` (the `--llm` help text at line 150)
- Acceptance:
  - [ ] `"codex"` and `"codex:gpt-5"` parse to `BackendId::Codex { model: None | Some }` and display back unchanged
  - [ ] `"codex:"`, `"codex:-x"` and `"codex:a b"` are rejected by the existing rules; the "valid forms" text lists `codex, codex:<model>`
  - [ ] `sends_off_machine()` is true for codex
  - [ ] `BackendId::provider()` returns `Some("Anthropic")` for claude, `Some("OpenAI")` for codex, `None` for ollama
  - [ ] `--llm` help mentions `codex`; `cargo build` is warning-free
- Tests first: `backend_id_parses_codex`, `backend_id_parses_codex_model`, `backend_id_rejects_invalid_codex`, `backend_id_display_round_trips` (extended), `codex_sends_off_machine`, `provider_names`
- Notes: follow the existing tests in `src/llm/mod.rs`. Until T5 the runner arms return a usage error ("codex backend is not wired yet"); T5 replaces them.

### T2: Share the subprocess helpers  (#204)
- Priority: 0
- Depends on: none
- Owns: `src/llm/subprocess.rs` (new), `src/llm/claude.rs`
- Touches (read-only or additive): `src/llm/mod.rs` (one `mod subprocess;` line; T1 owns the file, so merge the one-line conflict)
- Acceptance:
  - [ ] `ScratchDir` (parameterised by a name prefix), `SetOnDrop` and `run_interruptible` live in `subprocess.rs` and `claude.rs` uses them
  - [ ] no behavior change: `tests/claude.rs` and the unit tests in `claude.rs` pass unchanged; the scratch dir name still starts with `minuta-claude-`
  - [ ] no duplicated copy of these items remains in `claude.rs`
- Tests first: `scratch_dir_is_private_and_removed_on_drop` (unit, moved or added in `subprocess.rs`), `scratch_dir_prefix_is_used`
- Notes: pure refactor; do not touch `argv` or `reply` logic in `claude.rs`.

### T3: The Codex backend  (#206)
- Priority: 0
- Depends on: T1, T2
- Owns: `src/llm/codex.rs`, `tests/codex.rs`, `tests/fixtures/bin/codex`
- Touches (read-only or additive): `src/llm/mod.rs` (one `pub mod codex;` line)
- Acceptance:
  - [ ] `codex::check(bin)` finds the binary and version (`codex --version`) or returns `Missing` with an install hint
  - [ ] argv is `exec --ephemeral --skip-git-repo-check --ignore-user-config --ignore-rules -s read-only`, then `--model=<m>` only when a model is set, then `-c developer_instructions=<TOML string>`, then `-` (prompt on stdin); a system prompt with quotes, backslashes and newlines round-trips through the TOML string
  - [ ] stdin is the user message; cwd is an empty temp directory that is removed afterwards
  - [ ] id is `codex` or `codex:<model>`; `context_tokens()` is 128 000
  - [ ] non-zero exit → transient `Error::Llm` carrying stderr (or "exited with status N"); empty stdout → transient "codex returned no output"; signal death with the interrupt flag → `Error::Interrupted`, without it → transient
- Tests first: `codex_argv_without_model`, `codex_argv_with_model`, `codex_system_prompt_is_toml_escaped`, `codex_sends_user_on_stdin_from_empty_cwd`, `codex_nonzero_exit_is_transient_with_stderr`, `codex_empty_output_is_transient`, `codex_interrupt_returns_interrupted`, `codex_signal_without_interrupt_is_transient`, `codex_check_missing_has_hint`, `codex_check_reports_version`
- Notes: mirror `src/llm/claude.rs` and `tests/claude.rs`; the fake is `tests/fixtures/bin/claude` with `FAKE_CODEX_*` variables. `complete_json` keeps the trait default.

### T4: `codex.bin` setting  (#205)
- Priority: 1
- Depends on: none
- Owns: `src/config.rs`, `src/config_init.rs`, `tests/config.rs`, `tests/config_init.rs`
- Touches (read-only or additive): none
- Acceptance:
  - [ ] `[codex] bin` in the file, `MINUTA_CODEX_BIN` in the environment (env wins), default `codex`
  - [ ] an unknown key in `[codex]` is rejected like `[claude]`
  - [ ] `config show` lists `codex.bin` with its source; `config init` writes a commented `# [codex]` / `# bin = "codex"` block
- Tests first: `codex_bin_default`, `codex_bin_from_file`, `codex_bin_env_overrides_file`, `codex_unknown_key_rejected`, `config_show_lists_codex_bin`, `config_init_documents_codex_bin`
- Notes: copy the `claude_bin` handling (`src/config.rs:92,104,233,325,394`) and the init text at `src/config_init.rs:50`.

### T5: Wire into the runner, `doctor` and the notice  (#207)
- Priority: 0
- Depends on: T1, T3, T4
- Owns: `src/runner.rs`, `tests/cli.rs`
- Touches (read-only or additive): `tests/integration/common.rs` (fake codex path constant)
- Acceptance:
  - [ ] `--llm codex` and a template `llm: codex:<model>` build a `Codex` backend; the binary is located once for all codex models, as an absolute path
  - [ ] a missing codex binary is listed in the missing-tools error with its install hint
  - [ ] `minuta doctor` with `llm.default = "codex"` prints `llm          codex (ok)` or `not reachable: <hint>`
  - [ ] the off-machine notice reads `<name> sends its input to codex (OpenAI)` for codex and is unchanged for claude (`… to claude (Anthropic)`), once per template
  - [ ] a full `run -t summary --llm codex` with the fake binary writes the document
  - [ ] the temporary arms from T1 are gone
- Tests first: `run_with_codex_backend_writes_output`, `missing_codex_is_reported_with_hint`, `doctor_reports_codex_ok`, `doctor_reports_codex_not_reachable`, `notice_names_codex_openai`, `notice_for_claude_unchanged`, `codex_binary_located_once`
- Notes: follow `locate_claude` (`src/runner.rs:382`); generalise it rather than copying if it stays readable.

### T6: Docs  (#208)
- Priority: 2
- Depends on: T5
- Owns: `README.md`, `docs/install.md`, `docs/specs/2026-11-01-codex-backend.md` (status section)
- Touches (read-only or additive): none
- Acceptance:
  - [ ] README backend table, `--llm` flag row, privacy paragraph and config table include `codex` and `codex.bin` / `MINUTA_CODEX_BIN`
  - [ ] `docs/install.md` has a Codex section (install, `codex login`, `--llm codex`) and the tools table row
  - [ ] the spec's implementation status is ticked with PR links
- Tests first: none (docs); `make ci` stays green.
- Notes: keep the style of the Claude Code section.

## Coverage check
| Spec requirement | Task(s) |
|---|---|
| ids parse, display, round-trip, reject unsafe names | T1 |
| `--llm`, template `llm:`, `llm.default` / `MINUTA_LLM` select it | T1, T5 |
| system prompt as developer instructions, user on stdin | T3 |
| isolation flags, empty temp dir | T2, T3 |
| off-machine notice names OpenAI | T1, T5 |
| `doctor` line and missing-tool hint | T3, T5 |
| error mapping (exit, empty, interrupt) | T3 |
| `[codex] bin` / `MINUTA_CODEX_BIN` | T4 |
| tests with a fake codex, no network | T3, T5 |
| shared helpers instead of copies | T2 |
| docs | T6 |
