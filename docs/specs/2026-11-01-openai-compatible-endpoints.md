# OpenAI-compatible endpoints — Spec

- Status: approved
- Date: 2026-11-01
- Owner: Arnold Gandarillas
- Related: [templates and backends](2026-10-24-templates-and-backends.md), [Codex backend](2026-11-01-codex-backend.md)

## TL;DR
- A new backend talks to any server with an OpenAI-compatible API: LM Studio, Colibri,
  llama.cpp, vLLM, or a cloud service.
- The user names a server once in the config (`[endpoints.lmstudio]`) and then uses
  `<endpoint>:<model>` wherever a backend id goes, such as `--llm lmstudio:qwen3`.
- The native Ollama backend stays: only its API can set `num_ctx` per request.
- A local URL keeps the transcript on the machine. Any other URL prints the off-machine notice.
- API keys are never in the config file: `api_key_env` names an environment variable.

## Change in a nutshell
```toml
[endpoints.lmstudio]
url = "http://127.0.0.1:1234/v1"

[endpoints.colibri]
url = "http://127.0.0.1:8000/v1"
context_tokens = 32768
timeout_secs = 3600

[endpoints.openrouter]
url = "https://openrouter.ai/api/v1"
api_key_env = "OPENROUTER_API_KEY"
```
```
minuta run meeting.mp4 -t summary --llm lmstudio:google/gemma-4-26b-a4b
minuta run meeting.mp4 -t summary --llm colibri:glm-5.3
```
`--llm`, a template's `llm:` key, `llm.default` and `MINUTA_LLM` all accept these ids. For each
call minuta sends `POST <url>/chat/completions`, after checking `GET <url>/models` once.

## Problem
minuta generates documents through Ollama, Claude Code or Codex. Someone who runs their models
in LM Studio, Colibri or another server has no way to use them, although these servers share one
API. A backend per server would repeat the same code.

## Goals / Success criteria
- `--llm <endpoint>:<model>`, a template's `llm:` key, `llm.default` and `MINUTA_LLM` select an
  endpoint named in the config.
- A run against a fake server writes the document. Tests pin the exact request body and path.
- `minuta doctor` reports `ok`, `model missing: ...` (listing the models the server offers) or
  `not reachable: ...` for an endpoint.
- An id naming an endpoint that is not configured fails before any media work, with a message
  showing the exact TOML to add.
- A model the server does not list fails preflight. LM Studio otherwise answers with whichever
  model is loaded (seen on LM Studio with `model: "nope-model"`).
- Structured output is sent as `response_format`, and a server that refuses it still works.
- Requests to a non-loopback URL print the notice with the endpoint and its host, and requests to
  a loopback URL do not.
- An API key is sent only as `Authorization: Bearer`, and never appears in `config show`, the
  config file, errors or the call cache.
- `config init`, `config show`, `--help`, the README and `docs/install.md` document the feature,
  with step-by-step setup for LM Studio and Colibri.

## Non-goals
- Replacing the native Ollama backend, or moving `claude` or `codex` onto this API.
- Streaming. `timeout_secs` covers slow models.
- Built-in endpoints or presets: every endpoint is declared in the config.
- A command that edits the config, or one that lists models.
- `MINUTA_*` overrides for endpoint keys: names are user-chosen, so the file is the one source.
- Sending `max_tokens`, temperature or thinking options.
- Fixing `ollama.url` pointing to another machine without a notice. That gap already exists and
  is tracked separately.

## Users & permissions
| Role | Can | Cannot |
|---|---|---|
| minuta user | declare endpoints, and choose one per run, per template or by default | n/a |
| template author (untrusted) | name a `<endpoint>:<model>` id | add an endpoint, or set a URL or key: only the user's config can |

## Tenancy & localization
n/a. Single-user CLI. Output language is already handled by the prompt (`llm.output_lang`).

## Domain model
- `Endpoint` (config): `name`, `url`, `context_tokens`, `timeout_secs`, `api_key_env`.
- `BackendId::Endpoint { name, model }`, shown as `<name>:<model>`.
- `OpenAiCompatible` (`src/llm/endpoint.rs`) implements `Backend`.

Invariants:
- An endpoint name matches `[a-z][a-z0-9_-]*` and is not `ollama`, `claude` or `codex`.
- `url` parses, uses `http` or `https`, and holds no credentials (no `@`), query or fragment.
- `context_tokens` and `timeout_secs` are at least 1.

## Interfaces
**Backend id grammar** (`src/llm/mod.rs`): `ollama:<model>`, `claude`, `claude:<model>`, `codex`,
`codex:<model>`, `<endpoint>:<model>`. Parsing cannot read the config, because templates are
parsed without it, so any well-formed `name:model` parses. `resolve` checks the `--llm` flag,
which reaches no backend without a template, and `preflight` checks every id in the run. `gpt:x` therefore moves from a parse error to an unknown-endpoint
error.

**Config** (file only):
| Key | Required | Default | Meaning |
|---|---|---|---|
| `endpoints.<name>.url` | yes | | Base URL, as the server documents it, including `/v1` |
| `endpoints.<name>.context_tokens` | no | `16384` | Prompt and reply budget per call; must fit the server's loaded context |
| `endpoints.<name>.timeout_secs` | no | `600` | Read timeout for one request |
| `endpoints.<name>.api_key_env` | no | | Name of the environment variable holding the key |

Unknown keys are rejected, as in every other table.

