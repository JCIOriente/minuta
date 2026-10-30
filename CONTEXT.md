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
| Template | A Markdown prompt file with optional YAML frontmatter (`input`, `output`, `llm`, `lang`, `description`, and for fields templates `fields`, `limits`, `format`; for `chapters`, `limits`). `output` is `markdown`, `fields`, or `chapters` (only the template named `chapters`). Its name is the file stem. Found in layers: built-in, `~/.minuta/templates/`, `./templates/` (later wins). | A template step, which is one run of a template in a job |
| Catalog | `templates/index.toml` in the repository: `version = 1` and one `[[template]]` entry per curated template (`name`, `description`, `sha256`, optional `platform`, `tags`, `input`). `pull` fetches it fresh from a ref, so a new template needs no release. | The built-in templates, which are compiled into the binary |
| Pull | `minuta templates pull`: download chosen templates from the catalog into `./templates` (or `--to DIR`) after confirmation, check each file's SHA-256 and parse it, and record it in `.pulled.toml`. | `minuta models pull`, which downloads a model, not a template |
| Home folder | `~/.minuta/`: where minuta keeps user files that persist across jobs: the config file (`config.toml`), the models (`models/`) and user templates (`templates/`). | The job folder, and the old `~/.config/minuta/` |
| Template step | Running one template in a job: produces `<name>.md` (and `<name>.json` for a fields template), recorded in `job.json` under `templates`. | A pipeline step |
| Fields template | A template with `output: fields`: the model returns a JSON object with the fields it declares, except filled fields. | A `markdown` template, whose output is free text |
| Field | One named value from the shared set (`title`, `body`, `chapters`, `tags`, `hashtags`, `thumbnail_text`). Kind text, list or chapters. | A frontmatter key |
| Filled field | A field minuta fills from another artifact instead of asking the model. Only `chapters`, filled from `chapters.json`; declaring it makes the template depend on `chapters`. | A field the model writes |
| Limit | A per-field bound in `limits`: `max_chars` (length of the field as rendered) or `max` (list items). | The context budget |
| Violation | One way an answer fails validation: not JSON, missing key, wrong type, empty, over a limit, or (for `chapters`) a broken chapter rule. | A transient call failure, which is retried without telling the model |
| Repair call | A call that sends the previous answer and its violations back to the model to fix. Up to `llm.repairs` per template run. | A retry, which repeats the same call after a transient error |
| Format | How a fields template's values are rendered to `<name>.md`: `plain` (paste-ready, no Markdown) or `markdown`. | The output kind (`markdown` or `fields`) |
| Chapters | The built-in template `chapters` (`output: chapters`) and what it writes: `chapters.json` (versioned, whole-second starts) and `chapters.md` (chapter lines). Computed once per job from the transcript. | Transcript blocks, the 30 s `[hh:mm:ss]` lines sent to the model |
| Chapter | One entry of a chapter list: `start` in whole seconds from the start of the joined media, and a one-line `title`. | A transcript segment |
| Chapter line | `<stamp> <title>`, the form YouTube reads in a description: `m:ss` below one hour, `h:mm:ss` from one hour. | The `[hh:mm:ss]` stamps in transcript blocks |
| Chapter rules | YouTube's rules for a chapter list: first at 0:00, at least 3, ascending, each at least 10 s (the last up to the transcript end), plus the template's `max` and `max_chars`. | Limits of a fields template |
| Snapping | Replacing each model start with the nearest transcript segment start, floored to whole seconds, then setting the first start to 0. Done in code before the chapter rules are checked. | A repair call |
| Fields artifact | `<name>.json`: versioned JSON with a fields template's validated values, for later consumers such as the YouTube publisher. | The rendered `<name>.md` |
| Models directory | The folder holding downloaded models: `~/.minuta/models` by default, or `MINUTA_MODELS_DIR` / `whisper.models_dir`. Whisper models sit flat in it; VAD models in `vad/`. | The job folder, and the old `~/.cache/minuta/models` |
| VAD model | A Silero voice activity detection model, passed to `whisper-cli --vad-model`, that marks where speech is. Downloaded into the `vad/` subfolder of the models directory. | A whisper model, which transcribes the speech |
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

The `chapters` template reads the transcript, asks for JSON in its final call, snaps the starts
and checks the chapter rules, with repair calls as for fields. A fields template that declares
the `chapters` field depends on `chapters` as well as on its `input`; the model never writes
that field, and minuta copies it from `chapters.json`.

## Decisions
Links to ADRs that define domain behavior: docs/adr/
