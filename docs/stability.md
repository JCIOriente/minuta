# Stability

**English** | [Español](stability.es.md)

From 1.0.0, minuta follows [semantic versioning](https://semver.org): a change that
breaks something listed as Stable below only ships in a major version (2.0.0).
Versions before 1.0.0 make no such promise; the [changelog](../CHANGELOG.md) shows
what broke then.

Every part of minuta you can depend on is in one of three classes:

| Class | What it means |
| --- | --- |
| Stable | Changes only in a major version. New things may be added in a minor version |
| Provisional | May change in a minor version. The change is listed in the changelog under `Changed` |
| Not covered | May change in any version, a patch included, without a note |

Anything not listed here is Not covered.

## Command line

| Surface | Class |
| --- | --- |
| Subcommands and their names: `run`, `transcribe`, `doctor`, `models list\|pull`, `templates list\|show\|pull`, `config path\|show\|init` | Stable |
| Flags and their meaning, as listed in the [reference](reference.md#options-for-run-and-transcribe) and by `minuta help` | Stable |
| Exit codes `0`, `1`, `2` and `130` and [what they mean](reference.md#exit-status) | Stable |
| `--llm` ids: `ollama:<model>`, `claude`, `claude:<model>`, `codex`, `codex:<model>`, `<endpoint>:<model>` | Stable |
| Whisper model names in the catalog (`tiny`, `base`, `small`, `medium`, `large-v3`, `silero-v5.1.2`) | Provisional |
| The `MINUTA_*` variables in the [configuration table](reference.md#configuration) and `MINUTA_CONFIG` | Stable |
| `MINUTA_TEMPLATES_URL` (not in the docs; it exists for testing) | Not covered |

Adding a subcommand, a flag or a backend is not a breaking change. For `run` and `transcribe`,
printing only the job folder's path on stdout, and everything else on stderr, is Stable.

## Configuration

| Surface | Class |
| --- | --- |
| The config file path and the keys in the [configuration table](reference.md#configuration), with the meaning of each | Stable |
| The `[endpoints.<name>]` keys: `url`, `context_tokens`, `timeout_secs`, `api_key_env` | Stable |
| Default values | Provisional |
| The order that settings are resolved in (flag, environment, config file, default) | Stable |

A new key may appear in a minor version. Unknown keys are an error, so a config
file that uses a new key does not load on an older minuta.

## Templates

| Surface | Class |
| --- | --- |
| Header keys (`description`, `input`, `output`, `fields`, `limits`, `format`, `llm`, `lang`), their values and defaults | Stable |
| Variables (`{{job_name}}`, `{{duration}}`, `{{output_lang}}`, `{{output_lang_code}}`, `{{speech_lang}}`) | Stable |
| Output kinds `markdown` and `fields`, and the field names `title`, `body`, `thumbnail_text`, `notes`, `tags`, `hashtags`, `chapters` | Stable |
| The three places templates are loaded from and which wins | Stable |
| The catalog `templates/index.toml`, its `version = 1` and its keys | Stable |
| `.pulled.toml` | Provisional |
| The text of built-in and curated templates, and the names and limits of curated ones | Not covered |

New header keys, variables and fields may come in a minor version. The wording of
a template changes whenever its output improves; pull it again to get the change.

## Files on disk

| Surface | Class |
| --- | --- |
| The names at the top of the [job folder](reference.md#the-job-folder): `transcript.txt`, `captions.srt`, `<template>.md` | Stable |
| The files in `assets/`: `transcript.json`, `<template>.json`, `audio.wav`, `joined.<ext>` | Stable |
| The content of `transcript.json`: `language` and `segments`, each with `start`, `end` and `text` | Stable |
| The content of `assets/<template>.json`: `version`, `template`, `output_lang` and `fields` (`chapters.json` has `version`, `output_lang` and `chapters`) | Stable |
| The layout of the readable files `transcript.txt` and `captions.srt` | Provisional |
| `.minuta/` and everything in it | Not covered |

Programs that read the JSON files must ignore keys they do not know: a minor
version may add keys. Changing or removing a key raises the file's `version` and
only happens in a major version. A job folder made by an earlier version is not
migrated: the next run in it starts over.

## What minuta prints

| Surface | Class |
| --- | --- |
| Progress lines, warnings and notices | Not covered |
| Error wording | Not covered |
| The output of `doctor`, `config show`, `models list` and `templates list` | Provisional |

Scripts should rely on exit codes and on the job folder's path, not on text.

## Rust crate

minuta is a program. The crate has a library target, but only so that the binary
and its tests can share code; its Rust API (`cli`, `error` and a hidden `testing`
module) is Not covered, and it does not follow semantic versioning. Do not depend
on it as a library.

## Platforms and tools

| Surface | Class |
| --- | --- |
| macOS (Apple silicon and Intel) and Linux (x64 and arm64) | Stable |
| Windows through WSL2 only | Provisional |
| Node.js 22 or newer for the npm package | Provisional |
| Minimum supported Rust version (1.88 today), for building from source | Provisional |
| ffmpeg, ffprobe, whisper.cpp, Ollama, Claude Code and Codex | Not covered |

Dropping a Stable platform needs a major version. A new Node.js or Rust floor may
come in a minor version, with a changelog note. minuta checks no minimum version of
the external tools, so a new release of one of them can change a result or break a
run; `minuta doctor` shows the versions in use.

## Deprecations

When something Stable is going to change or go away, a minor version first marks it
deprecated: the changelog says so under `Deprecated`, and the docs say what to use
instead. It keeps working for at least one more minor version. Removal happens in
the next major version.

## Where breaking changes are announced

Every breaking change is listed in [`CHANGELOG.md`](../CHANGELOG.md) under
`Changed (breaking)`, with the steps to move over. The same text is the body of
the version's GitHub release.
