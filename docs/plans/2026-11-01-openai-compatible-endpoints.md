# OpenAI-compatible endpoints — Implementation Plan

- Spec: docs/specs/2026-11-01-openai-compatible-endpoints.md
- Date: 2026-11-01
- Status: in progress

## Overview
One task and one PR, with a commit per layer so the diff reads in order: shared HTTP helpers,
config, backend id, the backend, wiring, docs. The owner merges every PR by hand, so a chain of
dependent tasks would stop at the first one. The layers also cannot ship apart: the id does
nothing without the config, and the backend nothing without the wiring. No new dependency.

## Contract
New public surface: backend id `<endpoint>:<model>`, config table `[endpoints.<name>]` with `url`,
`context_tokens`, `timeout_secs` and `api_key_env`. No file formats change.

## Impact
- Affected call paths: `BackendId` is matched in `src/llm/mod.rs` and `src/runner.rs` (`resolve`,
  `off_machine`, `preflight`, `llm_status`, `concurrency`).
- Moved code: the HTTP helpers leave `src/llm/ollama.rs`, so `tests/ollama.rs` must pass unchanged.
- Changed behavior: `gpt:x` parses, and fails later as an unknown endpoint. The expected text in
  `backend_id_rejects_invalid` and `backend_id_rejects_invalid_codex` changes.
- Data and migrations: none. Call-cache keys already include the backend id.
- Must stay green: `tests/ollama.rs`, `tests/claude.rs`, `tests/codex.rs`, `tests/config.rs`,
  `tests/config_init.rs`, `tests/cli.rs`, `make ci`.
- Rollout and rollback: additive. Revert the PR to remove it.

## Dependency graph
```
T1
```

## Tasks

### T1: OpenAI-compatible endpoints  (#218)
- Priority: 1
- Depends on: none
- Owns: `src/llm/endpoint.rs` (new), `src/llm/http.rs` (new), `src/llm/ollama.rs`, `src/llm/mod.rs`, `src/config.rs`, `src/config_init.rs`, `src/runner.rs`, `src/cli.rs`, `tests/endpoint.rs` (new), `tests/support/fake_openai.rs` (new), `tests/config.rs`, `tests/config_init.rs`, `tests/cli.rs`, `README.md`, `docs/install.md`, `CONTEXT.md`, the spec's status section
- Touches (read-only or additive): none
- Acceptance:
  - [ ] `lmstudio:a/b:c` parses to `BackendId::Endpoint { name: "lmstudio", model: "a/b:c" }` and displays unchanged; `Bad:x`, `1a:x`, `a b:x` and `x:` are rejected; the "valid forms" text lists `<endpoint>:<model>`
  - [ ] each spec invariant is a `Error::Config` naming the key: reserved name, bad name, missing `url`, bad scheme, `@` in the URL, `context_tokens` or `timeout_secs` of 0; an unknown key is rejected
  - [ ] an unknown endpoint in a run is an `Error::Usage` before any media step, and shows the `[endpoints.<name>]` lines to add
  - [ ] the request path and whole body match the spec for `complete` and `complete_json`
  - [ ] HTTP 400 or 422 to `complete_json` resends once without `response_format`; `complete` does not resend
  - [ ] `finish_reason: "length"`, empty content, 429, 5xx, 401, 403, 404, 3xx, both error-body shapes and a timeout map as the spec says
  - [ ] the bearer header is sent exactly when `api_key_env` is set; an unset variable is a missing-tool entry naming the variable; the key appears in no output
  - [ ] an unlisted model fails preflight and `doctor` says `model missing`, listing offered ids
  - [ ] the notice appears for `https://openrouter.ai/api/v1` (`<template> sends its input to openrouter (openrouter.ai)`) and a LAN address, and not for `localhost`, `127.0.0.1` or `[::1]`
  - [ ] `config show` lists each endpoint key with its source; `config init` writes a commented example; `--llm` help names the form
  - [ ] the README and `docs/install.md` give step-by-step setup for LM Studio and Colibri and a config key table
  - [ ] `tests/ollama.rs`, `tests/claude.rs` and `tests/codex.rs` pass unchanged
- Tests first: `backend_id_parses_endpoint_*`, `backend_id_rejects_endpoint_*`, `endpoint_config_*`, `endpoint_host_is_local_*`, `endpoint_sends_exact_body`, `endpoint_structured_*`, `endpoint_status_*`, `endpoint_error_body_*`, `endpoint_finish_length_*`, `endpoint_empty_content_is_transient`, `endpoint_bearer_*`, `endpoint_redirect_not_followed`, `endpoint_interrupt_returns_interrupted`, `endpoint_check_*`, `run_with_endpoint_writes_output`, `unknown_endpoint_is_reported_with_toml`, `doctor_reports_endpoint_*`, `notice_for_endpoint_*`, `config_show_lists_endpoints`, `config_init_documents_endpoints`
- Notes: follow `src/llm/ollama.rs` and `tests/ollama.rs`. Never bind-then-drop for a closed port: use `http://127.0.0.1:9`. Read the key from the process environment in the runner, not through `ENV_NAMES`. Use `.redirects(0)` on the agent. Manual run against real LM Studio before the PR.

## Coverage check
| Spec requirement | Task(s) |
|---|---|
| ids in every place a backend id goes | T1 |
| named endpoints, validation | T1 |
| request, reply, structured fallback | T1 |
| model check, `doctor` | T1 |
| notice, local detection | T1 |
| API key handling, redirects | T1 |
| docs and help | T1 |
