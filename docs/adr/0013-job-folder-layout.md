# ADR-0013: Job folder layout

- Status: accepted
- Date: 2026-10-30
- Deciders: Arnold Gandarillas Castillo
- Related: docs/specs/2026-10-30-job-folder-layout.md, ADR-0003 (step fingerprint cache), ADR-0005 (template steps and call cache)

## Context
A job folder holds documents users read next to bookkeeping they never need: `job.json`, `probe.json` and `llm-cache/` with hundreds of hashed files. Large reusable files (`audio.wav`, the joined recording, the `.json` results) sit in the same flat listing. Paths are recorded in `job.json`, so moving files later means migrating every existing job. The choice is hard to reverse.

## Options considered
1. **Three tiers: documents at the root, `assets/` for reusable machine files, hidden `.minuta/` for bookkeeping**: a clean default listing, and nothing useful is hidden. Cost: two extra folders.
2. **Everything but documents in a hidden `.minuta/`**: simplest root, but the audio, joined recording and JSON results become hard to find for users who reuse them.
3. **Dot-prefix each internal file in place** (`.job.json`, `.llm-cache/`): no new folder, but the root still has several hidden entries and no single place to exclude or delete.

## Decision
Option 1. Root: `transcript.txt`, `captions.srt`, `<template>.md`. `assets/`: every JSON result (`transcript.json`, `<template>.json`), `audio.wav`, `joined.<ext>`. `.minuta/`: `job.json`, `probe.json`, `llm-cache/` and temporary whisper output. With one source there is no joined file (never a duplicate of the original). Templates that run only as another's dependency write under `.minuta/derived/`. All paths come from one layout module. Jobs made by earlier versions are not migrated; a run in one starts fresh.

## Consequences
- Easier: a readable default listing; one folder to ignore, sync-exclude or delete for bookkeeping; one place that defines every path.
- Harder: tools or scripts that read `job.json`, `llm-cache/` or any `.json` result at the old paths break. Old jobs are redone once when run again.
- Must do: README, CONTEXT.md and the specs' current-state docs show the new paths; tests use the layout module.
- Revisit if: users ask for the old flat layout, or a third kind of file does not fit any tier.
