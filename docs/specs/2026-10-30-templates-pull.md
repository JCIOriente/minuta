# Pull curated templates from the repo — Spec

- Status: approved
- Date: 2026-10-30
- Owner: Arnold Gandarillas Castillo
- Related: ADR-0004 (template file format), `src/template/discover.rs`, `src/cli.rs` (`templates` commands), `src/models.rs` (download code), `src/config_init.rs` (safe write)

## TL;DR
- Curated templates live as plain `.md` files in a top-level `templates/` folder of the repo, with a `templates/index.toml` catalog (TOML, like the config file). They are not compiled into the binary.
- `minuta templates pull` downloads the ones the user picks into `./templates` (or `--to DIR`). Pulled files are ordinary project templates and win over `~/.minuta/templates` and the built-ins.
- Before any network access or write, minuta shows the host, ref, files and destination and asks for confirmation (`--yes` skips it; a non-interactive run without `--yes` refuses).
- Every file is checked against the catalog SHA-256 and parsed as a template before it is written.
- A small manifest `templates/.pulled.toml` records the hash of each file at pull time, so `pull` tells *new*, *up to date*, *update available* and *modified* (edited by you) apart, and never overwrites your edits without `--force`.
- Lookup order does not change: `./templates` already comes first. The 5 built-ins stay as the offline fallback.
- The first curated set is written with Codex at high reasoning effort, one specialist persona per platform, then reviewed (see Curated content).

## Change in a nutshell
```console
$ cd ~/Downloads/vids
$ minuta templates pull --list
templates from github.com/JCIOriente/minuta @ main
NAME       PLATFORM   STATE             DESCRIPTION
instagram  instagram  new               Reel/post caption with a hook in the first 125 characters
youtube    youtube    update available  Title, description, tags and thumbnail text
linkedin   linkedin   modified          Post with a hook and a closing question
summary    -          up to date        ...

$ minuta templates pull instagram youtube
minuta will download from:
  https://raw.githubusercontent.com/JCIOriente/minuta/main/templates/
  instagram.md  (new)
  youtube.md    (update available)
into /Users/you/Downloads/vids/templates
Continue? [y/N] y
instagram.md  written
youtube.md    updated

$ minuta templates list      # instagram now shows layer "project"
```

Selection forms: names (`pull instagram youtube`), `--all`, `--filter linkedin` (matches name, platform or tag), or, on a terminal with nothing selected, a checklist. `--to ~/.minuta/templates` installs them for every folder instead of the current one.

## Problem
The built-in templates are compiled in, so improving or adding one needs a release, and a
contributor must edit Rust. A user can customize one by redirecting `templates show NAME`, but
there is no way to get a larger set of well-made templates, and no place for others to add
theirs. The folder-first lookup the user wants (a templates folder inside the working folder
wins) already exists; what is missing is a way to fill it.

## Goals / Success criteria
- `templates pull --list` prints the catalog without writing anything.
- `templates pull NAME...`, `--all` and `--filter TEXT` select templates; with a terminal and no selection a checklist opens.
- Default destination is `./templates` (created if missing); `--to DIR` overrides it.
- A confirmation prompt names the host, ref, each file with its state (new, update available), and the destination. No bytes are fetched before it is answered, except the catalog needed to build it.
- Without a terminal and without `--yes`, the command exits with a usage error and writes nothing.
- Each template in the destination has one state, computed from three hashes (local file, manifest entry, catalog entry):

  | Local file | vs manifest | vs catalog | State | `pull` does |
  |---|---|---|---|---|
  | missing | - | - | **new** | downloads |
  | present | any | equal | **up to date** | nothing |
  | present | equal | differs | **update available** | downloads, no `--force` needed |
  | present | differs, or no manifest entry | differs | **modified** | skips and says so; with `--force` saves the old file as `NAME.md.bak`, then overwrites |

  The *up to date* row is checked first: a file whose bytes already equal the catalog is up to date whatever the manifest says. Otherwise a file minuta did not write has no manifest entry, so it counts as *modified*. Minuta never overwrites a file it did not write unless `--force`.
- A file whose SHA-256 differs from the catalog, or that fails `Template::parse`, is not written and the command exits non-zero naming the file. Other selected files are not written either (all or nothing).
- Adding a template to the repo needs a new `templates/<name>.md` and one catalog entry. No Rust change.
- A repo test fails if `templates/index.toml` and the files disagree (missing file, wrong hash, file not parsable, name not matching the file stem).
- Offline use is unchanged: nothing else in minuta touches the network for templates.

