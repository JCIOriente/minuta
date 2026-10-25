# Structured fields and social templates (sub-project 3 of 6) — Spec

- Status: approved
- Date: 2026-10-25
- Owner: Arnold Gandarillas Castillo
- Related: docs/brief.md, docs/specs/2026-10-24-templates-and-backends.md, ADR-0001, ADR-0003,
  ADR-0004, ADR-0005, ADR-0006, ADR-0007, issues #54, #22

## TL;DR
- A template can declare `output: fields`. The model then returns a JSON object with the
  fields the template lists, taken from one shared set: `title`, `body`, `tags`, `hashtags`,
  `thumbnail_text` (ADR-0006).
- `limits` per field (`max_chars`, `max` items) are checked after generation. When an answer
  breaks a limit or is not valid JSON, minuta sends the model a repair call that lists the
  problems, up to `llm.repairs` times (default 2), then fails with the remaining problems.
- A fields template writes `<name>.json` (versioned, for later consumers such as the YouTube
  publisher) and `<name>.md`, rendered as `plain` (paste-ready) or `markdown` (ADR-0007).
- New built-in templates: `youtube`, `linkedin` and `instagram`, chained on `summary`. They
  are ordinary template files, with no platform-specific code.
- Only answers that pass validation are written to the call cache. A failed run therefore
  gets a fresh answer the next time, and chunk notes are kept.

## Change in a nutshell
```
$ minuta run clase.mp4 -t youtube -t linkedin
[1/8] probe       cached
…
[6/8] summary     ollama:gemma4:26b  1 call  51.0s
[7/8] youtube     ollama:gemma4:26b  2 calls  14.3s
[8/8] linkedin    ollama:gemma4:26b  1 call  9.8s
minuta-out/clase
$ ls minuta-out/clase
… linkedin.json  linkedin.md  summary.md  youtube.json  youtube.md
```
`youtube` took 2 calls: the first description had 5,412 characters, so minuta asked the model
to shorten it below 5,000, and the second answer passed. `youtube.json`:
```json
{
  "version": 1,
  "template": "youtube",
  "output_lang": "es",
  "fields": {
    "title": "Clase 4: Ownership y borrowing en Rust",
    "body": "En esta clase vemos…\n\n[Agregar capítulos con marcas de tiempo]",
    "tags": ["rust", "ownership", "borrowing"],
    "thumbnail_text": "Ownership en Rust"
  }
}
```
`youtube.md` has the same values as paste-ready text: the title, a blank line, the description,
a blank line, `rust, ownership, borrowing`, a blank line, the thumbnail text. The built-in
`linkedin.md`:
```markdown
---
description: LinkedIn post about the recording
input: summary
output: fields
fields: [body, hashtags]
limits:
  body: { max_chars: 2800 }
  hashtags: { max: 5, max_chars: 150 }
format: plain
---
Write a LinkedIn post about "{{job_name}}" from the summary in the input. …
```
A user template can declare any subset of the shared fields with its own limits, and override
a built-in one by creating a file with the same name (ADR-0004). Today a template can only
produce free Markdown, and `output: fields` is rejected (`src/template/mod.rs:193`). After this
change minuta produces social copy with platform limits enforced. The JSON file is the input
that sub-project 6 uploads to YouTube.

## Problem
Social copy has hard platform limits: a YouTube title of 100 characters, a LinkedIn post of 3,000,
an Instagram caption of 2,200 with at most 30 hashtags. Local models often ignore length
instructions, so copy that is pasted by hand fails at upload or is cut off. The YouTube
publisher (sub-project 6) needs title, description and tags as separate values, not as
Markdown it has to parse. The bash pipeline produced YouTube copy as free text with no
checks.

## Goals / Success criteria
1. `minuta run a.mp4 -t youtube` runs `summary` and then `youtube`, writes `youtube.json` and
   `youtube.md`, and records `templates.youtube` with both artifacts in `job.json`.
