# Changelog

## 1.0.0

From 1.0, minuta follows a versioning promise; what it covers is in
[docs/stability.md](https://github.com/JCIOriente/minuta/blob/main/docs/stability.md).

### Added
- Four how-to guides, each in English and Spanish, linked from the README: writing your own
  templates, setting up an LLM and whisper, fixing what `minuta doctor` reports, and a worked
  example from a recording to the finished files.
- The templates page lists all 13 curated templates with their platform, what they write and
  how to pull them with `minuta templates pull <name>`.
- The README and LICENSE, the English and Spanish docs and the how-to guides now ship in the
  npm package, so the package page on npmjs.com is no longer empty.
- A project logo and badges in the README.

### Changed
- The install page shows `--llm claude:<model>` for Claude Code, like `codex:<model>` for Codex.
- A release is published with a single approval: npm, crates.io and the GitHub release, with no
  stored npm token (npm trusted publishing).

### Fixed
- Control characters in a transcript's language or a backend name are no longer printed raw in
  the done line; they show as `?`.

## 0.9.0

### Added
- minuta installs from npm: `npm i -g @jcioriente/minuta`, or `npx @jcioriente/minuta` without installing. It needs
  Node.js 22 or newer and no Rust. Prebuilt binaries ship for macOS (Apple silicon and Intel) and
  Linux (x64 and arm64, statically linked), each in its own package that npm picks for your system.

### Fixed
- A step's detail file that cannot be read now produces a warning instead of being skipped silently.

## 0.8.0

### Added
- Three new curated templates: `youtube-shorts` (a timestamped clip with start, end and
  on-screen hook), `instagram-carousel` (cover text, slide copy, caption and hashtags) and
  `lesson` (learning objectives, key concepts and practice questions).
- A missing tool now gets its own install hint: Homebrew on macOS and Linux for `ffmpeg`,
  `ffprobe` and `whisper-cli`, and a link to the installation guide on other systems.

### Changed
- The curated templates are rewritten as publishing kits, with stricter limits on inventing
  facts. They return extra fields a publisher needs: `youtube` a pinned comment and alternative
  titles, `instagram` and `tiktok` cover text and on-screen text, `linkedin` a first comment,
  `newsletter` preview text and an alternative subject, `podcast-notes` chapters and an
  alternative title, and `blog-post` a meta description, slug and image alt text.
- `meeting-minutes` leads with decisions and actions before discussion, `action-items` is a
  flat checklist sorted by owner, and `x-thread` keeps a standalone hook with concise posts.
- Some limits changed, for example `linkedin` allows 3 hashtags instead of 5 and `blog-post`
  5 tags instead of 8. Pull the templates again with `minuta templates pull <name> --force`
  to update.
- The installation guide has one ordered path, with Linuxbrew for whisper.cpp.

### Fixed
- The install hint for `whisper-cli` said `brew install whisper-cpp`; the Homebrew formula is
  `whisper.cpp`.

## 0.7.0

### Added
- Codex as an LLM backend: `--llm codex` or `codex:<model>`, using your existing Codex login.
  The transcript goes to OpenAI, and minuta prints a notice when a template uses it. Point
  `codex.bin` or `MINUTA_CODEX_BIN` at the binary to use a specific one.
- OpenAI-compatible servers such as LM Studio, Colibri or a cloud service. Name one under
  `[endpoints.<name>]` in the config file and use `<endpoint>:<model>` as the backend. An API
  key is read from the variable named in `api_key_env`, never from the config file. minuta
  prints a notice when the URL is not on your machine.
- The docs have a Spanish version, and the installation guide has steps for macOS, Linux and
  Windows through WSL2.

### Changed
- The README is now a short path to a first document. Templates and reference material moved
  to `docs/templates.md` and `docs/reference.md`.
- minuta now needs Rust 1.88 or newer, up from 1.80. All dependencies are on their latest
  versions.
- The HTTP client changed, so some error text differs. A connection timeout to an LLM server
  now covers the TLS handshake too.

### Fixed
- The curated `youtube` template in the catalog now fills `chapters` and `hashtags` like the
  built-in one. Before, a pulled copy replaced the built-in and the video description came
  out with no chapter list. Pull it again with `minuta templates pull youtube --force`.

## 0.6.0

### Changed
- A template that runs only because another needs it (for example `chapters` under `youtube`)
  writes its files to `assets/`, not to `.minuta/derived/`, which no longer exists. Asking for
  it with `-t` moves its `.md` to the top of the job folder.

### Fixed
- `minuta templates pull` no longer hangs on an interactive terminal when it draws the
  progress bar.

## 0.5.0

### Changed (breaking)
- The job folder is reorganized so a plain `ls` shows only documents.
  - `<template>.md`, `transcript.txt` and `captions.srt` stay at the top of `<out>/<name>/`.
  - `transcript.json`, every `<template>.json`, `audio.wav` and `joined.<ext>` moved to `assets/`.
  - `job.json`, `probe.json` and `llm-cache/` moved to the hidden `.minuta/` folder.
  - Scripts that read the old paths must change.
- A single recording no longer gets a `joined.<ext>` copy: the original file is used as it is.
  Several recordings still produce `assets/joined.<ext>`.
- Job folders from earlier versions are not migrated. The next run in one rebuilds it in the
  new layout (transcription and model calls run again); the old files stay and can be deleted.
- The built-in `linkedin`, `instagram` and `youtube` templates read the transcript instead of
  `summary`, so `summary.md` is only produced by `-t summary`. Curated templates in the
  catalog changed the same way; pull them again with `--force` to update.

### Added
- A template that runs only because another needs it (for example `chapters` under `youtube`)
  writes its files to `.minuta/derived/`. Asking for it with `-t` moves them to the normal
  places.
- `minuta templates pull` shows a progress bar (files done of total) on an interactive
  terminal. Output piped or redirected is unchanged and stdout still carries only the paths.
- The built-in `youtube` template fills a `notes` field and hashtags.

### Fixed
- Writing job files no longer follows a planted temporary-file symlink.

## 0.4.0

### Added
- `minuta templates pull` downloads curated templates from the repository into `./templates`
  (or any folder with `--to`). `--list` shows the catalog with the state of each template;
  choose by name, with `--all` or `--filter TEXT`, or pick from a numbered list on a terminal.
  It asks before downloading (`--yes` skips the question) and `--ref` pins a branch, tag or
  commit. Each file is checked against the catalog's SHA-256 and parsed as a template before
  anything is written.
- Templates you pulled are tracked in `.pulled.toml` next to them, so a pull can tell a new
  template, one that is up to date, one with an update available and one you edited
  (*modified*). Edited files are skipped unless `--force` is given, which saves the old file
  as `NAME.md.bak` first.
- Ten curated templates in the repository's `templates/` folder: `instagram`, `youtube`,
  `linkedin`, `tiktok`, `x-thread`, `newsletter`, `blog-post`, `podcast-notes`,
  `meeting-minutes` and `action-items`. The five built-in templates are unchanged and keep
  working offline. A pulled template replaces the built-in or user template of the same name
  (`youtube`, `linkedin` and `instagram` exist in both) for that folder. New templates are added to the repository and appear in `pull --list`
  without a new release.
- README sections "Downloading templates" and "Contributing a template".

## 0.3.0

### Changed (breaking)
- The config file moved from `~/.config/minuta/config.toml` to `~/.minuta/config.toml`.
  The old file is ignored, so move it: `mv ~/.config/minuta/config.toml ~/.minuta/config.toml`.
- User templates moved from `~/.config/minuta/templates/` to `~/.minuta/templates/`.
  Templates in the old folder are ignored, so move them:
  `mv ~/.config/minuta/templates ~/.minuta/templates`.

### Added
- `minuta config init [--force]` writes a commented template of every setting to the config
  file path. An existing file is left alone unless `--force` is given.

## 0.2.0

### Changed (breaking)
- Models now live in `~/.minuta/models/` instead of `~/.cache/minuta/models/`. Models
  already in the old folder are not used, so `minuta models pull` downloads them again;
  the old folder can be deleted.
- VAD models are stored in `~/.minuta/models/vad/`. Whisper models stay flat, as
  whisper.cpp expects them.

### Added
- `whisper.models_dir` config key and `MINUTA_MODELS_DIR` env var to choose the models
  folder. `~` is expanded; other `~user` forms are rejected.
- `minuta models list` prints the folder it reads, and `minuta config show` lists
  `whisper.models_dir` with its source.
- `minuta models pull` shows a progress bar (percent, speed, time left) on a terminal, or
  one start and one finish line otherwise. `--quiet` hides both.

### Fixed
- `minuta models pull --help` lists every catalog model, including the VAD model.
- A catalog name such as `tiny` resolves to the models folder even when a file of that name
  exists in the working directory.

## 0.1.0

First release.
