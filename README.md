# minuta

Turn recordings of meetings, classes and webinars into transcripts and written
documents, such as a summary, timestamped chapters, YouTube copy or a LinkedIn
post.

minuta joins your recordings, transcribes them on your machine with
[whisper.cpp](https://github.com/ggml-org/whisper.cpp), then runs prompt
**templates** on the transcript through a local [Ollama](https://ollama.com)
model or [Claude Code](https://claude.com/claude-code). Every step is cached, so
running the same command again only redoes what changed.

```console
$ minuta run standup.mp4 -t summary -t youtube
/Users/you/talks/minuta-out/standup
$ ls /Users/you/talks/minuta-out/standup
assets  captions.srt  summary.md  transcript.txt  youtube.md
```

Documents sit at the top. JSON results and audio are in `assets/`, and
minuta's own bookkeeping is in a hidden `.minuta/` folder
([details](#the-job-folder)).

- **Local by default.** Transcription always runs on your machine. Documents
  stay local with a local Ollama, and leave the machine only if you choose
  Claude or a remote Ollama.
- **Templates are plain Markdown files.** Write your own, and chain them: one
  template's output can be another's input.
- **Resumable.** Interrupt a run or change a template, and the next run picks up
  where it can.

## Contents

- [Install](#install)
- [Quick start](#quick-start)
- [Commands](#commands)
- [Templates](#templates)
- [The job folder](#the-job-folder)
- [Configuration](#configuration)
- [Troubleshooting](#troubleshooting)
- [Development](#development)
- [License](#license)

## Install

minuta runs on macOS and Linux. Install it from [crates.io](https://crates.io/crates/minuta)
with Cargo. You need Rust 1.80 or newer, which you can get from
[rustup.rs](https://rustup.rs).

```sh
cargo install --locked minuta
```

`--locked` builds with the dependency versions minuta was tested with.

### Runtime requirements

minuta calls a few external tools. Install them once:

| Tool | Used for | Install |
| --- | --- | --- |
| `ffmpeg` and `ffprobe` | Reading, joining and converting media | `brew install ffmpeg` |
| `whisper-cli` | Transcription | `brew install whisper-cpp` |
| A whisper model | Transcription | `minuta models pull base` |
| Ollama *or* Claude Code | Templates only (`run -t ...`) | `brew install ollama`, start it with `ollama serve`, then `ollama pull gemma4:26b` |

**[Read the full installation guide](https://github.com/JCIOriente/minuta/blob/main/docs/install.md)**
for Linux package managers, building whisper.cpp from source, choosing a
model, setting up Ollama or Claude Code, and voice activity detection.

Then check your setup:

```console
$ minuta doctor
ffmpeg       ffmpeg version 8.0 ... (/opt/homebrew/bin/ffmpeg)
ffprobe      ffprobe version 8.0 ... (/opt/homebrew/bin/ffprobe)
whisper-cli  whisper.cpp version: 1.8.0 (/opt/homebrew/bin/whisper-cli)
model        /Users/you/.minuta/models/ggml-base.bin
llm          ollama:gemma4:26b (ok)
```

If a tool or model is missing, `doctor` exits with status 1 and shows a fix for
each one. The `llm` line is for information: it shows whether the default
backend is reachable, but does not change the exit status.

## Quick start

1. **Download a whisper model and make it the default.** `base` is a good first
   choice. `small` and larger models are more accurate but slower.

   ```sh
   minuta models pull base
   mkdir -p ~/.minuta
   printf '[whisper]\nmodel = "base"\n' > ~/.minuta/config.toml
   ```

   This creates the config file, replacing any you already have. If you have
   one, add `model = "base"` under its `[whisper]` section instead. You can
   also pass `--model base` on each run instead of setting a default.

2. **Transcribe a recording.**

   ```sh
   minuta transcribe meeting.mp4
   ```

   This writes `transcript.txt` and `captions.srt` to
   `./minuta-out/meeting/`, and `transcript.json` to its `assets/` folder. It
   prints the job folder's path.

3. **Generate documents.**

   ```sh
   minuta run meeting.mp4 -t summary
   ```

   The transcript is already cached, so only the summary runs. The result is
   `./minuta-out/meeting/summary.md`.

More examples:

```sh
# Join two parts of one class, in order, into a single job named "class-4"
minuta run part1.mp4 part2.mp4 --name class-4 -t summary

# YouTube title, description, tags and chapters, written with Claude
minuta run webinar.mov -t youtube --llm claude

# A Spanish meeting, with the notes written in English
minuta run reunion.m4a --lang es --output-lang en -t summary

# The path goes to stdout, so you can use it in scripts
cat "$(minuta run talk.mp4 -t linkedin -q)/linkedin.md"
```

## Commands

| Command | What it does |
| --- | --- |
| `minuta run FILES... [-t TEMPLATE]...` | Transcribe, generate documents from templates, and print the job folder's path |
| `minuta transcribe FILES...` | Same as `run`, without templates |
| `minuta doctor` | Report tools, versions, the model and the LLM backend; exit 1 if a tool or model is missing |
| `minuta models list` | Show every catalog model and whether it is installed |
| `minuta models pull NAME` | Download a model and verify its checksum |
| `minuta templates list` | Show every template with its layer, input, output and backend, then any invalid files |
| `minuta templates show NAME` | Print the file that defines a template (its path goes to stderr) |
| `minuta templates pull [NAME]...` | Download curated templates from the repository into `./templates`, after asking (see [Downloading templates](#downloading-templates)) |
| `minuta config path` | Print the config file path |
| `minuta config show` | Print every setting, its value and where it came from |
| `minuta config init [--force]` | Write a commented template of every setting to the config file path; an existing file is left alone unless `--force` is given |

Run `minuta help <command>` for the full usage of any command.

### Options for `run` and `transcribe`

| Option | Description |
| --- | --- |
| `FILES...` | One or more recordings, joined in the order given |
| `-n, --name NAME` | Job name. Defaults to the first file's name without its media extension |
| `--out DIR` | Where job folders are created. Default: `./minuta-out` |
| `--lang auto\|CODE` | Speech language: `auto` (detect) or a whisper language code such as `en` or `es` |
| `--model PATH\|NAME` | A whisper model file, or a catalog name such as `base` |
| `--force [STEP\|TEMPLATE]` | Redo work instead of reusing the cache (see [Caching](#caching-and---force)) |
| `-q, --quiet` | Print only warnings and errors |

`run` also accepts:

| Option | Description |
| --- | --- |
| `-t, --template NAME` | Generate a document with this template. Repeat it for more templates |
| `--llm ID` | Backend for every template in this run: `ollama:<model>`, `claude` or `claude:<model>` |
| `--output-lang CODE` | Language the documents are written in. Default: the detected speech language |

Only the job folder's path goes to stdout. Progress, warnings and errors go to
stderr.

### Exit status

| Code | Meaning |
| --- | --- |
| `0` | Success |
| `1` | A missing tool, a tool error, an LLM or generation error, or a config, job or I/O error |
| `2` | Usage error: bad arguments, an unknown or invalid template, an unknown language, step or backend ID, or an unknown model name for `models pull` |
| `130` | `run` or `transcribe` was interrupted (Ctrl-C, `SIGTERM` or `SIGHUP`). The running tool is stopped too |

### Whisper models

| Name | Download size | Notes |
| --- | --- | --- |
| `tiny` | 74 MiB | Fastest, least accurate |
| `base` | 141 MiB | A good default |
| `small` | 465 MiB | |
| `medium` | 1.4 GiB | |
| `large-v3` | 2.9 GiB | Most accurate, slowest |
| `silero-v5.1.2` | 0.8 MiB | Voice activity detection (VAD) model, see below |

Models are saved in `~/.minuta/models/`, a folder for models that work with
whisper. Whisper models sit there as plain `ggml-*.bin` files, the same layout
whisper.cpp uses, so whisper.cpp can read them directly. VAD models go in
`~/.minuta/models/vad/`. Change the folder with `MINUTA_MODELS_DIR` or
`whisper.models_dir` in the config file; `minuta models list` shows the folder
it reads.

`models pull` shows a progress bar with the percent, speed and time left on a
terminal. Pass `--quiet` to hide it.

Models that an older version saved in `~/.cache/minuta/models/` are not used
any more, so `models pull` downloads them again. You can delete that folder.

Wherever minuta accepts a model, you can give a catalog name or the path to any
whisper.cpp `ggml` model file.

**Voice activity detection** stops whisper from inventing lines during
silences. It is off until you configure a VAD model:

```sh
minuta models pull silero-v5.1.2
```

```toml
# ~/.minuta/config.toml
[whisper]
vad_model = "silero-v5.1.2"
```

## Templates

A template is a prompt that turns the transcript, or another template's output,
into a document. You choose which templates to run with `-t`. Templates they
depend on run first.

### Built-in templates

| Name | Input | Writes |
| --- | --- | --- |
| `summary` | transcript | `summary.md`: overview, key points, decisions, action items, open questions |
| `chapters` | transcript | `chapters.md` and `assets/chapters.json`: timestamped chapters that follow YouTube's rules |
| `youtube` | transcript | `youtube.md` and `assets/youtube.json`: title, description, chapters, hashtags, tags and thumbnail text |
| `linkedin` | transcript | `linkedin.md` and `assets/linkedin.json`: post body and hashtags |
| `instagram` | transcript | `instagram.md` and `assets/instagram.json`: caption and hashtags |

For example, `-t youtube` also runs `chapters`, because `youtube` fills its
chapters field from it. A template that runs only because another one needs it
writes its files under `.minuta/derived/`, so `chapters.*` shows up at the top
and in `assets/` only when you ask for `-t chapters`. `summary` runs only on
`-t summary`.

### Where templates come from

minuta loads templates from three places. A template with the same name in a
later place replaces the earlier one:

1. Built in to minuta
2. Your user templates: `~/.minuta/templates/*.md`
3. Project templates: `./templates/*.md` in the current directory

To customize a built-in template, copy it and edit the copy (to get more
templates instead, see [Downloading templates](#downloading-templates)):

```sh
mkdir -p ~/.minuta/templates
minuta templates show summary > ~/.minuta/templates/summary.md
```

> **Security:** templates in `./templates` are read from whatever directory you
> run minuta in. Review the templates of a repository you cloned before running
> minuta in it, the same as you would review its scripts.

### Downloading templates

Curated templates are plain files in the repository's
[`templates`](https://github.com/JCIOriente/minuta/tree/main/templates) folder,
listed in `templates/index.toml` (the catalog). They are not built in, so
anyone can add one and it needs no release. `minuta templates pull` downloads
the ones you choose into `./templates`:

```console
$ minuta templates pull --list
templates from https://raw.githubusercontent.com/JCIOriente/minuta/main/templates/
NAME       PLATFORM   STATE             DESCRIPTION
instagram  instagram  new               Caption with a hook in the first 125 characters
youtube    youtube    update available  Title, description, tags and thumbnail text
linkedin   linkedin   modified          Post with a hook and a closing question

$ minuta templates pull instagram youtube
minuta will download from:
  https://raw.githubusercontent.com/JCIOriente/minuta/main/templates/
  instagram.md  (new)
  youtube.md    (update available)
into /Users/you/Downloads/vids/templates
Continue? [y/N] y
instagram.md  written
youtube.md    updated
```

On a terminal, a progress bar on stderr counts the files as they download; it is not drawn when output is piped or `NO_COLOR` is set.

The names above are examples; `--list` shows what the catalog has now. It is
fetched fresh each time, so a new template shows up without a new minuta
release. A pulled file is an ordinary project template: `minuta templates list`
shows it with the layer `project`, and it replaces a user or built-in template
of the same name. The 5 built-in templates stay in the binary, so offline use
does not change.

| Option | What it does |
| --- | --- |
| `NAME...` | Download these templates |
| `--all` | Download every template in the catalog |
| `--filter TEXT` | Download the templates whose name, platform or tag contains `TEXT` |
| `--list` | Show the catalog with the state of each template and download nothing |
| `--to DIR` | Put the files in `DIR` instead of `./templates`. `--to ~/.minuta/templates` makes them available in every folder |
| `--force` | Replace templates you edited. The old file is saved as `NAME.md.bak` first |
| `--yes` | Do not ask for confirmation |
| `--ref REF` | Branch, tag or commit to download from (default `main`) |

With nothing chosen, on a terminal, `pull` opens a numbered list to pick from.
Before downloading it shows the source URL, each file with its state and the
destination, and asks `Continue? [y/N]`. Without a terminal, name the templates
(or use `--all` or `--filter`) and pass `--yes`; otherwise it refuses before
downloading anything. The prompt and messages go to stderr; stdout carries only
the paths written.

Each template in the destination folder is in one of four states:

| State | Meaning | `pull` does |
| --- | --- | --- |
| new | The file is not there | Downloads it |
| up to date | The file is the same as the repository's | Nothing |
| update available | The file is as minuta wrote it and the repository has a newer version | Updates it |
| modified | You edited the file, or minuta did not write it | Skips it. With `--force`, saves it as `NAME.md.bak`, then replaces it |

minuta remembers what it wrote in `.pulled.toml` in the destination folder: the
hash of each pulled file. That is how it tells your edits from a newer upstream
version. If you delete `.pulled.toml`, or it is invalid, every file reads as
modified and nothing is overwritten. Nothing else needs cleaning up: delete any
template file you do not want.

Before writing anything, minuta checks that every file matches the SHA-256 in
the catalog and is a valid template, and writes none of them if one is not.
Downloads use HTTPS only, and a symlinked destination, `.pulled.toml` or
template is refused. The hash comes from the same repository version as the
file, so it catches corrupted downloads, not a malicious change to the
repository. To get fixed content, pin a tag or commit with
`--ref v0.3.0`. If a pulled template reads another template that you do not
have, minuta prints a `note:` naming it.

### Contributing a template

A template is added to the repository, not to minuta's code:

1. Write `templates/NAME.md` in the [format below](#writing-a-template). The
   briefs in [`docs/template-briefs`](https://github.com/JCIOriente/minuta/tree/main/docs/template-briefs)
   show how to describe what a good template for a platform looks like.
2. Add an entry to `templates/index.toml` with its `name`, `description`
   and `sha256`, and optionally `platform`, `tags` and `input`.
3. Get the `sha256` with `shasum -a 256 templates/NAME.md`. The consistency
   test `tests/templates_catalog.rs` prints this command when a hash is wrong.
4. Run `make ci`. The test fails if an entry has no file, a hash differs, a
   file is not a valid template, a name differs from its file name, or a
   `templates/*.md` file has no entry.

Change the hash again whenever you edit the file.

### Writing a template

The file name is the template's name, so `weekly-sync.md` defines
`weekly-sync`. Names start with a lowercase letter or digit and use only
lowercase letters, digits, `-` and `_`. The names `transcript`, `probe`,
`join`, `audio`, `transcribe`, `outputs` and `job` are reserved. The file is a
Markdown prompt with an optional YAML header:

```markdown
---
description: Notes for our weekly sync
input: transcript
---
These are the notes of "{{job_name}}" ({{duration}}).

List what each person reported, then the blockers and who owns each one.
Use only what is said in the transcript.
```

```sh
minuta run sync.mp4 -t weekly-sync
```

minuta tells the model which language to write in, so the prompt itself can be
in any language.

#### Header keys

Every key is optional. Unknown keys are an error.

| Key | Values | Default |
| --- | --- | --- |
| `description` | Free text, shown by `templates list` | none |
| `input` | `transcript`, or the name of another template whose output this one reads | `transcript` |
| `output` | `markdown`, or `fields` for structured output (see below) | `markdown` |
| `fields` | With `output: fields`, the fields to generate | none |
| `limits` | With `output: fields`, the limits for each field: `{ max_chars: N, max: N }` | none |
| `format` | With `output: fields`, how fields render in the `.md` file: `plain` or `markdown` | `plain` |
| `llm` | The backend for this template, such as `ollama:qwen3:14b` or `claude` | the configured default |
| `lang` | The language this template writes in, such as `en`. `--output-lang` overrides it | `llm.output_lang`, else the detected speech language |

The `chapters` template is special: it uses `output: chapters`, which no other
template may use. If you override `chapters`, keep `output: chapters`. Its
only allowed limit is `limits: { chapters: { max: N, max_chars: N } }`, with
`max` at least 3.

#### Variables

| Variable | Value |
| --- | --- |
| `{{job_name}}` | The job name |
| `{{duration}}` | The recording's length, as `h:mm:ss` |
| `{{output_lang}}` | The output language's name, such as `English` |
| `{{output_lang_code}}` | The output language's code, such as `en` |
| `{{speech_lang}}` | The detected speech language's name, such as `Spanish` |

Any other `{{...}}` is an error that lists these.

#### Structured output (`fields`)

A `fields` template writes `assets/<name>.json` for scripts and `<name>.md`
for reading. Fields come from a fixed set:

- Text: `title`, `body`, `thumbnail_text`, `notes` (for the person publishing,
  not part of the published text)
- Lists: `tags`, `hashtags`
- `chapters`: filled in from the `chapters` template's result, not written by
  the model

`max_chars` limits the length of a field as rendered. `max` limits the number
of items in a list. Local models often ignore length instructions, so when a
field breaks a limit, minuta asks the model to repair it, up to `llm.repairs`
times (default 2). If the field still breaks the limit, the template fails.

```markdown
---
description: Short post for our team channel
input: transcript
output: fields
fields: [title, body, hashtags]
limits:
  title: { max_chars: 80 }
  body: { max_chars: 600 }
  hashtags: { max: 3 }
---
Write a short post about "{{job_name}}" from the transcript in the input.
```

### LLM backends

| ID | Backend |
| --- | --- |
| `ollama:<model>` | A model served by your local Ollama, such as `ollama:gemma4:26b` |
| `claude` | Claude Code (`claude -p`) with its default model |
| `claude:<model>` | Claude Code with a given model, such as `claude:sonnet` |

The backend is chosen in this order: the `--llm` flag, then the template's
`llm` key, then `llm.default`, set in the config file or with `MINUTA_LLM`
(default: `ollama:gemma4:26b`). Unlike the flag, `MINUTA_LLM` does not override
a template's `llm` key.

With Claude, minuta runs `claude -p` using your existing Claude Code login. It
runs Claude without tools, sessions or MCP servers, from an empty temporary
directory. Claude sends the transcript to Anthropic, and minuta prints a notice
whenever a template uses it. Ollama keeps it on your machine, unless
`ollama.url` points to another machine.

Long transcripts are split to fit the model's context window and then
combined. For Ollama, the window is the smaller of `ollama.num_ctx` and the
model's own limit.

## The job folder

Each run works in a job folder, `<out>/<name>/`. By default that is
`./minuta-out/<first file's name without its extension>/`.

Three tiers: documents at the top, reusable machine files in `assets/`, and
minuta's bookkeeping in a hidden `.minuta/` folder.

| File | Contents |
| --- | --- |
| `transcript.txt` | Readable transcript with timestamps |
| `captions.srt` | Subtitles |
| `<template>.md` | Each template you asked for |
| `assets/transcript.json` | Timestamped segments and the detected language |
| `assets/<template>.json` | The structured result of a `fields` or `chapters` template |
| `assets/audio.wav` | 16 kHz mono audio given to whisper |
| `assets/joined.<ext>` | The joined recording. Only with several sources; with one, the original file is used as it is |
| `.minuta/job.json`, `.minuta/probe.json` | What each step used and found; minuta reads them to skip finished work |
| `.minuta/llm-cache/` | One file per LLM call |
| `.minuta/derived/` | Output of templates that ran only because another needed them (for example `chapters` under `youtube`) |

For example, after `minuta run standup.mp4 -t linkedin -t youtube -t instagram`:

```console
$ ls minuta-out/standup
assets  captions.srt  instagram.md  linkedin.md  transcript.txt  youtube.md
$ ls minuta-out/standup/assets
audio.wav  instagram.json  linkedin.json  transcript.json  youtube.json
$ ls -A minuta-out/standup/.minuta
derived  job.json  llm-cache  probe.json
$ ls minuta-out/standup/.minuta/derived
chapters.json  chapters.md
```

Job folders made by an earlier version are not migrated: the next run in one
starts fresh and redoes the work once. The old files stay where they are, and
you can delete them.

A job folder belongs to the recordings that created it. If you run different
files with a name that is already taken, minuta stops instead of overwriting.
Choose another name with `--name`, or replace the job with `--force`.

### Caching and `--force`

minuta remembers what each step used: the source files, tool versions, model,
language, and each template's text and backend. When you run a command again,
it skips every step whose inputs have not changed. LLM answers are cached too,
so a failed or interrupted run does not pay for the same call twice.

To redo work anyway:

| Command | Redoes |
| --- | --- |
| `--force` | Everything |
| `--force transcribe` | That step, every later step and every template in the run. Steps, in order: `probe`, `join`, `audio`, `transcribe`, `outputs` |
| `--force summary` | That template and the templates that read it, without using cached answers |

`--force` takes an optional value, so put it after the files:

```sh
minuta run meeting.mp4 -t summary --force summary
```

## Configuration

Every setting can come from the config file or an environment variable, and
some also have a flag. When a setting is given in more than one place, a flag
beats an environment variable, which beats the config file, which beats the
default. A template's `llm` and `lang` keys sit between the flag and the other
layers (see [LLM backends](#llm-backends)).
Run `minuta config show` to see every value and where it came from.

The config file is `~/.minuta/config.toml`. Set `MINUTA_CONFIG` to use
another file. Unknown keys are an error. `minuta config init` writes a
commented template of every setting to that path, so you can see what can be set.

**Upgrading from 0.2.0 or earlier:** the config file and user templates moved
into `~/.minuta/`, next to the models. Files in the old folder are ignored, so
move them once:

```sh
mkdir -p ~/.minuta
mv ~/.config/minuta/config.toml ~/.minuta/config.toml      # if you have one
mv ~/.config/minuta/templates ~/.minuta/templates          # if you have user templates
```

```toml
out_dir = "/Users/you/Documents/minuta"   # `~` is not expanded

[whisper]
model = "small"
lang = "auto"
vad_model = "silero-v5.1.2"

[llm]
default = "ollama:gemma4:26b"
output_lang = "en"

[ollama]
url = "http://127.0.0.1:11434"
num_ctx = 16384
```

| Key | Environment variable | Flag | Default |
| --- | --- | --- | --- |
| `out_dir` | `MINUTA_OUT_DIR` | `--out` | `./minuta-out` |
| `whisper.model` | `MINUTA_WHISPER_MODEL` | `--model` | unset (required) |
| `whisper.lang` | `MINUTA_WHISPER_LANG` | `--lang` | `auto` |
| `whisper.vad_model` | `MINUTA_WHISPER_VAD_MODEL` | | unset (VAD off) |
| `whisper.models_dir` | `MINUTA_MODELS_DIR` | | `~/.minuta/models` |
| `whisper.bin` | `MINUTA_WHISPER_BIN` | | `whisper-cli` |
| `ffmpeg.bin` | `MINUTA_FFMPEG_BIN` | | `ffmpeg` |
| `ffprobe.bin` | `MINUTA_FFPROBE_BIN` | | `ffprobe` |
| `llm.default` | `MINUTA_LLM` | `--llm` | `ollama:gemma4:26b` |
| `llm.output_lang` | `MINUTA_OUTPUT_LANG` | `--output-lang` | the detected speech language |
| `llm.concurrency` | `MINUTA_LLM_CONCURRENCY` | | chosen by the backend |
| `llm.retries` | `MINUTA_LLM_RETRIES` | | `3` (attempts in total) |
| `llm.backoff_ms` | `MINUTA_LLM_BACKOFF_MS` | | `2000` (doubles after each failure) |
| `llm.repairs` | `MINUTA_LLM_REPAIRS` | | `2` (`0` turns repairs off) |
| `ollama.url` | `MINUTA_OLLAMA_URL` | | `http://127.0.0.1:11434` |
| `ollama.num_ctx` | `MINUTA_OLLAMA_NUM_CTX` | | `16384` |
| `claude.bin` | `MINUTA_CLAUDE_BIN` | | `claude` |

The `*.bin` keys take a command name to look up on your `PATH`, or a full path.

## Troubleshooting

**`missing required tools: model`.** No whisper model is configured. Run
`minuta models pull base` and set `whisper.model = "base"` in the config, or
pass `--model base`.

**`ffmpeg`, `ffprobe` or `whisper-cli` is missing.** Install them (see the
[installation guide](https://github.com/JCIOriente/minuta/blob/main/docs/install.md)),
or point `ffmpeg.bin`, `ffprobe.bin` or `whisper.bin` at the binary.

**Ollama is `not reachable` or `model missing`.** Start it with
`ollama serve`, and pull the model, for example `ollama pull gemma4:26b`. If
Ollama runs on another host or port, set `ollama.url`.

**An Ollama reply is cut off.** The prompt does not fit the context window.
Raise `ollama.num_ctx`, if the model supports a larger one.

**The transcript repeats phrases during silences.** Turn on
[voice activity detection](#whisper-models).

**`choose another name with --name or replace it with --force`.** A job with
that name was created from other recordings. See
[The job folder](#the-job-folder).

**A template is not listed or fails to load.** `minuta templates list` lists
invalid template files with the reason at the end.

## Development

```sh
git clone https://github.com/JCIOriente/minuta.git
cd minuta
make ci        # rustfmt, clippy and tests, the same checks CI runs
```

The design is in
[`docs/specs`](https://github.com/JCIOriente/minuta/tree/main/docs/specs), and
the decisions behind it are in
[`docs/adr`](https://github.com/JCIOriente/minuta/tree/main/docs/adr).

## License

minuta is licensed under the
[Apache License, Version 2.0](https://github.com/JCIOriente/minuta/blob/main/LICENSE).