2. `youtube.json` matches the format in ADR-0007. It holds exactly the declared fields, hashtags
   are stored without `#`, and every value meets its limits.
3. When the fake backend returns a body over its `max_chars`, the next request is a repair call
   whose input names the field, its length and the limit. If the repaired answer passes, the
   template is done with 2 calls.
4. When every answer breaks a limit, the template fails (exit 1) after `1 + llm.repairs`
   answers (the final call plus `llm.repairs` repair calls). The message names each remaining violation. Neither artifact is written.
5. An answer that is not a JSON object, a missing key, or a value of the wrong type is treated
   like a limit violation (one test each).
6. A rejected answer is never read back from the call cache: after a failed run, the next run
   makes a new final call (request count checked), while map calls stay cached.
7. Requests to Ollama for a final or repair call carry a JSON schema in `format`. Map calls do
   not. `claude` gets the same JSON instructions in the system prompt and no extra flags.
8. With chunked input, only the final consolidation call asks for JSON. Map calls are the same
   as in sub-project 2.
9. A second identical run makes zero requests. Changing a field, a limit or the format reruns
   that template (the template file is in the fingerprint). Fingerprints of `markdown`
   templates do not change, so sub-project 2 job folders stay cached.
10. Every frontmatter error in the list under "Behavior & edge cases" exits 2 before any step
    runs, with a message that names the template (one test per error).
11. The built-in `youtube`, `linkedin` and `instagram` templates load, and each declares the
    limits listed under "Built-in social templates" (one test per limit).
12. `templates list` shows an OUTPUT column (`markdown` or `fields`).
13. The prompt for a fields template still ends with the language instruction from
    sub-project 2. The text before it tells the model to keep the JSON keys in English.
14. No test contacts a real LLM or the network.

## Non-goals
- The `chapters` field and any chapter logic (sub-project 4). `chapters` in `fields` is
  rejected with "arrives in a later version".
- Fields beyond the shared set, or user-defined fields (ADR-0006).
- Limits other than `max_chars` and `max`: no minimums, no forbidden characters (for example
  YouTube rejects `<` and `>`; sub-project 6 handles upload rules), no per-item length.
- Cutting text to fit. minuta never cuts a value; only the model rewrites it.
- `claude` structured-output flags. `claude` is asked for JSON in the prompt only.
- Grammar-enforced lengths in the Ollama schema (`maxLength`, `maxItems`). Cutting mid-word
  is worse than a repair call.
- Publishing anything, and `fields` output for templates that run before the pipeline ends.
- Fixing issue #54 (empty sections in `summary` on gemma4), although the social templates
  read `summary.md`.

## Users & permissions
| Role | Can | Cannot |
|---|---|---|
| Local user | Write fields templates in `~/.config/minuta/templates/` and `./templates/`, override the built-in social templates, set `llm.repairs` | n/a: single-user tool with no accounts |

Security notes:
- Model answers are untrusted. They are parsed with `serde_json` into typed values. Control
  characters other than `\n` and `\t` are removed and `\r\n` becomes `\n` before validation.
  Values are only written to files, never printed. Violation messages printed in an error
  name fields and counts. The one message that quotes a value (a hashtag that contains
  whitespace) goes through `render_error`, which sanitizes it.
- Frontmatter `fields` and `limits` are validated against the shared set with typed,
  `deny_unknown_fields` structs (ADR-0004).
- The off-machine note from sub-project 2 is unchanged. Repair calls go to the same backend.

## Tenancy & localization
- Tenancy: n/a.
- The output language rules from sub-project 2 are unchanged (precedence, codes, the language
  instruction always last). JSON keys are the English field names, and the values are written
  in the output language.
- Fixed strings that a template produces come from the model, so they follow the output
  language. The built-in `youtube` prompt asks for a last description line saying chapters
  will be added, for example `[Agregar capítulos con marcas de tiempo]` in Spanish.
  minuta has no translation tables, and this line is not validated.