## Non-goals
- Removing or replacing the 5 built-ins.
- Merging or diffing local edits with upstream changes. The user keeps their copy or takes the new one, and the `.bak` file lets them diff by hand. No `templates remove`; users delete the file.
- Signature verification. The SHA-256 detects corruption and a mismatched catalog, not a compromised repo (see Behavior).
- Mirrors, private repos, auth tokens, or catalogs other than this repo (a `--ref` override is the only knob).
- Walking up parent folders for `./templates`, or a `.minuta/templates` project folder.
- Dependency resolution beyond reporting it (see Behavior).

## Users & permissions
| Role | Can | Cannot |
|---|---|---|
| Local user | Browse the catalog, pull templates into any folder they can write | n/a (single-user CLI, no shared state) |
| Repo contributor | Add a template and catalog entry by pull request | Change what a user's machine downloads without the user confirming |

## Tenancy & localization
n/a for data. Catalog descriptions and prompts are English, like the rest of the CLI. A template
chooses its output language through minuta's existing `output_lang` handling, not through pull.

## Domain model
- **Catalog**: `templates/index.toml` at a ref: `version = 1` and one `[[template]]` table per Entry. Minuta reads it fresh on every `pull`, so templates added to the repo show up without a new release or tag.
- **Entry**: `name` (matches `valid_stem`), `description`, `platform`, `tags[]`, `input` (the template's `input:` chain, informational), `sha256` of the file bytes.
- **Pulled template**: a file `<dest>/<name>.md`. After writing it is an ordinary project-layer template.
- **Manifest**: `<dest>/.pulled.toml`, `version = 1` and one `[[file]]` table per pulled template with `name` and `sha256` (hash of the bytes written). Written after the files, via temp file + rename, and only entries for files written in this run change. A missing, unreadable or invalid manifest is treated as empty with a warning, so everything present reads as *modified* and nothing is overwritten. The template loader reads only `*.md`, so it ignores the manifest.
- Invariants: a written file's bytes hash to the catalog value and parse as a template; entry name equals file stem.

## Curated content
Part of this work, as its own plan task after the pull command exists. Process, per template:
1. A persona brief fixes role, platform rules and limits (for example, Instagram copywriter: hook in the first 125 characters, caption length, hashtag strategy, call to action).
2. `codex exec` at high reasoning effort drafts the template in the ADR-0004 format, with `description`, `limits`, a grounding rule ("only facts in the input") and the `input:` chain. If the `codex` CLI is not installed, Claude subagents draft instead and the PR says so.
3. A second, independent agent reviews each draft, and each template runs through `minuta run -t NAME` on a sample transcript.
4. The persona briefs are committed under `docs/` so contributors can reuse them.
The briefs go to the owner for approval before drafting. Initial set: instagram, youtube, linkedin, tiktok, x-thread, newsletter, blog-post, podcast-notes, meeting-minutes, action-items.

## Interfaces
`minuta templates` has three commands, each with one job:
| Command | What it does | Network |
|---|---|---|
| `templates list` | Templates minuta can use now, with layer (built-in, user, project) | no |
| `templates show NAME` | Prints one template's file | no |
| `templates pull` | Downloads curated templates into `./templates` | yes, after confirmation |

```
minuta templates pull [NAME...] [--all] [--filter TEXT] [--list]
                      [--to DIR] [--force] [--yes] [--ref REF]
```
- Source URL: `https://raw.githubusercontent.com/JCIOriente/minuta/<ref>/templates/<file>`. Default `--ref main` (see FAQ). HTTPS only; connect/read timeouts like `models.rs`; size cap per file (256 KiB) and for the catalog (1 MiB).
- `--list` conflicts with selection flags and `--force`. `NAME...` conflicts with `--all` and `--filter`.
- Stdout carries only the written paths (one per line); the prompt, the table and messages go to stderr, so `$(...)` and pipes stay clean.
- New module `src/template/pull.rs` (catalog parsing, selection, verify, write); `TemplatesCommand::Pull` in `src/cli.rs`.
- Reused: `ureq` agent setup from `src/models.rs:206`, `valid_stem` and `Template::parse` from `src/template/mod.rs`, the `create_new`/`--force`/`Written` pattern from `src/config_init.rs:67`, `sanitize` from `src/cli.rs`.

## Behavior & edge cases
- Order: fetch catalog -> read manifest -> select -> build the plan (state per file) -> prompt -> fetch files -> verify all -> write all. Verification finishes before the first write.
- Unknown NAME: usage error listing available names. `--filter` with no match, or with blank text: usage error ("no template matches filter ...", "filter must not be empty"), nothing fetched beyond the catalog.
- Empty selection from the checklist: exit 0, "nothing selected".
- When every selected file is *up to date* or *modified* (without `--force`): no prompt, a line per file with its state, exit 0.
- `--force` on an *up to date* file does nothing. `--force` on a *modified* file writes `NAME.md.bak` (replacing an older `.bak`) before overwriting; if the backup cannot be written, the file is not overwritten.
- Write is temp file + rename in the destination, so a failure leaves no half file. If the destination exists as a symlink or is not a directory, error naming the path (`try_exists`, not `exists`, per `docs/conventions.md`).
- Catalog or file parse failure from the remote (invalid TOML, `version` != 1, name not `valid_stem`, duplicate name) is a hard error that shows the sanitized reason. Remote strings are passed through `sanitize` before printing.
- A pulled template whose `input:` names a template minuta cannot find is still written; `templates list` shows the existing invalid-state output. Pull prints a note listing the missing name.
- A template using frontmatter an older binary does not know fails `Template::parse` (`deny_unknown_fields`). It is rejected at pull time with the parse error and a hint to upgrade minuta, instead of failing later during a run.
- Trust: the SHA-256 comes from the same repo ref as the file, so it protects against truncation and mismatched files, not against a malicious commit. The confirmation prompt, the all-parse check, and the project-layer protections already in `discover.rs` (symlink and hard-link refusal) are the controls. A user who wants stability passes `--ref v0.3.0` or a commit.
- Network failure: error naming the URL (without query), nothing written.

## Data & migrations
None. No config key. The only new on-disk state is `.pulled.toml` in the destination folder. The `templates/` folder and `index.toml` are new repo content; `index.toml` has `version = 1`. Packaging (`cargo package`/release) must not need these files, since nothing embeds them.

## Testing strategy
- Unit: catalog parsing (valid, bad version, duplicate, bad name, oversized), selection (names, `--all`, `--filter` on name/platform/tag), plan states, hash check, parse check.
- Integration against a local fake HTTP server (pattern: `tests/support/fake_ollama.rs`, `tests/config_init.rs`): `--list`, pull by name, `--all`, `--filter`, each of the four states (including no manifest and invalid manifest), update applied without `--force`, modified file skipped, `--force` writes `.bak` then overwrites, manifest updated only for written files, `--to`, hash mismatch rejects and writes nothing, unparsable file rejects all, non-TTY without `--yes` refuses, stdout contains only paths, stale half file never appears after a mid-download failure.
- The source URL base is injectable (env var `MINUTA_TEMPLATES_URL`, undocumented, used by tests) so no test touches GitHub.
- Repo consistency test: every `templates/index.toml` entry has a file whose hash matches, parses with `Template::parse`, and has `name == stem`; every `templates/*.md` has an entry.
- Permission-denial: destination not writable gives an error naming the path and exit non-zero.
- Manual check in a scratch folder after the content lands (see plan): `pull`, `templates list`, `run -t <pulled>`.

## Decisions
- New ADR `docs/adr/0011-templates-pulled-from-repo.md`: curated templates are repo content fetched on demand, default ref `main`, SHA-256 as integrity check only.
- `CONTEXT.md`: add **catalog** and **pull** if the file defines the other terms; otherwise skip.

## FAQ
### Why download instead of embedding more templates?
Content changes faster than the binary and anyone can contribute a template with a file and a catalog entry. The cost is a network step; the 5 built-ins keep the offline path working.

### Why `pull` and not `install`, `fetch` or `get`?
`minuta models pull` already means "download a file into minuta's folder". One verb for one idea. `install` suggests system-wide state, and `fetch` does not say the files are kept.

### Why default to `main` and not the binary's release tag?
Templates are content, not code. A tag-pinned default means a template added after the release is unreachable until the next release, which defeats the point. Compatibility is protected by the parse check at pull time, and `--ref` pins a tag for people who want stability.

### Why verify a hash from the same repo?
It catches truncated or mismatched downloads and lets the repo test guard the catalog. It does not prove authenticity, and the spec says so. Real signing is out of scope.

### Why all-or-nothing writes?
A half-installed set is hard to reason about. Verifying every file first costs nothing extra since all must be fetched anyway.

## Open questions
None. Approved by the owner with default `--ref main` and the 256 KiB / 1 MiB size caps.

## Implementation status
- [x] ADR-0011 (proposed, PR #159)
- [x] Plan: `docs/plans/2026-10-30-templates-pull.md`
- [x] #160 catalog, manifest and states (PR #167, in review)
- [ ] #161 fetch, plan, verify, write
- [ ] #162 CLI command
- [ ] #163 repo catalog and consistency test
- [ ] #164 persona briefs
- [ ] #166 curated templates
- [ ] #165 docs
