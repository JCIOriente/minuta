# Codex backend — Spec

- Status: approved
- Date: 2026-11-01
- Owner: Arnold Gandarillas
- Related: [templates and backends](2026-10-24-templates-and-backends.md)

## TL;DR
- A new LLM backend runs the Codex CLI (`codex exec`) the way the `claude` backend runs `claude -p`.
- Backend ids: `codex` (codex's built-in default model) and `codex:<model>`.
- It is isolated: empty temporary directory, read-only sandbox, no session files, and no
  user config, so `~/.codex/config.toml` cannot change a run.
- Codex sends the transcript to OpenAI, so minuta prints the off-machine notice for it.
- Config: `[codex] bin` / `MINUTA_CODEX_BIN`, default `codex`.

## Change in a nutshell
```
minuta run meeting.mp4 -t summary --llm codex            # codex's default model
minuta run meeting.mp4 -t summary --llm codex:gpt-5      # a given model
```
```toml
[llm]
default = "codex"

[codex]
bin = "codex"
```
For each LLM call minuta runs, from an empty temporary directory, with the user message
on stdin:
```
codex exec --ephemeral --skip-git-repo-check --ignore-user-config --ignore-rules \
  -s read-only [-m <model>] -c developer_instructions=<system prompt> -
```
and takes stdout as the reply (codex writes its banner and progress to stderr, and
only the final message to stdout; checked with codex-cli 0.166.0).

## Problem
minuta can generate documents with a local Ollama model or Claude Code. Users with a
Codex login and no Claude subscription have no way to use it.

## Goals / Success criteria
- `--llm codex`, `--llm codex:<model>`, a template's `llm: codex...` key and `llm.default`
  / `MINUTA_LLM` all select the backend.
- `codex` and `codex:<model>` parse, display and round-trip like the other ids; unsafe model
  names are rejected by the existing character rules.
- The system prompt reaches codex as developer instructions (in argv, as for `claude`) and
  the user message on stdin, so the transcript never appears in argv.
- A run with `codex` prints one notice that the template's input is sent to codex (OpenAI).
- `minuta doctor` reports `llm  codex (ok)` or `not reachable: <install hint>`; a missing binary
  is listed with an install hint in the missing-tools error.
- Failures map as for `claude`: non-zero exit → transient `Error::Llm`; empty reply →
  transient `Error::Llm`; Ctrl-C → `Error::Interrupted`.
- Tests run against a fake `codex` script (like `tests/fixtures/bin/claude`); none need the
  real CLI or network.

## Non-goals
- Structured output (`--output-schema`): `complete_json` keeps the trait default and ignores
  the schema, as `claude` does.
- Choosing reasoning effort, profiles, or other codex options.
- Codex's `--oss` / Ollama provider (use `ollama:<model>` instead).
- Auth handling: the user logs in with `codex login` themselves.

## Users & permissions
| Role | Can | Cannot |
|---|---|---|
| minuta user | choose `codex` per run, per template or by default | have codex read files or run commands: sandbox is read-only, in an empty directory |

## Tenancy & localization
n/a. Single-user CLI. Output language is already handled by the prompt (`llm.output_lang`).
The transcript leaves the machine for OpenAI; the notice says so.

## Domain model
`BackendId` gains `Codex { model: Option<String> }`. `Display` gives `codex` or
`codex:<model>`. `sends_off_machine()` is true. A new `Codex` struct implements `Backend`
with `id()`, `context_tokens()` and `complete()`. The notice names the provider per
backend (`claude (Anthropic)`, `codex (OpenAI)`), replacing the hard-coded text at
`src/runner.rs:115`.

## Interfaces
- Backend id grammar (`src/llm/mod.rs`): `ollama:<model>`, `claude`, `claude:<model>`,
  `codex`, `codex:<model>`. The "valid forms" error text lists all.
- CLI help for `--llm` (`src/cli.rs:150`) lists `codex`.
- Config: `codex.bin` (env `MINUTA_CODEX_BIN`, default `codex`) in the file, `config show`
  and `config init` output.
- argv, as above. Option values use `--flag=value`; the model goes through `-m` as
  `--model=<model>`. The system prompt goes through `-c` as a TOML string so quotes and
  newlines survive (`developer_instructions=` plus the prompt as a TOML basic string).

## Behavior & edge cases
- Context budget: `context_tokens()` is 128 000 (assumed; codex models differ and codex
  does not report the window, so this is a conservative fixed value, like `claude`'s
  150 000). Long transcripts are chunked by the existing code.
- The binary is located once for all codex models, as an absolute path (the backend runs
  from a temporary directory), like `locate_claude` (`src/runner.rs:382`).
- The scratch directory is created and removed exactly as for `claude`
  (`src/llm/claude.rs`, `ScratchDir`); the shared pieces (scratch dir, interruptible
  run) move to a common module rather than being copied.
- Not logged in: codex exits non-zero with a message on stderr; it surfaces as a transient
  `Error::Llm` with that stderr, so retries apply as for `claude`.
- Empty stdout: transient error "codex returned no output".
- `--ignore-user-config` is required for isolation; auth still works with it (verified
  with codex-cli 0.166.0: a call ran and authenticated).

## Data & migrations
None. Existing config files stay valid; `[codex]` is optional. Cached call results key on
the backend id, so `codex` results never mix with other backends.

## Testing strategy
- Unit: `BackendId` parse/display/round-trip, rejects (`codex:`, `codex:-x`, unsafe
  characters), `sends_off_machine`, argv construction (with and without a model, system
  prompt with quotes and newlines), reply mapping (signal death with and without the
  interrupt flag, empty output, non-zero exit).
- Integration (`tests/codex.rs`, modeled on `tests/claude.rs`): fake `codex` script
  checks argv flags, stdin and cwd, returns a reply; failure, empty-output and
  interrupt cases.
- CLI: `doctor` line for codex ok / missing; notice names the provider (OpenAI); config show/init include `codex.bin`; `--llm codex` end to end with the
  fake binary.
- `make ci` passes.

## Decisions
- Isolation with `--ignore-user-config` (chosen by the owner over honoring the user's
  codex config): runs do not depend on codex tools, MCP servers or profiles. A plain
  `codex` therefore uses codex's built-in default model; pick another with `codex:<model>`.
- No ADR: the backend is easy to remove or change. New term for `CONTEXT.md`: none (the
  Backend entry gains `codex`, `codex:<model>`).

## FAQ
### Why not use `--output-last-message`?
Stdout already holds only the final message, which avoids a second temp file. If a future
codex version prints more to stdout, switch to `-o <file>` in the scratch directory.

## Open questions
None.

## Implementation status
Plan: [2026-11-01-codex-backend.md](../plans/2026-11-01-codex-backend.md)
- [x] T1: backend id and provider labels (#203, PR #209)
- [x] T2: shared subprocess helpers (#204, PR #210)
- [x] T4: `codex.bin` setting (#205, PR #211)
- [x] T3: the Codex backend (#206, PR #212)
- [x] T5: runner, `doctor` and notice (#207, PR #213)
- [x] T6: docs (#208, PR #214)