- Character counts are Unicode scalar values (`chars().count()`), the unit YouTube, LinkedIn
  and Instagram document. Some emoji count as more than one.
- Rendering adds no labels or other fixed text of its own, so a rendered file has no
  English words that did not come from the model.

## Domain model
New terms in `CONTEXT.md`: Field, Fields template, Limit, Violation, Repair call, Format,
Fields artifact.

### Shared fields (ADR-0006)
| Field | Kind | Description given to the model | Rendered (both formats) |
|---|---|---|---|
| `title` | text | A title. | the value (`markdown`: `# <value>`) |
| `body` | text | The main text. | the value |
| `tags` | list | Search keywords, without `#`. | `a, b, c` |
| `hashtags` | list | Hashtags, written without the `#` sign. | `#a #b #c` |
| `thumbnail_text` | text | A few words to show on a thumbnail image. | the value |

`chapters` is reserved for sub-project 4. Adding a field later is additive. Removing or
retyping one needs a new ADR and a new JSON artifact version.

### Frontmatter (additions to ADR-0004)
```yaml
output: fields                 # required to use the keys below
fields: [title, body, tags]    # non-empty, no duplicates, from the shared set; order = render order
limits:                        # optional; each key must be a declared field
  title: { max_chars: 100 }    # text: max_chars
  tags: { max: 15, max_chars: 500 }   # list: max (items), max_chars (rendered length)
format: plain                  # plain (default) | markdown
```
- `max_chars` is the length of the field as rendered: for text the trimmed value, for `tags`
  the string `a, b, c`, for `hashtags` the string `#a #b #c`. This one rule bounds a whole
  caption: Instagram `body` 1800 + blank line 2 + `hashtags` 390 = 2192 <= 2200.
- `max` applies to lists only. Both limits are integers >= 1.
- `format: plain` tells the model to write values without Markdown syntax. `markdown` allows
  Markdown in `body` and renders `title` as a heading. `format` is part of the prompt, so
  changing it reruns the template with a new call.

### Normalization and validation
After each final or repair answer:
1. Clean it as in sub-project 2 (trim, `<think>` block, a single fence). Parse it as a JSON
   object. If that fails, parse the text from the first `{` to the last `}`.
2. For each declared field: the key must exist. A text field must be a string, a list field
   an array of strings. Strings are normalized (security notes) and trimmed. Empty list items
   are dropped. In `hashtags`, leading `#` characters are removed from each item. Keys that
   were not declared are ignored and dropped.
3. Violations, all collected in one pass, in field order:
   - `the answer is not a JSON object (<parser message>)`
   - `missing key "title"`
   - `"title" must be a string` / `"tags" must be a list of strings`
   - `"title" is empty` (also a list with no items left)
   - `hashtag "machine learning" contains whitespace`
   - `"body" has 5412 characters; the limit is 5000. Shorten it to at most 5000 characters.`
   - `"hashtags" has 8 items; the limit is 5. Keep the 5 most relevant.`
   - `"hashtags" is 412 characters as rendered; the limit is 390. Use fewer or shorter hashtags.`
4. No violations: the answer is accepted.

### Prompts
- **Final call** (the single call, or the consolidation call when chunked): system = rendered
  body + (chunked: the consolidation sentence from sub-project 2) + the fields block + the
  language instruction. The fields block, in English:
  > Return only a JSON object with exactly these keys, and no text before or after it:
  > "title" (string): A title. At most 100 characters.
  > "tags" (array of strings): Search keywords, without `#`. At most 15 items, at most 500
  > characters joined with ", ".
  > Write the values as plain text without Markdown. Keep the keys exactly as written, in
  > English; write the values in the language given below.

  With `format: markdown` the sentence "Write the values as plain text without Markdown." is
  replaced by "`body` may use Markdown; the other values are plain text."
