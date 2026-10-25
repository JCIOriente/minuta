# Templates and LLM backends (sub-project 2 of 6) — Spec

- Status: approved
- Date: 2026-10-24
- Owner: Arnold Gandarillas Castillo
- Related: docs/brief.md, docs/specs/2026-10-23-core-pipeline.md, ADR-0001, ADR-0003,
  ADR-0004, ADR-0005, issue #22

## TL;DR
- `minuta run <files…> -t <template>…` turns the transcript into Markdown documents described
  by prompt templates: one `<template>.md` per template in the job folder.
- A template is a Markdown file with YAML frontmatter (ADR-0004). Its `input` is the
  transcript or another template's output, so templates chain. Dependencies run first and
  cycles are an error.
- Two backends: `ollama:<model>` (local HTTP API, the default is `ollama:gemma4:26b`) and
  `claude` (`claude -p` subprocess). Long inputs are split into chunks that fit the backend's
  context, then the partial notes are consolidated.
- Template results are cached like pipeline steps (ADR-0003). Each LLM call is cached too, so
  a failed or interrupted run resumes without repeating finished calls (ADR-0005).
- Output language defaults to the detected speech language. `--output-lang` overrides it,
  and every prompt ends with an explicit "write in X" instruction. `--lang` keeps meaning
  speech language.

## Change in a nutshell
```
$ minuta run reunion.mp4 -t summary
[1/6] probe       cached
[2/6] join        cached
[3/6] audio       cached
[4/6] transcribe  cached
[5/6] outputs     cached
[6/6] summary     ollama:gemma4:26b  1 call  48.2s
minuta-out/reunion
$ ls minuta-out/reunion
audio.wav  captions.srt  job.json  joined.mp4  llm-cache/  summary.md  transcript.json  transcript.txt
```
`summary.md` is written in Spanish because whisper detected Spanish; `--output-lang en`
writes it in English. A user template `~/.config/minuta/templates/acta.md`:
```markdown
---
description: Formal minutes for the board
input: transcript
llm: claude
---
Write formal minutes for {{job_name}} ({{duration}}): attendees mentioned, decisions,
action items with owners, open questions.
```
is run with `-t acta`. A template with `input: acta` would get `acta.md` as its input and run
after it. Running the same command again reports every step and template as `cached` and
makes no LLM call. Today minuta stops at the transcript. After this change any number of
documents can be generated from it with prompts the user writes, without changing code.

## Problem
The bash pipeline hard-codes one prompt and one backend (`claude -p`). Each new document type
needs code, and one failed LLM call loses all chunk results. Sub-project 1 produced a cached,
timestamped transcript. Users now need a way to turn it into documents they define, with a
free local model by default and Claude when quality matters.

## Goals / Success criteria
1. `minuta run a.mp4 -t summary` writes `summary.md` to the job folder using the built-in
   `summary` template and the default backend, and records `templates.summary` in `job.json`.
2. A template in `./templates/` overrides one with the same name in
   `~/.config/minuta/templates/`, which overrides the built-in one (a test per layer).
3. Template B with `input: A` runs after A and receives `A.md` as input. If A is regenerated
   with different text, B reruns. If A's text is identical, B stays cached. A cycle
   (`a -> b -> a`) is a usage error (exit 2) that names the cycle, reported before any step
   runs.
4. A second identical run makes zero LLM requests (the fake server counts them) and reports
   each template as `cached`.
5. Changing a template file, its backend, the output language or the transcript reruns that
   template. A template that depends on it reruns only if the regenerated text differs
   (goal 3). Nothing else reruns.
6. An input larger than the context budget is split into chunks. No request exceeds the
   budget, by minuta's own token estimate. The chunk notes are consolidated into one
   document.
7. If a run fails or is interrupted after some chunk calls finished, the next run repeats
   only the unfinished calls (request count checked).
8. Calls that fail transiently (connection error, HTTP 429/5xx, `claude` non-zero exit,
   empty output) are retried with backoff up to `llm.retries` attempts.
9. Without `--output-lang`, every prompt asks for the detected speech language by name
   ("Write the entire response in Spanish…"). With `--output-lang en` it asks for English.
