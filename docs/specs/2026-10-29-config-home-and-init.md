# One home folder and `minuta config init` — Spec

- Status: approved
- Date: 2026-10-29
- Owner: Arnold Gandarillas Castillo
- Related: `src/config.rs` (`default_path`, `resolve_path`), `src/cli.rs` (`config` commands), `src/template/discover.rs`, `src/models.rs`, `docs/specs/2026-10-29-models-dir-and-pull-progress.md`

## TL;DR
- Minuta keeps its user files in one folder, `~/.minuta/`: the config file at `~/.minuta/config.toml`, the models in `~/.minuta/models/`, and user templates in `~/.minuta/templates/`.
- `minuta config init` creates the config file with every key listed and commented out, so the user can see what can be set.
- `MINUTA_CONFIG` still overrides the config path. `MINUTA_MODELS_DIR` and `whisper.models_dir` still override the models folder. Nothing else changes.

## Change in a nutshell
```console
$ minuta config init
created /Users/you/.minuta/config.toml

$ minuta config init
/Users/you/.minuta/config.toml already exists; nothing changed (use --force to overwrite)

$ minuta config path
/Users/you/.minuta/config.toml
```

```toml
# ~/.minuta/config.toml (written by `minuta config init`)
[whisper]
# model = "small"
# vad_model = "silero-v5.1.2"
# models_dir = "~/.minuta/models"
...
```

## Problem
Settings live in `~/.config/minuta/`, models in `~/.minuta/models/`, and templates in `~/.config/minuta/templates/`. A user has to know two folders to find their files. Users also get no file to start from, so they do not know which keys exist. This is why `minuta config show` printed only defaults for a user who created an empty config file.

## Goals / Success criteria
- Default config path is `~/.minuta/config.toml`. `minuta config path` prints it.
- Default user templates folder is `~/.minuta/templates/`. Layers stay as they are: built-in, user folder, `./templates/` (later wins).
- `minuta config init` writes the default config path with a commented template of all keys, creates the parent folder, and prints the path it wrote.
- `minuta config init` does not overwrite an existing file. `--force` overwrites it.
- `minuta config show` after `init` shows every value as `(default)` until the user uncomments a line.

## Non-goals
- Moving or copying files from `~/.config/minuta/` or `~/.cache/minuta/`. Nothing is migrated.
- Reading the old config path or the old templates folder as a fallback.
- An interactive wizard. `init` writes a file and exits.
- Changing `MINUTA_CONFIG`, `MINUTA_MODELS_DIR` or their precedence.
- Creating the templates folder on `init`. It is created when the user adds a template.

## Users & permissions
| Role | Can | Cannot |
|---|---|---|
| Local user | Run `config init` and `config path`; put config and templates in `~/.minuta/` | n/a (single-user CLI) |

## Tenancy & localization
n/a. Single-user CLI. The `init` messages are English, like the rest of the CLI output.

## Domain model
- **Home folder**: `~/.minuta/`, the default location for the config file, models and user templates. Not configurable itself; each part can still be overridden by its own setting.
- **Config template**: the text `config init` writes. Every key from `config show` appears once, commented out, with its default value. It is kept in the code next to the key list so the two cannot drift apart (a test checks it).

## Interfaces
- `config::default_path()` returns `~/.minuta/config.toml`.
- `template::discover` user layer becomes `~/.minuta/templates`.
- New `minuta config init [--force]`. Exit 0 when the file is created or already exists and is left alone. Exit 1 on I/O error.
- `config init` ignores a malformed existing file: it only checks that the file exists. It does not read it.

## Behavior & edge cases
- `MINUTA_CONFIG` set: `config init` writes to that path. The existing rule for an explicit missing file (`Error::Config`) applies to reads, not to `init`.
- Parent folder `~/.minuta/` missing: created by `init`. Its permissions follow the umask.
- Existing `~/.config/minuta/config.toml`: ignored. `config path` shows the new path, so the user sees the change.
- Existing `~/.config/minuta/templates/*.md`: ignored. The user must move them. This is a breaking change for users with custom templates; the README and the release notes say so.
- A `--force` overwrite replaces the file without a backup.

## Data & migrations
No data is migrated. The README gets a short note: move `~/.config/minuta/templates/` to `~/.minuta/templates/`, and copy your config file to `~/.minuta/config.toml` if you have one.

## Testing strategy
- Unit: `default_path()` and the template layer use `~/.minuta/` (injected home directory, no real home).
- Unit: the config template contains every key from the key list, each commented out, and parses as valid TOML once uncommented.
- Integration: `config init` creates the file and folder in a temp home; a second run does not change it; `--force` replaces it.
- Integration: `config init` with `MINUTA_CONFIG` writes to that path.
- Integration: `config show` after `init` reports only defaults.
- Regression: `config path` output is the new default when no env is set.

## Decisions
- Config and templates go in `~/.minuta/`, next to the models. Chosen by the user.
- `init` writes a full commented template, not a minimal file. Chosen by the user.
- No migration from the old folders. The old files are left on disk.
- Possible ADR: "Home folder for user files" (to be written with the plan if approved).

## FAQ
### Why not keep `~/.config`?
`~/.config` is for tools that follow the XDG layout. Minuta already puts models in `~/.minuta/`, and users look in one place for their files.

### Why does `init` not read the existing file?
A malformed file must not block `init`, and `config path` already works in that case. `init` only needs to know whether the file exists.

## Open questions
- Confirm the breaking change for templates is acceptable (users with templates under `~/.config/minuta/templates/` must move them).

## Implementation status
Updated by the PRs that implement this spec.