- **Map calls** are unchanged. They produce notes, not JSON.
- **Repair call**: system = "You correct a JSON object. Fix only the problems listed in the
  input, keep everything else unchanged, and return only the corrected JSON object." + the
  fields block + the language instruction. User = `Previous answer:\n<cleaned answer>\n\n
  Problems:\n- <violation>…`. It does not resend the transcript, so it always fits the
  context.
- `FIELDS_VERSION` (a new constant, starting at 1) is bumped when the fields block, repair
  prompt, normalization or rendering changes. It is part of the fingerprint of fields
  templates only. `PROMPT_VERSION` is not bumped.

### Requesting JSON from a backend
- The `Backend` trait gains
  `fn complete_json(&self, system, user, schema: &serde_json::Value, interrupt) -> Result<String>`
  with a default that calls `complete`. `claude` keeps the default.
- **Ollama** overrides it and adds `"format": <schema>` to the `/api/chat` body
  (`src/llm/ollama.rs:177`). The schema is
  `{"type":"object","properties":{…},"required":[…declared fields…]}`, with `{"type":"string"}`
  or `{"type":"array","items":{"type":"string"}}` per field. It has no length constraints.
  Schema `format` needs Ollama 0.5 or later. An HTTP 400 on such a request gets the hint
  "structured output needs Ollama 0.5 or later".
- Final and repair calls use `complete_json`. Map calls use `complete`.

### Repair loop and call cache (ADR-0006)
- Final calls are tried up to `1 + llm.repairs` times: the final call, then repair calls,
  each built from the previous answer and its violations. Transient errors are still retried
  inside each call per `llm.retries`, as in sub-project 2.
- Cache key for a `complete_json` call = SHA-256 of (backend id, context budget, system,
  user, schema as compact JSON). Keys of `complete` calls are unchanged
  (`src/llm/call.rs:779`).
- A `complete_json` answer is written to the cache only after it is accepted. A cached entry
  is therefore always valid. On a read hit it is still validated, and a hit that fails (an
  entry from a template whose limits changed has a different key, so this means a damaged
  entry) is treated as a miss.
- `--force <template>` skips cache reads for every call of that template, as before.

### Template step
- Artifacts: `<name>.json` and `<name>.md`, each written with `write_atomic`, JSON first.
  `job.json` records both (`"artifacts": ["youtube.json", "youtube.md"]`). Invariant: status
  `done` if and only if both files exist. A missing one makes the step stale
  (`src/job/stale.rs:117` already checks every recorded artifact).
- Chaining is unchanged: `input: youtube` reads `youtube.md`. The dependent's fingerprint
  hashes that file's content.
- Fingerprint of a fields template = the sub-project 2 parts + `FIELDS_VERSION`. `fields`,
  `limits` and `format` are covered by the raw file bytes. `llm.repairs` is not part of it.

### Rendering
Declared fields in `fields` order, each rendered as in the shared-fields table, separated
by one blank line, with a trailing newline. With `markdown`, only `title` changes, to
`# <title>`. Example with `fields: [body, hashtags]`:
```
Ayer cerramos la cuarta clase del curso…

#rust #programacion #clases
```

### JSON artifact (ADR-0007)
```json
{ "version": 1, "template": "<name>", "output_lang": "<code>",
  "fields": { "<field>": "<string>" | ["<string>", …] } }
```
Pretty-printed with a trailing newline. `fields` holds exactly the declared fields, after
normalization. Readers reject an unknown `version`.

### Built-in social templates
All have `input: summary`, `output: fields`, `format: plain`, and prompts written in English.
| Template | Fields | Limits | Rendered maximum |
|---|---|---|---|
| `youtube` | title, body, tags, thumbnail_text | title 100; body 5000; tags max 15, max_chars 500; thumbnail_text 40 | per field (YouTube adds quotes around multi-word tags when it counts them; sub-project 6 checks the exact rule) |
| `linkedin` | body, hashtags | body 2800; hashtags max 5, max_chars 150 | 2952 <= 3000 |
| `instagram` | body, hashtags | body 1800; hashtags max 30, max_chars 390 | 2192 <= 2200 |