10. A missing Ollama server, a model that is not pulled, a missing `claude` binary and missing
    media tools are all reported in one message before any work starts, each with a hint.
11. `minuta templates list` shows every resolvable template with its layer, input and backend.
    `minuta templates show <name>` prints the winning file.
12. No test contacts a real LLM or the network.

## Non-goals
- The `fields` output kind, limit validation and social templates: YouTube, LinkedIn and
  Instagram (sub-project 3). `output: fields` is rejected in #2.
- `input: chapters` and any chapter logic (sub-project 4).
- The Claude HTTP API backend. Only `claude -p`.
- Streaming tokens to the terminal, and running independent templates concurrently. Templates
  run one at a time, in dependency order. Calls within a template run concurrently.
- Template variables other than the five listed below, and an escape syntax for a literal
  `{{`.
- The brief's `name` and `flag` frontmatter fields. The name is the file stem (ADR-0004) and
  templates declare no short flags (ADR-0001).
- Pruning old call-cache entries. They are deleted with the job folder.
- Translating the transcript, or detecting the language a template prompt is written in.

## Users & permissions
| Role | Can | Cannot |
|---|---|---|
| Local user | Write templates in `~/.config/minuta/templates/` and `./templates/`; pick any backend; run any template on jobs they can write | n/a: single-user tool with no accounts |

Security notes:
- **Data leaving the machine.** The default backend is local Ollama. The transcript goes to
  Anthropic only when `claude` is selected by `--llm`, by a template's `llm:` or by
  `llm.default`. A template in `./templates/` can select `claude` too, so minuta prints a
  note once per run for each template that sends data off the machine:
  `note: acta sends its input to claude (Anthropic)`.
- `claude` runs through `tools::run` with an argv array, never a shell. The system prompt is
  passed as an argument and the input on stdin. It runs in an empty temporary working
  directory, with all tools disabled, no session persistence and no project or user settings,
  so the prompt cannot make it read or change files. The implementing task pins the exact
  flags against `claude --help` and a test asserts the argv.
- Ollama is reached at `ollama.url` (default `http://127.0.0.1:11434`). minuta sends no
  credentials.
- `llm-cache/` holds model output derived from the recording. It has the same sensitivity as
  the transcript and lives in the job folder.
- Templates are text. They cannot run commands. A malformed template fails only when it is
  requested. `templates list` shows it as invalid.

## Tenancy & localization
- Tenancy: n/a (single local user).
- Three independent languages (CONTEXT.md):
  - **Speech language**: what whisper transcribes (`--lang`, `whisper.lang`). Unchanged.
  - **Prompt language**: whatever the template author wrote. The built-in template is in
    English.
  - **Output language**: what documents are written in. Precedence: `--output-lang` >
    template `lang:` > `llm.output_lang` (env `MINUTA_OUTPUT_LANG` > config file) > the
    speech language detected in `transcript.json`.
- Language codes are whisper's codes (`en`, `es`, `pt`, …). minuta embeds whisper's
  code-to-English-name table, so code `es` reaches the prompt as "Spanish". An unknown code is
  a usage error that suggests `minuta run --help`.
- Every prompt ends with: "Write the entire response in {output_lang}, including headings and
  any placeholder text, regardless of the language of the input or of these instructions."
- Timestamps in the transcript input are `[hh:mm:ss]` from the start of the joined media.
- minuta's own messages stay English only.

## Domain model
See `CONTEXT.md` for Template, Template step, Backend, Output language, Call cache, Chunk and
Consolidation.

### Template file
`<name>.md`. The name is the file stem and must match `^[a-z0-9][a-z0-9_-]*$`. Files whose
stem does not match are ignored, and `templates list` reports them. Reserved names, which are
rejected: `transcript`, `chapters`, `probe`, `join`, `audio`, `transcribe`, `outputs`, `job`.
```markdown
---
description: Short summary of the recording   # optional; shown by `templates list`
input: transcript      # transcript | <template name>; default transcript
output: markdown       # only markdown in #2; default markdown
llm: claude            # optional backend id
lang: en               # optional output language code
---
Prompt body with {{variables}}.
```
- The frontmatter is optional. A file that does not start with a `---` line is all body and
  uses the defaults.
