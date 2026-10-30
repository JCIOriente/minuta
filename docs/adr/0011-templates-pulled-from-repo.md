# ADR-0011: Curated templates are pulled from the repo, not embedded

- Status: proposed
- Date: 2026-10-30
- Deciders: Arnold Gandarillas Castillo
- Related: docs/specs/2026-10-30-templates-pull.md, ADR-0004 (template file format), ADR-0010 (models directory location)

## Context
The 5 built-in templates are compiled in with `include_str!`. Improving one, or adding a new
platform, needs a release and a Rust edit. We want a larger set of well-made templates and a
low-friction way for others to contribute theirs. Templates are prompts, so users also need to
see and own what they run. How they are distributed is hard to reverse: once users rely on a
download source and a catalog format, changing either breaks their scripts.

## Options considered
1. **Fetch from GitHub on demand (`templates pull`)**: content ships independently of the binary and a contribution is a file plus a catalog entry. Cost: a network step, a trust question about remote content, and a catalog format to keep stable.
2. **Bundle `templates/*.md` into the binary at build time and write them out on request**: works offline and always matches the binary version. Cost: still needs a release for every template change, and the set is compiled in, which is what we want to move away from.
3. **Copy from a local checkout path**: simplest and no trust issue. Unusable for anyone who installed through cargo or a package manager.

## Decision
Option 1. Templates are plain files in `templates/` with a `templates/index.toml` catalog
(`version = 1`; TOML like the rest of minuta's config). `minuta templates pull` fetches them over HTTPS from `raw.githubusercontent.com`
at a ref (default `main`, override with `--ref`), after a confirmation that names host, ref,
files and destination. Each file is checked against the catalog SHA-256 and parsed as a
template before anything is written. The SHA-256 is an integrity check only, since it comes
from the same ref as the file. The 5 built-ins stay as the offline fallback.

## Consequences
- Easier: adding or fixing a template needs no release and no Rust; users choose what to download and can delete any file.
- Harder: `index.toml` `version` and the entry fields are now a public contract; a new `Frontmatter` key makes the newest templates unusable on older binaries (rejected at pull time with a hint to upgrade).
- Must do: README explains why templates are downloaded and how to remove them; a repo test keeps catalog and files consistent.
- Revisit if: users need authenticity, not just integrity (then sign the catalog), or the repo moves, or offline-first users ask for bundled copies (then add option 2 on top).