Prompt requirements: `youtube` asks for a searchable title, a description whose first two
lines work as a preview, and a last description line, in the output language, saying that
timestamped chapters will be added. `linkedin` asks for a first line that works as a hook, short
paragraphs and no links. `instagram` asks for a caption that opens with a hook and an optional
call to action. None of them may invent facts that are not in the input.

## Interfaces
```
minuta run <files…> -t youtube [-t linkedin]…     # unchanged grammar
minuta templates list                              # NAME  LAYER  INPUT  OUTPUT  LLM  DESCRIPTION
minuta templates show <name>                       # unchanged
minuta config show                                 # lists llm.repairs
```
| Key | Env | Default |
|---|---|---|
| `llm.repairs` | `MINUTA_LLM_REPAIRS` | `2` (repair calls after the first final answer; 0 disables) |

- Progress: while a repair call runs, the spinner shows `repair 1/2`. The done line counts
  repair calls in `N calls`.
- Exit codes unchanged: 0, 1 (including a template that still breaks limits after repairs),
  2 usage, 130 interrupted.
- Failure message:
  `template youtube: the answer still breaks 1 limit after 2 repairs: "body" has 5230
  characters; the limit is 5000.` + hint: `raise llm.repairs, relax the limit in a project
  template, or use a stronger backend with --llm`.

## Behavior & edge cases
- **Frontmatter errors** (exit 2 at resolution, each naming the template):
  `fields`, `limits` or `format` without `output: fields`; `output: fields` without `fields`
  or with an empty list; an unknown field (the message lists the shared set); `chapters`
  ("arrives in a later version"); a duplicate field; a limit for an undeclared field; an
  unknown limit key; `max` on a text field; a limit of 0; an unknown `format`.
- **Repairs exhausted**: exit 1, no artifacts, the template is not marked done. Accepted
  answers of other templates and map calls stay cached.
- **`llm.repairs = 0`**: the first violation fails the template.
- **Interrupt during a repair**: exit 130, nothing written. That template's final answers
  were not accepted, so they are not cached.
- **Answer with extra keys**: ignored, not a violation. Ollama's schema already discourages
  them.
- **Empty input**: as in sub-project 2 (`transcript is empty`). An empty `summary.md` cannot
  exist, because empty output is a transient failure.
- **Chained after a fields template**: reads the rendered `<name>.md`. The JSON file is not a
  template input.

## Data & migrations
- `job.json` stays version 1. A fields template records two artifacts in the existing
  `artifacts` list.
- New files: `<name>.json` for fields templates. `llm-cache/` entries for `complete_json`
  calls use the new key with the schema.
- New config key `llm.repairs`. No new crates: `serde_json` and `serde_yaml_ng` are already
  dependencies.
- Existing `markdown` templates: same fingerprint, same cache keys, so nothing reruns.

## Testing strategy
- **Unit** (one assertion per case in table tests):
  - Frontmatter: each key, each error in the list above, the defaults (`format: plain`).
  - Built-ins: each built-in template parses, and each limit in the table, one case each.
  - Normalization: CRLF, control characters, trim, empty list items, `#` stripping.
  - Validation: each violation message, including counts at the boundary (exactly
    `max_chars` passes, `max_chars + 1` fails) and multi-byte characters counted as one.
  - Parsing: a bare object, a fenced object, prose around the object, not an object.
  - Rendering: each field kind in each format, field order, trailing newline.
  - JSON artifact: the exact layout, round-tripped through `serde_json`.
  - Schema: per kind, required keys.
  - Prompts: the fields block lists each declared field with its limits, and the language
    instruction stays last.
  - Repair loop with a fake `Backend`: accept on the first answer; accept after one repair;
    fail after `llm.repairs`; `llm.repairs = 0`; a rejected answer is not cached; an accepted
    one is; cache-key sensitivity to the schema; `complete` keys unchanged (a fixed
    expected key).
  - Fingerprint: changes with `FIELDS_VERSION`; markdown fingerprint unchanged (a fixed
    expected value).
