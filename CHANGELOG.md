# Changelog

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
