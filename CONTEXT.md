# Domain context

Shared vocabulary for people and agents working on this project. Use these terms exactly, in
code, specs and issues. When a term is ambiguous or keeps needing explanation, add it here.

| Term | Meaning | Not to be confused with |
|---|---|---|
| Source | One or more media files given to a run. Several are joined into one. | A job: the source is the input, the job is the folder of work |
| Transcript | Timestamped segments `{start, end, text}` in seconds, plus the detected speech language. minuta's own schema. | whisper.cpp's raw JSON, which is only parsed into it |
| Job | One run over one source: a folder `./minuta-out/<name>/` holding `job.json` and every artifact. | A process; a job persists across runs |
| Step | One fixed stage of the pipeline (`probe`, `join`, `audio`, `transcribe`, `outputs`) with declared inputs and outputs. | A template step, which is user-defined and recorded under `templates` in `job.json` |
| Artifact | A file a step produces and stores in the job folder. | |
| Fingerprint | Hash of a step's inputs, parameters and tool version. A mismatch makes the step stale. For a template step it includes the hash of its input's content. | A checksum of the output |
| Stale | A step whose recorded fingerprint differs from the current one, or whose output is missing. | Failed |
| Speech language | The language whisper transcribes. `auto` or a code. | Output language and prompt language |
| Template | A Markdown prompt file with optional YAML frontmatter (`input`, `output`, `llm`, `lang`, `description`, and for fields templates `fields`, `limits`, `format`). Its name is the file stem. Found in layers: built-in, `~/.config/minuta/templates/`, `./templates/` (later wins). | A template step, which is one run of a template in a job |
| Template step | Running one template in a job: produces `<name>.md` (and `<name>.json` for a fields template), recorded in `job.json` under `templates`. | A pipeline step |
| Fields template | A template with `output: fields`: the model returns a JSON object with the fields it declares. | A `markdown` template, whose output is free text |
| Field | One named value from the shared set (`title`, `body`, `tags`, `hashtags`, `thumbnail_text`). Kind text or list. | A frontmatter key |
| Limit | A per-field bound in `limits`: `max_chars` (length of the field as rendered) or `max` (list items). | The context budget |
| Violation | One way an answer fails validation: not JSON, missing key, wrong type, empty, or over a limit. | A transient call failure, which is retried without telling the model |
| Repair call | A call that sends the previous answer and its violations back to the model to fix. Up to `llm.repairs` per template run. | A retry, which repeats the same call after a transient error |
| Format | How a fields template's values are rendered to `<name>.md`: `plain` (paste-ready, no Markdown) or `markdown`. | The output kind (`markdown` or `fields`) |
| Fields artifact | `<name>.json`: versioned JSON with a fields template's validated values, for later consumers such as the YouTube publisher. | The rendered `<name>.md` |
| Backend | An LLM provider that completes (system, user) into text, named by a backend id: `ollama:<model>`, `claude`, `claude:<model>`. | The model, which is only part of the id |
| Output language | The language documents are written in. Default: the detected speech language. | Speech language, prompt language |
| Prompt language | The language a template's prompt is written in. minuta never assumes or changes it. | Output language |
| Chunk | A slice of a template's input that fits the backend's context budget, cut at line boundaries. | A transcript segment |
| Consolidation | The final call that turns the notes from all chunks into one document (the reduce step). | A template chained on another template |
| Call cache | `<job>/llm-cache/`: one file per LLM call, keyed by backend id, context budget, system prompt and input. Lets a failed run resume without repeating finished calls. | The step fingerprint cache in `job.json` |

## Relationships
A job has one source and an ordered list of steps. Each step has at most one recorded
fingerprint and produces artifacts. A step is stale when its fingerprint or any earlier
step's fingerprint changed.

A run may also request templates. Each template's input is the transcript or another
template's output, so they form a graph that runs after the pipeline steps, in dependency
order. A template step's input is split into chunks when it does not fit its backend, and
the chunk notes are joined in one consolidation call. Every call goes through the call cache.

A fields template asks for JSON only in its final call (the single call or the consolidation).
The answer is validated against the declared fields and limits; violations lead to repair
calls. Only accepted answers are stored in the call cache. The accepted values are written to
the fields artifact and rendered to `<name>.md`, which is what chained templates read.

## Decisions
Links to ADRs that define domain behavior: docs/adr/
