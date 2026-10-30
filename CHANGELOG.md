# Changelog

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
