# ADR-0010: Models directory location

- Status: proposed
- Date: 2026-10-29
- Deciders: Arnold Gandarillas Castillo
- Related: docs/specs/2026-10-29-models-dir-and-pull-progress.md, ADR-0002 (whisper CLI as subprocess)

## Context
`minuta models pull` stores whisper and VAD models under `~/.cache/minuta/models`. Models are
large (up to ~3 GB), reused across runs and tools, and users often already keep whisper.cpp
models in a folder of their own. A cache location suggests the files can be deleted at will,
and the folder name does not say these files are whisper models. Moving the location later
means a migration or a second download for every user, so the choice is hard to reverse.

## Options considered
1. **`~/.minuta/models`, overridable by env and config**: a dedicated folder whose name says what it holds. Override for users with an existing folder. Cost: one more dotfolder in the home directory.
2. **Reuse an existing whisper folder** (the directory of `whisper.model` in config, or a whisper.cpp `models/` folder found on disk): no new folder, but the target depends on machine state, so the same config can resolve differently on two machines, and detection needs heuristics we would have to maintain.
3. **Keep `~/.cache/minuta/models`**: no change and no migration, but keeps the cache-location and naming problems.

## Decision
Option 1. Default `~/.minuta/models`. Override order: `MINUTA_MODELS_DIR`, then `whisper.models_dir`, then the default. Whisper models sit flat in the directory, which matches whisper.cpp's layout. VAD models go in a `vad/` subfolder. Files in the old cache folder are not read or moved.

## Consequences
- Easier: one place to look, explained by the name; whisper.cpp can use the same files; room for more model kinds.
- Harder: users with models in `~/.cache/minuta/models` download them again (large-v3 is ~3 GB). The README must say where models live and that the old folder can be deleted.
- Must do: the README and `minuta config` output show the directory; the runner and CLI use one resolver.
- Revisit if: many users ask for automatic reuse of an existing whisper.cpp folder, or if the default needs to differ per platform (for example, on Windows).
