# Job folder layout — Spec

- Status: draft
- Date: 2026-10-30
- Owner: Arnold Gandarillas Castillo
- Related: ADR-0013 (job folder layout), ADR-0003 (step fingerprint cache), ADR-0005 (call cache), `src/job/mod.rs`, `src/steps/`

## TL;DR
- The top level of a job folder shows only what a user reads: `transcript.txt`, `captions.srt`, `<template>.md` and `<template>.json`.
- `transcript.json`, `audio.wav` and the joined recording move to `data/`. They stay visible, for people who want to reuse them.
- `job.json`, `probe.json` and `llm-cache/` move to a hidden `.minuta/` folder, with the temporary whisper output.
- Job folders from earlier versions are not migrated. The next run in one rebuilds its state and redoes the work once.

## Change in a nutshell
Today:
```console
$ ls minuta-out/standup
audio.wav      chapters.json  job.json    llm-cache   summary.md       transcript.txt  youtube.md
captions.srt   chapters.md    joined.mp4  probe.json  transcript.json  youtube.json
```

After:
```console
$ ls minuta-out/standup
captions.srt  chapters.json  chapters.md  data  summary.md  transcript.txt  youtube.json  youtube.md

$ ls minuta-out/standup/data
audio.wav  joined.mp4  transcript.json

$ ls -A minuta-out/standup/.minuta
job.json  llm-cache  probe.json
```

Three tiers: documents at the top, reusable machine files in `data/`, bookkeeping in `.minuta/`. Only the location of files changes. Names and contents stay the same.

## Problem
A finished job folder lists 13 entries, and 5 of them mean nothing to someone who wanted a summary. `llm-cache/` holds hundreds of hashed `.txt` files and sits next to the deliverables. `audio.wav` and the joined recording are large and rarely wanted, but they hide the documents in a file listing.

## Goals / Success criteria
- After `minuta run <file> -t summary`, a plain `ls` of the job folder shows only `transcript.txt`, `captions.srt`, `summary.md` and `data`. `ls -A` adds `.minuta`.
- `job.json`, `probe.json` and `llm-cache/` are in `.minuta/`. Nothing but that and the files below is written outside it by minuta, except `data/`.
- `transcript.json`, `audio.wav` and `joined.<ext>` are in `data/`.
- Re-running an unchanged job in the new layout skips every step (caching still works), and `--force` redoes everything.
- Leftover `*.tmp` files in the job root, `data/` and `.minuta/` are removed at the start of a run.
- The README job-folder table, the CONTEXT.md terms and the docs that name these paths match the new layout.

## Non-goals
- Moving or reading files of job folders created by an earlier version. No migration, no warning.
- Renaming files, or changing the contents or format of any file.
- A config option or flag to choose the layout.
- Moving `<template>.json` (chapters, fields results) or `captions.srt`. They are deliverables.
- Pruning `llm-cache/` (ADR-0005 still stands).

## Users & permissions
| Role | Can | Cannot |
|---|---|---|
| Local user | Read and delete any file in the job folder | n/a (no shared state) |

## Tenancy & localization
n/a. Single-user CLI. Folder names are fixed English identifiers.

## Domain model
- **Job folder** `<out>/<name>/`, with three tiers:
  - Root: documents (`transcript.txt`, `captions.srt`, `<template>.md`, `<template>.json`).
  - `data/`: reusable machine inputs and results (`transcript.json`, `audio.wav`, `joined.<ext>`).
  - `.minuta/`: minuta's own state (`job.json`, `probe.json`, `llm-cache/`, `whisper-raw.json` while transcribing).
- Invariant: every path inside the job folder comes from one place (a small layout module), so no step builds `ctx.dir.join("...")` by hand.
- `data/` and `.minuta/` are created on first write, as `llm-cache/` is today (`src/llm/call.rs:39`).

## Interfaces
No CLI change. Internal changes:
- A layout module in `src/job/` with the folder names and one function per path (`state_file`, `probe_file`, `call_cache_dir`, `transcript_json`, `audio_file`, `joined_file`, `raw_whisper`). The constants `JOB_FILE` (`src/job/mod.rs:43`), `PROBE_FILE` (`src/steps/probe.rs:14`), `AUDIO_FILE` (`src/steps/audio.rs:11`, `src/steps/transcribe.rs:15`), `TRANSCRIPT_FILE` (`src/steps/transcribe.rs:17`) and `joined_path` (`src/steps/join.rs:83`) are replaced by it.
- `CallCache::new` (`src/llm/call.rs:39`) takes its directory from the layout module.
- `write_atomic` (`src/job/mod.rs:290`) creates the parent folder when it is missing.
- Output paths shown to the user (run summary lines, `--json` output if any) print the new paths.

## Behavior & edge cases
- `Job::open` removes `*.tmp` files in the root, `data/` and `.minuta/` (today only the root, `src/job/mod.rs:238`). `llm-cache/` keeps its own cleanup (`src/llm/call.rs:86`).
- A job folder from an earlier version has `job.json` at the root and no `.minuta/job.json`. `Job::open` sees no state and treats the folder as new: the sources conflict check does not apply, old files are left in place, new ones are written beside them. Stale old files are harmless and the user can delete them.
- The step fingerprint cache compares recorded outputs by path. Paths in `job.json` are relative to the job folder, so they gain the `data/` prefix. A `job.json` written by this version always has the new paths.
- A user who deletes `data/` loses `audio.wav` and the joined recording: the next run sees the missing outputs and rebuilds them, the same as deleting them today.
- `--force` replaces the job: it removes `.minuta/` and `data/` and the documents it owns, as it removes the flat files today.
- The `--name` clash rule (different sources, same name) reads `.minuta/job.json`.

## Data & migrations
No migration (decision recorded in ADR-0013). Cost: one re-transcription and fresh LLM calls per old job that is run again. Old jobs that are only read are unaffected.

## Testing strategy
- Unit: layout module returns the expected paths. `write_atomic` creates a missing parent. `Job::open` cleans `*.tmp` in all three tiers.
- Integration (`tests/cli.rs`): a run with the fake tools produces exactly the root, `data/` and `.minuta/` listings above, and writes nothing else. A second run skips all steps. `--force` redoes all. The existing assertions on `llm-cache` (`tests/cli.rs:1220`) and on the other moved files move to the new paths.
- Integration: a folder with a flat-layout `job.json` and no `.minuta/` is treated as new, and the run succeeds.
- Existing tests that read `job.json` or `transcript.json` directly use the layout functions, not string literals.

## Decisions
- ADR-0013: job folder layout (three tiers, hidden bookkeeping, no migration).
- CONTEXT.md: **Job** and **Call cache** change their paths. New terms: **Data folder** (`data/`) and **State folder** (`.minuta/`).

## FAQ
### Why not put everything but the documents in `.minuta/`?
`transcript.json`, the audio and the joined recording are useful outside minuta (scripting, re-editing, re-transcribing with other tools). A hidden folder makes them hard to find. `data/` keeps them visible but out of the way.

### Why `.minuta/` and not dot-prefixing each file?
One folder is one `ls -A` entry and one thing to delete or exclude from a sync. Dot-prefixed files would still scatter four hidden entries across the root.

### Why not migrate old jobs?
The project is pre-1.0 and a job is cheap to redo. A migration is one-time code with its own tests and failure modes (partial moves, half-written state). The cost of skipping it is one rerun.

## Open questions
- Is `data/` the right name? Alternatives: `media/` (but it holds `transcript.json`), `assets/`. Defaults to `data/` unless you prefer another.

## Implementation status
- [ ] Plan and issues
