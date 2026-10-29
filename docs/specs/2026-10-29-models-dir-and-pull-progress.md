# Models directory and download progress — Spec

- Status: approved
- Date: 2026-10-29
- Owner: Arnold Gandarillas Castillo
- Related: ADR-0010 (models directory location), `src/models.rs`, `src/cli.rs` (`models` commands), `src/progress.rs`

## TL;DR
- Models move from `~/.cache/minuta/models` to `~/.minuta/models`, a folder whose name says it holds models for whisper.
- The folder can be overridden with `whisper.models_dir` in config or the `MINUTA_MODELS_DIR` env var.
- Whisper models sit flat in that folder, so whisper.cpp can read them directly. VAD models go in `vad/`.
- `minuta models pull` shows a byte progress bar (percent, rate, ETA) on an interactive terminal, for every download.
- The old cache folder is ignored. Nothing is moved, and large models are downloaded again once.

## Change in a nutshell
```toml
# minuta config (optional)
[whisper]
models_dir = "/Volumes/models/whisper"
```

```console
$ minuta models list
models in /Users/you/.minuta/models
tiny           available  74 MiB
base           installed  141 MiB
...
silero-v5.1.2  available  864 KiB   (vad/)

$ minuta models pull base
pulling base (141 MiB) into /Users/you/.minuta/models
base  [██████████████░░░░░░]  68%  48 MiB/s  eta 2s
base downloaded and verified
/Users/you/.minuta/models/ggml-base.bin
```

```console
$ whisper-cli -m ~/.minuta/models/ggml-base.bin ...   # works: same file, same layout as whisper.cpp
```

## Problem
Users who ran `models pull` earlier have models in a cache folder that is easy to miss, and
the folder name does not say what it holds. `models pull` prints one line and then sits silent
for minutes on a 3 GB model, so a slow connection looks like a hang. Once VAD variants are
added, the flat folder gets mixed.

## Goals / Success criteria
- `models list` and `models pull` use the same directory, shown in the output.
- Default directory is `~/.minuta/models`. Override order: `MINUTA_MODELS_DIR` > `whisper.models_dir` > default.
- Whisper model files are at `<dir>/<file>` (flat), byte-identical to whisper.cpp's layout.
- VAD model files are at `<dir>/vad/<file>`.
- On an interactive stderr without `NO_COLOR`, `pull` draws a progress bar with percent, rate and ETA. Non-interactive runs print one start line and one finish line.
- `--quiet` prints neither the bar nor the start and finish lines.
- Stdout still carries only the resolved model path after `pull`.

## Non-goals
- Reading or moving files from `~/.cache/minuta/models`, and any message about it.
- Resume of a partial download, parallel or ranged download, mirror URLs.
- A CLI flag for the directory. Env and config only for now.
- A directory per model. Only the VAD kind gets a subfolder.

## Users & permissions
| Role | Can | Cannot |
|---|---|---|
| Local user | Choose the directory with config or env; pull and list models | n/a (no shared state) |

## Tenancy & localization
n/a. Single-user CLI. Progress text is English, like the rest of the CLI output.

## Domain model
- **Models directory**: the folder that holds whisper models and the `vad/` subfolder. Resolved once per command.
- **Catalog entry** (`ModelSpec`): name, kind (`Whisper` or `Vad`), file name, URL, SHA-256, size. Gains a subdirectory derived from the kind: `""` for whisper, `"vad"` for VAD.
- Invariant: a file in the directory is installed only if its SHA-256 matches on pull. `list` checks presence and size only, as today.

## Interfaces
- Config key `whisper.models_dir` (string, optional). Env `MINUTA_MODELS_DIR`. Both accept `~` at the start.
- `models::default_dir()` becomes `models::dir(config) -> Result<PathBuf>`: env, then config, then `~/.minuta/models`. The runner and CLI both call it.
- `models::resolve(name_or_path, dir)` joins the catalog entry's subdirectory.
- `models::pull(spec, dir, progress)` takes a progress sink. The CLI passes a bar on a terminal and a no-op otherwise.
- `minuta config` prints the resolved models directory and where it came from (env, config, default).

## Behavior & edge cases
- Directory missing on `list`: nothing is created; every entry is "available".
- Directory missing on `pull`: created, including `vad/` when needed.
- `~` expansion uses the home directory; a `models_dir` with no home directory available is a config error with the key named.
- Relative `models_dir` is resolved against the current directory, same as other paths in config.
- Existing `whisper.model` set to a bare catalog name resolves into the new directory. A file path is used as is.
- A failed or interrupted download removes the `.part` file (unchanged). The bar is cleared before the error prints.
- If the bar cannot be drawn (no terminal), the download still runs and prints the start and finish lines.
- If the old cache folder holds a model, it is not used. A user who wants it back points `models_dir` there.

## Data & migrations
None in the job folder. No file is moved. Existing `~/.cache/minuta/models` is left on disk, and the README notes that it can be deleted.

## Testing strategy
- Unit: precedence of env, config and default, with an injected environment and no real home directory. `~` expansion. Relative and empty values.
- Unit: `resolve` for whisper and VAD names gives `<dir>/<file>` and `<dir>/vad/<file>`.
- Integration: `pull` against a local HTTP server serving a small fake model with a pinned SHA-256 (same approach as `tests/support/`). Checks file location, checksum, `.part` cleanup on a mismatch, and the output lines in plain mode.
- Integration: `pull` with a terminal stub checks the bar is created and cleared; with `--quiet` nothing is printed to stderr.
- Integration: `models list` prints the directory header.
- Regression: `models pull` stdout is exactly the resolved path.

## Decisions
- [ADR-0010: models directory location](../adr/0010-models-directory-location.md)
- New terms in `CONTEXT.md`: "Models directory", "VAD model".

## FAQ
### Why flat for whisper models and a subfolder for VAD?
whisper.cpp's own `models/` folder is flat, so a flat layout lets users point whisper at the same files. VAD models are not whisper models, so they go in their own folder and more variants can sit beside each other.

### Why show the bar for small files?
The VAD model is under 1 MB and the bar disappears in a fraction of a second, but a slow link can make any file slow, and the code path is the same for every entry. Hiding it by size would add a rule to explain.

### Why not keep `~/.cache`?
It is a cache location by convention. Models are user data the user reuses across tools, and the name should say so.

## Open questions
None at approval time.

## Implementation status
Updated by the PRs that implement this spec.
- [x] Plan: [docs/plans/2026-10-29-models-dir-and-pull-progress.md](../plans/2026-10-29-models-dir-and-pull-progress.md)
- [x] T1 config keys: #138
- [x] T2 catalog subfolders: #139
- [ ] T3 CLI and runner: #140
- [ ] T4 pull progress bar: #141
- [ ] T5 docs: #142
