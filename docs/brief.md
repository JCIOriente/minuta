# minuta: design brief

Input for `/baton:spec`. It records the decisions made so far and the open
questions. The approved spec, ADRs and glossary that baton produces replace it.

## Problem

Recordings (classes, webinars, meetings) are turned into written material by
hand, or by a single-purpose bash pipeline (`orientate-clases`: mp4 -> mp3 ->
whisper.cpp -> `claude -p` -> lesson Markdown + YouTube copy). Every new
document type (meeting notes in a given format, a LinkedIn post, an Instagram
caption) means another hard-coded step. The pipeline is also fragile: one
failed LLM call loses all chunk results, and the transcript has no timestamps.

## Goal

A Rust CLI, `minuta`, that turns a recording into any number of documents
described by prompt templates. Templates can chain: one template's output is
another's input. It is free to run (whisper.cpp + Ollama, all local) and can use
Claude when quality matters.

```
minuta join v.mp4 x.mp4 -cy                       # class lesson + YouTube copy
minuta call.mp4 -m -t weekly-sync                 # meeting notes from a custom template
minuta v.mp4 -cl --llm ollama:gemma4:26b          # lesson + LinkedIn post, all local
minuta v.mp4 -m --lang en                         # Spanish meeting, English notes
```

The CLI shape is a sketch; the spec decides the final form.

## Non-goals (for now)

- A GUI or a server.
- Publishing to networks without a practical public API (LinkedIn, Instagram):
  minuta writes ready-to-paste copy for them.
- Training or fine-tuning models.

## Core concepts (glossary seed)

- **Source**: one or more media files. Several are joined into one.
- **Transcript**: timestamped segments `[{start, end, text}]` from whisper.cpp
  (`-oj`), plus the detected speech language.
- **Template**: a prompt file with a frontmatter header. It declares its input,
  output kind, fields, limits and optional short flag.
- **Artifact**: what a template run produces, stored in the job folder.
- **Job**: one run over one source, with a folder of cached intermediate and
  final artifacts. Rerunning reuses what's already done.
- **Backend**: an LLM provider (Ollama, Claude CLI, later the Claude API).
- **Chapters**: timestamped topic sections computed once from the transcript and
  available to any template.
- **Edit plan**: cuts to the source (trim the waiting room or closing chatter,
  long silences). Everything with timestamps is remapped through it.
- **Publisher**: uploads a rendered video plus an artifact to a platform.
  YouTube only, at first.

## Templates

A Markdown prompt with a frontmatter header. Discovery order, later wins:
built-in (compiled in) -> `~/.config/minuta/templates/` -> `./templates/`.

```markdown
---
name: linkedin
flag: l
input: class              # transcript | chapters | <template name>
output: fields            # markdown | fields (structured JSON)
fields: [hook, body, hashtags]
limits:
  body: { max_chars: 3000 }
  hashtags: { max: 5 }
format: plain             # how fields render: plain | markdown
---
Prompt text, in any language.
```

- Chaining comes from `input`: asking for `-y` runs `class` first and reuses its
  artifact. Dependencies are resolved as a DAG; cycles are an error.
- Social templates share one structured output shape (`title`, `body`, `tags`,
  `hashtags`, `chapters`, `thumbnail_text`, ...). Each declares which fields it
  uses and their limits, e.g. YouTube title <= 100 chars and description <= 5000;
  Instagram caption <= 2200 chars and <= 30 hashtags; LinkedIn <= 3000 chars.
- Limits are validated after generation. Violations go back to the model for a
  retry (local models often ignore length instructions).
- YouTube is one template, not special-cased code.

## Language model (human language)

Three independent settings. None of them is assumed to be Spanish:

1. **Speech language**: what whisper transcribes. `auto` (detect) or a code.
2. **Prompt language**: whatever the template author wrote. Built-in templates
   are written in English, which models follow best.
3. **Output language**: what the documents are written in. Default: the
   detected speech language. Override per run with `--lang`, or per template in
   frontmatter. It reaches the prompt as a variable (e.g. `{{output_lang}}`),
   and minuta always adds an explicit "write in X" instruction.

Template-generated fixed strings (placeholders such as
`[Agregar capítulos con marcas de tiempo]`) must follow the output language too.

## LLM backends

- A small trait: complete(system, user) -> text, plus a context-size hint.
- `ollama:<model>` over its local HTTP API (free; `gemma4:26b` is installed).
- `claude` via `claude -p` (uses the existing subscription); the API later.
- Selectable per run (`--llm`) and per template (frontmatter), with a config
  default.
- Long inputs are chunked by the backend's context size (smaller for local
  models), then consolidated.
- Every call is cached by hash(backend, model, system, instruction, input), with
  retries and backoff. Chunks run in parallel with a concurrency limit.

## Media pipeline

- Join with stream copy when the inputs match, else re-encode (VideoToolbox when
  available). Logic already exists in `orientate-clases/lib/pipeline.sh`.
- Extract mono 16 kHz audio for whisper.
- Run whisper.cpp with `--vad`: it removes hallucinated lines in silences, which
  were seen in real transcripts ("¡Gracias, Mauricio!" repeated).
- Chapters: group segments into ~30 s blocks `[hh:mm:ss] text`, have the LLM
  return `[{start, title}]`, snap to segment boundaries, and validate YouTube's
  rules (first at 0:00, >= 3 chapters, >= 10 s each, ascending).
- Edit plan (later): trim points from the LLM, silences from `silencedetect`,
  then a single render. Chapters and captions are remapped to the edited
  timeline.
- Captions: `.srt` from segments, remapped through the edit plan.

## Publishing (later)

YouTube Data API v3: resumable upload, metadata, captions, thumbnail. Caveat:
uploads from an unverified API project are locked to private until Google
audits the project.

## UX carried over from the bash pipeline

Step counter, per-step timing, live progress for ffmpeg and whisper, a spinner
with elapsed time during LLM calls, a final summary, and `--force` to redo.
Known bug to avoid: a job name taken from `-n clase-4.mp4` produced
`clase-4.mp4.mp4`, so names must have media extensions stripped.

## Suggested milestones

1. Skeleton: CLI, config, job folder, the media steps, whisper JSON with VAD.
2. Templates: loading, frontmatter, DAG chaining, the `markdown` output kind,
   Ollama and Claude backends, the cache with retries.
3. `fields` output, limit validation, and social templates (YouTube, LinkedIn,
   Instagram).
4. Chapters.
5. Edit plan, render, captions.
6. YouTube publisher.

## Open questions

- Final CLI grammar: subcommands (`minuta join`) vs. one command with inputs,
  and how short flags for user templates avoid collisions.
- Template variable syntax and which variables exist (`output_lang`, `chapters`,
  `source_name`, `date`, ...).
- Where jobs live: next to the source, `./minuta-out/<name>`, or a config path.
- Config file format and location (`~/.config/minuta/config.toml`?).
- Whether whisper.cpp is called as `whisper-cli` or embedded via `whisper-rs`.
- Distribution: `cargo install`, Homebrew, prebuilt binaries.
