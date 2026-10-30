# Changelog

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
