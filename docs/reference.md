# Reference

**English** | [Español](reference.es.md)

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
| `minuta templates pull [NAME]...` | Download curated templates into `./templates`, after asking ([details](templates.md#downloading-templates)) |
| `minuta config path` | Print the config file path |
| `minuta config show` | Print every setting, its value and where it came from |
| `minuta config init [--force]` | Write a commented template of every setting to the config file path; an existing file is left alone unless `--force` is given |

Run `minuta help <command>` for the full usage of any command. What is safe to
rely on across versions is in [Stability](stability.md).

### Options for `run` and `transcribe`

| Option | Description |
| --- | --- |
| `FILES...` | One or more recordings, joined in the order given |
| `-n, --name NAME` | Job name. Defaults to the first file's name without its media extension |
| `--out DIR` | Where job folders are created. Default: `./minuta-out` |
| `--lang auto\|CODE` | Speech language: `auto` (detect) or a whisper language code such as `en` or `es` |
| `--model PATH\|NAME` | A whisper model file, or a catalog name such as `base` |
| `--force [STEP\|TEMPLATE]` | Redo work instead of reusing the cache (see [Caching](#caching-and---force)) |
| `-q, --quiet` | Hide progress messages. The job folder's path still goes to stdout, and notices, warnings and errors still go to stderr |

`run` also accepts:

| Option | Description |
| --- | --- |
| `-t, --template NAME` | Generate a document with this template. Repeat it for more templates |
| `--llm ID` | Backend for every template in this run: `ollama:<model>`, `claude`, `claude:<model>`, `codex`, `codex:<model>` or `<endpoint>:<model>` |
| `--output-lang CODE` | Language the documents are written in. Default: the detected speech language |

Only the job folder's path goes to stdout. Progress, warnings and errors go to
stderr.

### Exit status

| Code | Meaning |
| --- | --- |
| `0` | Success |
| `1` | A missing tool, a tool error, an LLM or generation error, or a config, job or I/O error |
| `2` | Usage error: bad arguments, an unknown or invalid template, an unknown language, step or backend id, or an unknown model name for `models pull` |
| `130` | `run` or `transcribe` was interrupted (Ctrl-C, `SIGTERM` or `SIGHUP`). The running tool is stopped too |

## Whisper models

| Name | Download size | Notes |
| --- | --- | --- |
| `tiny` | 74 MiB | Fastest, least accurate |
| `base` | 141 MiB | A good default |
| `small` | 465 MiB | |
| `medium` | 1.4 GiB | |
| `large-v3` | 2.9 GiB | Most accurate, slowest |
| `silero-v5.1.2` | 0.8 MiB | Voice activity detection (VAD) model, see below |

Models are saved in `~/.minuta/models/`. Whisper models sit there as plain
`ggml-*.bin` files, the same layout whisper.cpp uses, so whisper.cpp can read them
directly. VAD models go in `~/.minuta/models/vad/`. Change the folder with
`MINUTA_MODELS_DIR` or `whisper.models_dir`; `minuta models list` shows the folder
it reads.

`models pull` shows a progress bar with the percent, speed and time left on a
terminal. Pass `--quiet` to hide it.

Models that an older version saved in `~/.cache/minuta/models/` are not used any
more, so `models pull` downloads them again. You can delete that folder.

`--model`, `whisper.model` and `whisper.vad_model` take a catalog name or the path to any whisper.cpp
`ggml` model file. `minuta models pull` takes catalog names only.

**Voice activity detection** reduces the lines whisper invents during silences.
It is off until you [configure a VAD model](install.md#optional-voice-activity-detection).

## The job folder

Each run works in a job folder, `<out>/<name>/`. By default that is
`./minuta-out/<first file's name without its extension>/`.

Documents sit at the top, reusable machine files in `assets/`, and minuta's
bookkeeping in a hidden `.minuta/` folder.

| File | Contents |
| --- | --- |
| `transcript.txt` | Readable transcript with timestamps |
| `captions.srt` | Subtitles |
| `<template>.md` | The document from each template you asked for |
| `assets/transcript.json` | Timestamped segments and the detected language, as `{"version": 1, "language": ..., "segments": [{"start", "end", "text"}]}` |
| `assets/<template>.json` | The structured result of a `fields` or `chapters` template |
| `assets/audio.wav` | 16 kHz mono audio given to whisper |
| `assets/joined.<ext>` | The joined recording. Only with several sources; with one, the original file is used as it is |
| `assets/<template>.md` | The Markdown of a template that ran only because another needed it (for example `chapters` under `youtube`); it moves to the top if you ask for that template |
| `.minuta/job.json`, `.minuta/probe.json` | What each step used and found; minuta reads them to skip finished work |
| `.minuta/llm-cache/` | One file per LLM call |

For example, after `minuta run standup.mp4 -t linkedin -t youtube -t instagram`:

```console
$ ls minuta-out/standup
assets  captions.srt  instagram.md  linkedin.md  transcript.txt  youtube.md
$ ls minuta-out/standup/assets
audio.wav  chapters.json  chapters.md  instagram.json  linkedin.json  transcript.json  youtube.json
$ ls -A minuta-out/standup/.minuta
job.json  llm-cache  probe.json
```

`transcript.json` carries `"version": 1`, like the other JSON files in `assets/`. A
file without `version`, written by 0.9.0, is read as version 1 and keeps its job's
cache valid. A file with another version is refused with a message naming the
version.

A job folder belongs to the recordings that created it. If you run different files
with a name that is already taken, minuta stops instead of overwriting. Choose
another name with `--name`, or replace the job with `--force`.

Job folders made by an earlier version are not migrated: the next run in one starts
fresh and redoes the work once. The old files stay where they are, and you can
delete them.

### Caching and `--force`

minuta remembers what each step used: the source files, tool versions, model,
language, and each template's text and backend. When you run a command again, it
skips every step whose inputs have not changed. LLM answers are cached too, so a run
after a failure or interruption reuses every answer that already came back.

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

The settings in the table below can come from the config file or an environment
variable, and some also have a flag. A flag beats an environment variable, which beats the config
file, which beats the default. A template's `llm` and `lang` keys sit between the
flag and the other layers (see [LLM backends](templates.md#llm-backends)). Run
`minuta config show` to see every value and where it came from.

The config file is `~/.minuta/config.toml`. Set `MINUTA_CONFIG` to use another
file. Unknown keys are an error. `minuta config init` writes a commented template
of every setting, so you can see what can be set.

An example:

```toml
out_dir = "minuta-out"

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

`~` is not expanded in `out_dir`. Use a relative path or a full one.

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
| `codex.bin` | `MINUTA_CODEX_BIN` | | `codex` |

The `*.bin` keys take a command name to look up on your `PATH`, or a full path.

`[endpoints.<name>]` tables are read from the file only. See
[OpenAI-compatible servers](templates.md#openai-compatible-servers) for their keys.

### Upgrading from 0.2.0 or earlier

The config file and user templates moved into `~/.minuta/`, next to the models.
Files in the old folder are ignored, so move them once:

```sh
mkdir -p ~/.minuta
mv ~/.config/minuta/config.toml ~/.minuta/config.toml      # if you have one
mv ~/.config/minuta/templates ~/.minuta/templates          # if you have user templates
```

## Troubleshooting

**`missing required tools: model`.** No whisper model is configured. Run
`minuta models pull base` and set `whisper.model = "base"` in the config, or pass
`--model base`.

**`ffmpeg`, `ffprobe` or `whisper-cli` is missing.** Install them (see the
[installation guide](install.md)), or point `ffmpeg.bin`, `ffprobe.bin` or
`whisper.bin` at the binary.

**Ollama is `not reachable` or `model missing`.** Start it with `ollama serve`, and
pull the model, for example `ollama pull gemma4:26b`. If Ollama runs on another
host or port, set `ollama.url`.

**An Ollama reply is cut off.** The prompt does not fit the context window. Raise
`ollama.num_ctx`, if the model supports a larger one.

**`unknown endpoint`.** The part before the `:` in the backend id is not a table in
the config file. Add `[endpoints.<name>]` with its `url`, or check the spelling.

**An endpoint says the model is `not offered by the server`.** The id must match the
server's model id exactly. The message lists what the server offers.

**An endpoint answers `exceeds the available context size`, or a reply is cut off.**
`context_tokens` is larger than the context the server gave the model. Load the
model with a larger context, or lower `context_tokens`.

**A request to an endpoint times out.** Slow models need more than the default 10
minutes. Raise `timeout_secs`.

**The transcript repeats phrases during silences.** Turn on
[voice activity detection](install.md#optional-voice-activity-detection).

**`choose another name with --name or replace it with --force`.** A job with that
name was created from other recordings. See [The job folder](#the-job-folder).

**A template is not listed or fails to load.** `minuta templates list` lists invalid
template files with the reason at the end.