- Unknown frontmatter keys are an error, so a sub-project 3 template fails clearly on this
  version.
- Variables (`{{name}}`, spaces inside the braces allowed). An unknown variable is an error
  at load time that lists the known ones:

  | Variable | Example |
  |---|---|
  | `output_lang` | `Spanish` |
  | `output_lang_code` | `es` |
  | `speech_lang` | `Spanish` (detected) |
  | `job_name` | `reunion` |
  | `duration` | `1:02:03` (`h:mm:ss`, end of the last segment) |

### Discovery
Layers, later wins by name: built-in (compiled in with `include_str!`) ->
`<home>/.config/minuta/templates/*.md` -> `./templates/*.md` (relative to the current
directory). A missing directory is an empty layer. Any other I/O error is an error that
names the path (conventions: never treat an I/O error as absent). Built-in templates in #2:
`summary` only.

### Template graph
The requested templates plus everything their `input` reaches, in topological order. When
order is not forced by dependencies, templates run in the order of the `-t` flags. Unknown
input -> usage error naming the template and the input. A cycle -> usage error
`template cycle: a -> b -> a`.

### Backend
```rust
pub trait Backend: Send + Sync {
    /// "ollama:gemma4:26b", "claude", "claude:sonnet": part of every cache key and fingerprint.
    fn id(&self) -> &str;
    /// Tokens the backend accepts per call (prompt + output).
    fn context_tokens(&self) -> usize;
    /// One completion. `interrupt` aborts the wait and yields Error::Interrupted.
    fn complete(&self, system: &str, user: &str, interrupt: &AtomicBool) -> Result<String>;
}
```
- Backend ids: `ollama:<model>` (split on the first `:`, so `ollama:gemma4:26b` is model
  `gemma4:26b`), `claude`, `claude:<model>`. Anything else is a usage error. Model names may contain only
  letters, digits and `._:/-[]@`, and may not start with `-`.