- **Integration** (fake Ollama with scripted answers that records requests; fake claude
  fixture):
  - Full `run -t youtube`: two templates done, both artifacts, the second run makes zero
    requests.
  - Over-limit, then valid answer: 2 final requests, the second is a repair whose user
    message names the violation.
  - Always over the limit: exit 1, the message lists violations, no artifacts. The rerun
    makes a new final request.
  - Invalid JSON, then valid answer.
  - Chunked input: map requests have no `format`, the final request has it.
  - `format` sent for final and repair calls, with the expected schema.
  - The same template on fake claude: JSON instructions in `--system-prompt`, valid reply.
  - A template with `input: youtube` receives `youtube.md`.
  - Editing a limit reruns only that template and its dependents.
  - `templates list` shows the OUTPUT column for built-in and project templates.
- **Opt-in e2e**: `MINUTA_E2E_LLM=1` also runs `-t youtube` on the real local Ollama and
  asserts that `youtube.json` parses and meets the built-in limits.
- Test conventions apply: every test environment sets `MINUTA_OLLAMA_URL`; no sleeps to order
  threads; generous deadlines.

## Decisions
- ADR-0006: Fields come from one closed, shared set with `max_chars` measured on the rendered
  field. Answers that break limits are repaired by the model, up to a configured number of
  times, and only accepted answers are cached.
- ADR-0007: A fields template writes a versioned JSON artifact plus a rendered `<name>.md`,
  which chained templates read.
- Relied on: ADR-0001 (no per-template flags), ADR-0004 (frontmatter extended with typed keys),
  ADR-0005 (fingerprints and call cache; `complete_json` keys add the schema).
- `CONTEXT.md` gains: Field, Fields template, Limit, Violation, Repair call, Format, Fields
  artifact. Template and Template step are updated.

## FAQ
### Why a closed field set instead of user-defined fields?
Sub-project 6 maps `title`, `body` and `tags` to YouTube API fields, and rendering needs to know
whether a value is text or a list. With known names the model also gets a description per
field. Adding a field later is additive. Removing one would break user templates.

### Why does a repair call send only the previous answer and not the transcript?
Fixing a length or a type is an edit. Without the transcript the call is small, always fits
the context (even when the input needed chunking), and is fast on local models.

### Why not cache rejected answers, for full resumability?
The repair chain is deterministic given cached answers. A template that failed would fail the
same way on every rerun with zero calls, until `--force`, which also discards the map notes.
Caching only accepted answers keeps the expensive map calls and gets a fresh final answer.

### Why `max_chars` on lists measured on the rendered string?
Platforms limit the whole caption, hashtags included. With the rendered length a template can
budget `body` + `hashtags` to the platform total with no extra "total" limit kind.

### Why is the rendered file `<name>.md` even for `format: plain`?
Chaining reads `<input>.md` (sub-project 2). Plain text with no Markdown syntax is still valid
Markdown, and keeping one name avoids making the input file depend on the upstream format.

### Why do the social templates read `summary` and not the transcript?
`-t youtube -t linkedin -t instagram` then chunks a long transcript once, in `summary`, and
all three reuse it. A user who wants the transcript as input overrides the template with
`input: transcript` in a project template.

### Why not let Ollama's grammar enforce lengths?
Grammar limits cut text mid-word. A repair call asks the model to rewrite instead.

## Open questions
None.

## Implementation status
Updated by the PRs that implement this spec.
- [ ] Plan: docs/plans/2026-10-25-fields-and-social-templates.md
