# Publishing kits: stronger curated templates — Implementation Plan

- Spec: docs/specs/2026-10-30-templates-strategy.md (ADR-0012)
- Date: 2026-10-30
- Status: approved

## Overview
Two tasks. T1 adds the `notes` field, because a curated file that declares it fails
`tests/templates_catalog.rs` until the parser accepts it. T2 rewrites the briefs and templates and
produces the evidence. No new dependency.

## Contract
`notes` in `fields` and `limits` (T1). T2 builds against it.

## Impact
- Affected call paths: `Field` in `src/fields/mod.rs` and every exhaustive match on it. Prompt
  building and the Ollama schema use the field kind, so `notes` needs no special case there.
- Consumers: the future publisher must ignore `notes` (ADR-0012).
- Data: `<name>.json` gains an optional key. `FIELDS_VERSION` stays 1.
- Must stay green: `tests/templates.rs`, `tests/integration/fields.rs`,
  `tests/templates_catalog.rs`, `make ci`.
- Rollout: T2 merges with the release that contains T1, because 0.4.0 refuses files that declare
  `notes` and `templates pull` reads `main`. Rollback: revert T2 first, then T1.

## Dependency graph
```
T1 ──> T2
```

## Tasks

### T1: `notes` field and built-in `youtube` hashtags  (#181)
- Priority: 0
- Depends on: none
- Owns: `src/fields/**`, `src/template/builtin/youtube.md`, `tests/integration/fields.rs`,
  `tests/templates.rs`
- Touches (read-only or additive): none
- Acceptance:
  - [ ] `Field::from_name("notes")` is `Field::Notes`, kind text, not filled, with a description
        that says it is for the publisher and not published
  - [ ] a fields template declaring `notes` with `max_chars` parses, and a limit over it causes a
        repair call
  - [ ] empty `notes` is a violation naming the field
  - [ ] `notes` renders in declared order in `.md` and under `fields.notes` in `.json`
  - [ ] fingerprint of a fields template without `notes` equals its fixed value from before this
        change, and `FIELDS_VERSION` is 1
  - [ ] built-in `youtube` declares `[title, body, chapters, hashtags, tags, thumbnail_text]`, with
        a `hashtags` limit of max 5 and 80 chars, and its render has chapters then hashtags
  - [ ] `baton verify` passes
- Tests first: `field_notes_name_kind_and_description`, `empty_notes_is_a_violation`,
  `notes_over_limit_gets_a_repair_call`, `notes_render_in_declared_order`,
  `fingerprint_without_notes_is_unchanged`, `builtin_youtube_declares_hashtags_after_chapters`
- Notes: follow `Field::ThumbnailText`. The model never gets a special instruction for `notes` from
  minuta; templates say what goes in it.

### T2: Curated templates, briefs and evidence  (#182)
- Priority: 1
- Depends on: T1
- Owns: `templates/**`, `docs/template-briefs/**`
- Touches (read-only or additive): `tests/templates_catalog.rs` only if a new file needs it,
  `README.md` where it counts or lists templates
- Acceptance:
  - [ ] the 10 curated templates and `youtube-shorts`, `instagram-carousel`, `lesson` declare the
        fields and limits in the spec's Domain model table
  - [ ] `templates/index.toml` lists 13 entries with correct hashes
  - [ ] each template has a brief in sync with it, and each platform number has a source and date
        or is marked a minuta choice
  - [ ] `docs/template-briefs/README.md` lists `notes` and no longer says `chapters` is reserved
  - [ ] the templates were produced in this order: Codex drafts, an independent Opus review at
        medium effort, Codex applies and reviews a final pass
  - [ ] each changed or new template ran on `Webinar 1` and `Clase 3.1` with `ollama:gemma4:26b`
        and with `claude:haiku`; the `claude:haiku` runs are executed by a Haiku subagent
  - [ ] an independent reviewer read the outputs against the briefs, and the PR records what ran
        and what was found, including any run that failed
  - [ ] no output invents a fact, computes a number, names a framework the input lacks, ends
        mid-sentence or puts a placeholder in `body`
  - [ ] `baton verify` passes
- Tests first: `tests/templates_catalog.rs` fails until `index.toml` lists the new files with their
  hashes
- Notes: sources are `docs/template-briefs/platform-research-2026-10.md` and the spec, which wins
  where they differ. The spec's sample output shows layout only. The owner's earlier prompts are the
  benchmark for `youtube` and `lesson`; copy them into the brief, since they live outside the repo.
  Run from a folder whose `./templates` holds these files, in a copy of the job folders, never over
  the owner's `~/Downloads/orientate-clases/templates` or `minuta-out`.