- **Ollama**: `POST {url}/api/chat` with `stream: false`, `options.num_ctx`, and
  `think: false` when `/api/show` lists the `thinking` capability. Context budget =
  min(`ollama.num_ctx`, the model's `context_length` from `/api/show`). minuta must enforce
  the budget itself because Ollama silently truncates prompts over `num_ctx`. A response with
  `done_reason: "length"` is an error with the hint "raise ollama.num_ctx".
- **claude**: `claude -p` with `--system-prompt`, `--output-format text`, tools disabled, no
  session persistence, no setting sources, `--model <m>` when given. The input goes on stdin.
  Context budget: 150,000 tokens.

### Configuration (new keys)
| Key | Env | Default |
|---|---|---|
| `llm.default` | `MINUTA_LLM` | `ollama:gemma4:26b` |
| `llm.output_lang` | `MINUTA_OUTPUT_LANG` | unset (use detected speech language) |
| `llm.concurrency` | `MINUTA_LLM_CONCURRENCY` | unset (ollama 1, claude 3) |
| `llm.retries` | `MINUTA_LLM_RETRIES` | `3` attempts in total |
| `llm.backoff_ms` | `MINUTA_LLM_BACKOFF_MS` | `2000` (doubles after each failed attempt) |
| `ollama.url` | `MINUTA_OLLAMA_URL` | `http://127.0.0.1:11434` |
| `ollama.num_ctx` | `MINUTA_OLLAMA_NUM_CTX` | `16384` |
| `claude.bin` | `MINUTA_CLAUDE_BIN` | `claude` |

The backend for a template is chosen by `--llm` > template `llm:` > `llm.default`.
`config show` lists every new key.

### Template step and job state
`job.json` gains an additive map. The version stays 1 (ADR-0005):
```json
"templates": {
  "summary": { "status": "done", "fingerprint": "…", "seconds": 48.2, "reason": null,
               "artifacts": ["summary.md"] }
}
```
- Fingerprint = SHA-256 of: `"template"`, the name, the raw file bytes, `PROMPT_VERSION` (a
  constant bumped when minuta's prompt wrappers change), the backend id, the context budget,
  the output language code, the content hash of the input artifact (`transcript.json` or
  `<input>.md`), and the rendered variable values (`output_lang`, `output_lang_code`,
  `speech_lang`, `job_name`, `duration`). The variables come from `transcript.json` even
  when the input is another template, so a chained template reruns when they change.
- The fingerprint is computed just before the template would run, after its input exists.
  Upstream changes therefore propagate through content, not through a precomputed chain.
- Stale reasons reuse `Reason`: not run, unfinished, forced, fingerprint changed, missing
  artifact.
- Invariant: `status: done` if and only if `<name>.md` exists. It is written with
  `write_atomic`.
- Templates recorded in `job.json` that are not requested in a run are left untouched.

### Prompt assembly, chunking and consolidation
- **Input text.** For `transcript`, segments are merged into blocks of at least 30 s, one line
  per block: `[00:01:30] text…`. For a template input it is that template's Markdown.
- **System prompt** = rendered template body + the language instruction (always last).
  **User message** = the input text.
- **Token estimate** = characters / 3, rounded up. This deliberately over-counts for
  English and Spanish.
- **Budget.** Output reserve = context / 4. If system + input + reserve fits the context,
  one call is made.
- **Map.** Otherwise the input is split at line boundaries, filling each chunk greedily. A
  single line over the budget is split at whitespace. Order is kept. Each chunk gets a map
  call: "You are preparing notes for this task: <rendered body>. This is part i of n. Extract
  everything the task needs from this part, keeping timestamps. Do not write the final
  document." Notes are written in the output language. Map calls run concurrently up to the
  concurrency limit.
- **Reduce.** The notes are joined in order under `## Part i` headings and passed as the
  input of one final call. Its system prompt is the rendered body + "The input is notes
  extracted from consecutive parts of a longer input." + the language instruction. If the
  joined notes do not fit, they are merged in groups with the map prompt, round after round,
  until they fit. If a round does not shrink the total, the template fails:
  `template summary: notes do not shrink below the context budget`.
- **Cleanup.** The output is trimmed. A leading `<think>…</think>` block is removed. If the
  whole output is one fenced code block, the fence is removed. Empty output counts as a
  transient failure.

### Call cache
`<job>/llm-cache/<key>.txt` holds one call's cleaned output. It is written atomically: to a unique temp file in
`llm-cache/` (so concurrent calls never share one), synced, then renamed.
Key = SHA-256 of (backend id, context budget, system, user). Every call checks the cache
first, except calls of a template whose stale reason is `forced`. Those calls skip the read
and overwrite the entry.

## Interfaces
```
minuta run <files…> [-t TEMPLATE]… [--llm ID] [--output-lang CODE]
                    [--name N] [--out DIR] [--lang auto|xx] [--model PATH|NAME]
                    [--force [STEP|TEMPLATE]] [--quiet]
minuta transcribe <files…>        # unchanged: no -t, --llm or --output-lang
minuta templates list
minuta templates show <name>
minuta doctor                     # gains one line: llm <default id> (status)
```
- `-t` is repeatable. Without `-t`, `run` behaves exactly as in sub-project 1 and never
  contacts a backend.
- "Templates in the run" means the requested templates plus the ones they need as input.
  `--force` with no value forces every step and every template in the run. `--force <step>`
  forces that step, every later step and every template in the run. `--force <template>`
  forces that template and every template in the run that depends on it, but no pipeline
  step. A value that is neither a step nor a template in the run is a usage error.
- An unknown `-t` name is a usage error that lists the available names.
- Progress: the step counter covers pipeline steps plus templates (`[6/6] summary`). While a
  template runs, the spinner shows `chunk 2/5` or `consolidating` with elapsed time. The done
  line shows backend, call count and time. stdout is still the job folder only.
- `templates list` prints one line per name: `NAME  LAYER  INPUT  LLM  DESCRIPTION`. LAYER is
  `built-in`, `user` or `project`. LLM is `(default)` when unset. Invalid templates and
  ignored file names follow, each with its path and error. The exit code is 0 either way.
- `templates show <name>` prints the winning file's path to stderr and its content to stdout.
  An unknown name exits 2.
- `doctor` reports the default backend as reachable, not reachable or model missing. That
  line does not affect the exit code, because transcription alone does not need a backend.
- Exit codes are unchanged: 0, 1 failure, 2 usage, 130 interrupted.

## Behavior & edge cases
- **Resolution happens first.** Templates are loaded, frontmatter and variables are validated,
  the graph is built and backend ids are parsed before preflight. Any error exits 2 before a
  file is touched.
- **Preflight** adds backend checks to the existing tool check, one combined message:
  - Ollama server unreachable: hint "start Ollama (`ollama serve`)".
  - Model not in `/api/tags`: hint "`ollama pull <model>`". A bare model name also matches
    `<model>:latest`.
  - `claude` not found: hint "install Claude Code: https://claude.com/claude-code".
  - The backends of every template in the run are checked even if every template turns out
    to be cached. The Ollama context budget is part of the fingerprint and comes from
    `/api/show`. This matches sub-project 1, where whisper is required even when
    `transcribe` is cached.
- **Missing speech language.** If `transcript.json` has a language not in the table and
  nothing else sets the output language, the template fails with a hint to pass
  `--output-lang`.
- **Retries.** Transient: connection or timeout errors, HTTP 429 and 5xx, `claude` non-zero
  exit, empty output. Not retried: HTTP 404 (model missing, with a pull hint), other 4xx,
  `done_reason: "length"`. Waits are `llm.backoff_ms` × 2^(attempt−1). The Ollama read
  timeout is 10 minutes per call.
- **Failure.** The first template that fails stops the run (exit 1). Templates that finished
  stay recorded. Finished calls stay in the cache.
- **Interrupt.** Ctrl-C kills a running `claude`, stops waiting on in-flight Ollama requests
  (their threads are abandoned when the process exits), removes temp files, exits 130 and
  marks no unfinished template done. Finished calls stay in the cache.
- **Empty transcript.** A transcript with zero segments fails each requested template with
  `template summary: transcript is empty`. Transcribe already warns about it.
- **Non-UTF-8 template file.** An error naming the path.
- **Off-machine note.** See Security notes.

## Data & migrations
- `job.json` stays version 1. `templates` is optional on read, so sub-project 1 job folders
  load unchanged. An older binary ignores the field and drops it on save. The next new run
  then reruns templates, which is safe (ADR-0005).
- New files in the job folder: `<template>.md` and `llm-cache/`.
- New crate: `serde_yaml_ng` for frontmatter (ADR-0004). HTTP uses the existing `ureq`.
- `tools::run` gains optional stdin input and separate stdout capture, which the `claude`
  backend needs. Existing callers are unchanged.

## Testing strategy
- **Unit.** One assertion per case in table tests (conventions):
  - Frontmatter: each key, the defaults, unknown key, `output: fields` rejected, no
    frontmatter, YAML error with its line number.
  - Name rule and reserved names: one case each.
  - Discovery precedence: each layer overriding the one below. A missing directory is empty;
    an unreadable one is an error.
  - Variables: each variable rendered; unknown variable.
  - Graph: chain, diamond, `-t` order kept, unknown input, self-reference, cycle message.
  - Backend id parsing, including `ollama:gemma4:26b` and invalid ids.
  - Output language and backend precedence: one case per layer.
  - Language table: code to name, unknown code.
  - Block formatting at 30 s.
  - Chunking: no chunk over budget, an overlong line is split, order is kept, the
    single-call path.
  - Map/reduce with a fake `Backend`: single call, chunked, multi-round reduce, error when
    notes do not shrink.
  - Retry classification and attempt count with zero backoff.
  - Cache key sensitivity to each part.
  - Output cleanup: think block, fence, empty.
  - Template fingerprint sensitivity to each part.
- **Integration.** Fake Ollama on a local `TcpListener` that serves `/api/tags`, `/api/show`
  and `/api/chat` from a script and records requests. The fake `whisper-cli` fixture from
  sub-project 1 is reused. Cases:
  - Full `run -t summary`; then a second run with zero chat requests.
  - Editing a template reruns only it and its dependents.
  - A chain where A is regenerated with different text versus identical text.
  - `--force summary` bypasses the cache.
  - Chunk 3 fails, then a rerun requests only chunks 3..n and the reduce.
  - Ctrl-C during a chat request exits 130 with no `summary.md`.
  - 500 then 200 is retried; 404 fails with a pull hint.
  - `num_ctx` and `think: false` are sent.
  - The prompt names Spanish for an `es` transcript and English with `--output-lang en`.
  - Preflight reports an unreachable Ollama and a missing `whisper-cli` together.
  - `run` without `-t` makes no request.
  - `templates list` and `show` across three layers, using a temp `HOME` and current
    directory.
- **Fake claude.** A committed executable fixture `tests/fixtures/bin/claude` (ETXTBSY-safe,
  conventions). It records its argv and stdin to files named by env vars and prints a
  scripted reply or exits non-zero. Tests assert the argv flags, that the input arrived on
  stdin, that a non-zero exit is retried, and that a file name with `$(…)` runs nothing.
- **Opt-in e2e.** `MINUTA_E2E_LLM=1` runs `-t summary` against the real local Ollama.
- CI runs `make ci` on macOS and Linux with no network access to LLMs.

## Decisions
- ADR-0001 (relied on): templates are selected with `-t` and declare no short flags.
- ADR-0003 (extended by ADR-0005): fingerprints in `job.json`.
- ADR-0004: Template files are Markdown with YAML frontmatter, named by file stem, found in
  three layers.
- ADR-0005: Template steps in an additive `templates` map with content-addressed
  fingerprints, plus a per-job LLM call cache.
- `CONTEXT.md` gains: Template, Template step, Backend, Output language, Prompt language,
  Call cache, Chunk, Consolidation. Step and Speech language are updated.

## FAQ
### Why is `--lang` still the speech language?
It shipped in sub-project 1 and matches `whisper.lang`. Renaming it would break scripts for a
flag that already works. Output language gets its own `--output-lang`.

### Why not put template steps into the pipeline's `StepName` enum?
`StepName` is a closed, ordered `Copy` enum used by stale planning, `--force` and progress.
Opening it up would rewrite shipped code that templates do not need (ADR-0005).

### Why hash the input's content instead of chaining upstream fingerprints?
LLM output is not deterministic. If an upstream template is regenerated (forced, or its
artifact was deleted) with the same fingerprint but different text, a precomputed chain would
leave its dependents cached on stale input. Hashing content reruns them only when the text
actually changed.

### Why cache individual calls when the template step is already cached?
The step cache records only finished templates. A one-hour recording on a local model can
need ten chunk calls. The call cache keeps the nine that finished when the tenth fails, which
was the main failure of the bash pipeline.

### Why characters / 3 instead of a tokenizer?
Each backend uses a different tokenizer, and none is available offline as a Rust crate for
gemma4 or Claude. Over-counting costs an extra chunk. Under-counting makes Ollama silently
drop input, so the estimate errs high.

### Why is the file stem the template name instead of a `name` field?
Two sources of truth can disagree. With the stem, overriding a template in a higher layer is
"create a file with the same name", and `templates show` can always find the file.

## Open questions
None.

## Implementation status
Updated by the PRs that implement this spec.
- [x] Plan: docs/plans/2026-10-24-templates-and-backends.md (#26)
- [x] T1 foundations: module skeleton, error variants, Backend trait, language table (#39)
- [x] T2 config keys for LLMs and output language (#40)
- [x] T3 template files, variables and layered discovery (#46)
- [x] T4 template dependency order and dependents (#41)
- [x] T5 Ollama backend and fake Ollama server (#45)
- [x] T6 `claude -p` backend, `tools::run_capture` and fake claude (#43)
- [x] T7 call cache, retries and output cleanup (#44)
- [x] T8 prompt assembly, chunking and consolidation (#49)
- [x] T9 template step state in `job.json` (#42)
- [x] T10 template step with content-hash fingerprint (#50)
- [x] T11 runner, CLI, progress, preflight and doctor (this PR, issue #36)
- [x] Fix: flaky backoff interrupt bound in CI (#48)
- [ ] T12 integration suite and opt-in LLM e2e (#37)