**Requests:**
```json
{"model": "<model>", "stream": false,
 "messages": [{"role": "system", "content": "..."}, {"role": "user", "content": "..."}]}
```
`complete_json` adds:
```json
"response_format": {"type": "json_schema",
                    "json_schema": {"name": "minuta", "strict": true, "schema": {}}}
```
The reply is `choices[0].message.content`. `reasoning` and `reasoning_content` are ignored.

**Notice:** `<template> sends its input to <endpoint> (<host>)`, in the existing format.

**`config show`** adds a line per key, per endpoint, after the fixed keys, with source `file` or
`default`, or `(unset)`. `api_key_env` shows the variable's name, never its value.

## Behavior & edge cases
Behavior marked *probed* was seen on 2026-11-01 against Ollama (`/v1`) and LM Studio with
`gemma4` 26b. Colibri is from its `docs/api.md`, not from a run.

- **Local or remote:** the URL's host is local when it is `localhost`, in `127.0.0.0/8`, or `::1`.
  Everything else counts as off-machine, LAN addresses included.
- **Reasoning:** every server tested returns it apart from `content` (*probed*: `reasoning` on
  Ollama, `reasoning_content` on LM Studio), so no thinking option is needed.
- **Model check:** `GET <url>/models` runs once per backend id in preflight, and the id must match
  `data[].id` exactly. The result lists up to 10 offered ids, sanitized, in the hint. Control
  characters are dropped as for every server text.
- **Structured output refused:** on HTTP 400 or 422 to a `complete_json` call, minuta sends the
  same request once more without `response_format`. Templates already handle a backend without
  structured output (`claude`, `codex`). Colibri's docs do not mention `response_format`, so this
  is the path it may take. A context error also lands here and fails again with its own message.
- **Prompt beyond the context:** HTTP 400 surfaces with the server's message (*probed* on LM
  Studio: `request (8021 tokens) exceeds the available context size (4096 tokens)`), and is not
  transient. minuta does not read the server's context size, because no standard API gives it.
- **Cut off:** `finish_reason: "length"` is a non-transient error telling the user to give the
  model a larger context on the server (*probed* on both).
- **Ended early:** `finish_reason` `content_filter` is a non-transient error and `error` a
  transient one, so partial text is never saved as a document.
- **Empty reply:** missing or empty `content` with any other finish reason is a transient error.
  It carries the message of an `error` object, which some services send with HTTP 200.
- **Error bodies:** `{"error": {"message": ...}}` (Ollama, OpenAI) and `{"error": "..."}` (LM
  Studio) are both read, and anything else is shown as a sanitized excerpt.
- **Status mapping:** 429 and 5xx are transient; every other status is not. 401 and 403 hint at
  `api_key_env`.
- **API key:** with `api_key_env` set, the variable must be set, non-empty and visible ASCII, or
  the endpoint is reported as missing, with a hint naming the variable and never its value. Without
  it, no header is sent. A key echoed in server text is replaced by `[redacted]` in every message.
- **Redirects:** never followed, so a key or transcript cannot be moved to another host. A 3xx is
  a non-transient error.
- **Concurrency:** 1 unless `llm.concurrency` is set, as for Ollama. One loaded model serves one
  request at a time, and Colibri's queue times out after 300 s by default.
- **Interrupt:** as in `ollama.rs`, the request runs on a worker thread that Ctrl-C abandons.
- **Timeouts:** the request is not streamed, so `timeout_secs` must cover a whole reply. Slow
  servers need a higher value (Colibri reports 0.2 to 3.5 tok/s on some CPU setups).

## Data & migrations
None. Existing config files stay valid, and `[endpoints]` is optional. The call cache keys on the
backend id, so results never mix with another backend. As with `ollama.url`, pointing an endpoint
at another server under the same name reuses cached calls until `--force`.

## Testing strategy
- Unit: id parse, display and rejects; config validation, one assertion per key and rule; host
  classification with real URLs; error-body parsing.
- Integration (`tests/endpoint.rs`, with a fake server): exact request body and path, bearer
  header present and absent, every status mapping, `finish_reason`, empty content, refused
  `response_format` retried without it, 3xx not followed, interrupt, timeout, unlisted model.
- CLI (`tests/cli.rs`): full `run -t summary --llm <endpoint>:<model>`; unknown endpoint; missing
  key variable; `doctor` for ok, model missing and not reachable; notice for remote and none for
  local; `config show` and `config init`.
- Manual: a run against real LM Studio before the PR. Colibri stays untested on a real server and
  the docs say so.
- `make ci` passes.

## Decisions
- `<endpoint>:<model>` over `endpoint:<name>:<model>`: shorter, at the cost of later validation.
  Reserved names keep it unambiguous.
- No ADR: the backend is additive and easy to remove. `CONTEXT.md` gains the term **Endpoint**,
  and the **Backend** entry gains `<endpoint>:<model>`.

## FAQ
### Why not move Ollama onto this backend?
Ollama's docs say the OpenAI API cannot set the context size. minuta would also lose `/api/show`,
which gives the model's context length.

### Why not include `lmstudio` and `colibri` out of the box?
A preset adds a second way to define an endpoint and merge rules between the two, to save two
lines of config. The error for an unknown endpoint prints those lines.

### Why reference a key by variable name?
The config file is often copied or kept in dotfile repositories, and `config show` prints it.

## Open questions
None.

## Implementation status
Plan: [2026-11-01-openai-compatible-endpoints.md](../plans/2026-11-01-openai-compatible-endpoints.md)
- [x] T1: OpenAI-compatible endpoints (#218)
