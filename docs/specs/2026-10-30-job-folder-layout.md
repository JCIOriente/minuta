# Job folder layout — Spec

- Status: approved
- Date: 2026-10-30
- Owner: Arnold Gandarillas Castillo
- Related: ADR-0013 (job folder layout), ADR-0003 (step fingerprint cache), ADR-0005 (call cache), `src/job/mod.rs`, `src/steps/`

## TL;DR
- The top level of a job folder shows only documents: `transcript.txt`, `captions.srt` and the `.md` of each template the user asked for (`youtube.md`, `linkedin.md`, `instagram.md`, ...).
- Every JSON result (`transcript.json`, `chapters.json`, `youtube.json`, ...), `audio.wav` and the joined recording move to `assets/`. They stay visible, for people who want to reuse them.
- `job.json`, `probe.json` and `llm-cache/` move to a hidden `.minuta/` folder, with the temporary whisper output.
- No file is stored twice. With one source there is no `joined` file at all (the original is used as is). With several, `assets/joined.<ext>` is the only copy.
- Only requested templates produce visible output. `summary` is no longer the input of the social templates, so it runs only on `-t summary`. A template that runs only because another needs it (for example `chapters` under `youtube`) writes its files to `.minuta/derived/`.
- Job folders from earlier versions are not migrated. The next run in one rebuilds its state and redoes the work once.

## Change in a nutshell
Today:
```console
$ ls minuta-out/standup
audio.wav      chapters.json  job.json    llm-cache   summary.md       transcript.txt  youtube.md
captions.srt   chapters.md    joined.mp4  probe.json  transcript.json  youtube.json
```

After `minuta run standup.mp4 -t linkedin -t youtube -t instagram`:
```console
$ ls minuta-out/standup
assets  captions.srt  instagram.md  linkedin.md  transcript.txt  youtube.md

$ ls minuta-out/standup/assets
audio.wav  instagram.json  linkedin.json  transcript.json  youtube.json

$ ls -A minuta-out/standup/.minuta
derived  job.json  llm-cache  probe.json

$ ls minuta-out/standup/.minuta/derived
chapters.json  chapters.md
```
With several sources, `assets/` also holds `joined.mp4`.

Three tiers: documents at the top, reusable machine files in `assets/`, bookkeeping in `.minuta/`. A file is a document if a person reads it; JSON and media are for tools and scripts. Only the location of files changes. Names and contents stay the same.

## Problem
A finished job folder lists 13 entries, and 5 of them mean nothing to someone who wanted a summary. `llm-cache/` holds hundreds of hashed `.txt` files and sits next to the deliverables. `audio.wav` and the joined recording are large and rarely wanted, but they hide the documents in a file listing.

## Goals / Success criteria
- After `minuta run <file> -t linkedin`, a plain `ls` of the job folder shows only `transcript.txt`, `captions.srt`, `linkedin.md` and `assets`. `ls -A` adds `.minuta`.
- With one source, no `joined.*` file exists anywhere in the job folder, and `audio.wav` is extracted from the original file. With several sources, `assets/joined.<ext>` exists and nothing else holds the joined recording.
- `linkedin`, `instagram` and `youtube` read the transcript, not a summary. A run of only these produces no `summary.md` anywhere. `-t summary` still produces a top-level `summary.md`.
- A template that runs only as another's dependency writes its files under `.minuta/derived/`. If the user later asks for it, its files are written to the normal place and the `derived/` copies are removed, so no file exists twice.
- `job.json`, `probe.json` and `llm-cache/` are in `.minuta/`. Nothing but that and the files below is written outside it by minuta, except `assets/`.
- Every `.json` result (`transcript.json`, `<template>.json`), `audio.wav` and `joined.<ext>` are in `assets/`. `.json` files at the root: none.
- Re-running an unchanged job in the new layout skips every step (caching still works), and `--force` redoes everything.
- Leftover `*.tmp` files in the job root, `assets/` and `.minuta/` are removed at the start of a run.
- The README job-folder table, the CONTEXT.md terms and the docs that name these paths match the new layout.

## Non-goals
- Moving or reading files of job folders created by an earlier version. No migration, no warning.
- Renaming files, or changing the contents or format of any file.
- A config option or flag to choose the layout.
- Moving `captions.srt`, which people load into players, or `transcript.txt`.
- Pruning `llm-cache/` (ADR-0005 still stands).
- Removing the `summary` template. It stays built in and runs when asked.
- Changing the join step for several sources.

## Users & permissions
| Role | Can | Cannot |
|---|---|---|
| Local user | Read and delete any file in the job folder | n/a (no shared state) |

## Tenancy & localization
n/a. Single-user CLI. Folder names are fixed English identifiers.

## Domain model
- **Job folder** `<out>/<name>/`, with three tiers:
  - Root: documents (`transcript.txt`, `captions.srt`, and `<template>.md` for each requested template).
  - `assets/`: machine-readable results and media (`transcript.json`, `<template>.json`, `audio.wav`, and `joined.<ext>` when there are several sources).
  - `.minuta/`: minuta's own state (`job.json`, `probe.json`, `llm-cache/`, `derived/`, `whisper-raw.json` while transcribing).
- Invariant: every path inside the job folder comes from one place (a small layout module), so no step builds `ctx.dir.join("...")` by hand.
- `assets/` and `.minuta/` are created on first write, as `llm-cache/` is today (`src/llm/call.rs:39`).

## Interfaces
No CLI change. Internal changes:
- A layout module in `src/job/` with the folder names and one function per path (`state_file`, `probe_file`, `call_cache_dir`, `transcript_json`, `audio_file`, `joined_file`, `template_json`, `raw_whisper`). The constants `JOB_FILE` (`src/job/mod.rs:43`), `PROBE_FILE` (`src/steps/probe.rs:14`), `AUDIO_FILE` (`src/steps/audio.rs:11`, `src/steps/transcribe.rs:15`), `TRANSCRIPT_FILE` (`src/steps/transcribe.rs:17`) and `joined_path` (`src/steps/join.rs:83`) are replaced by it. Template outputs declared in `src/generate.rs:117` (`chapters::JSON_FILE` and the fields `.json`) get their `assets/` path from it too.
- The join step has no single-source mode: `Mode::Link` (`src/steps/join.rs:53`) is removed. With one source the audio step reads the source path, and the join step has no outputs.
- The social built-ins (`src/template/builtin/{linkedin,instagram,youtube}.md`) and the matching catalog files in `templates/` change `input: summary` to `input: transcript`, and their prompt text says transcript where it says summary.
- `CallCache::new` (`src/llm/call.rs:39`) takes its directory from the layout module.
- `write_atomic` (`src/job/mod.rs:290`) creates the parent folder when it is missing.
- Output paths shown to the user (run summary lines, `--json` output if any) print the new paths.

## Behavior & edge cases
- `Job::open` removes `*.tmp` files in the root, `assets/` and `.minuta/` (today only the root, `src/job/mod.rs:238`). `llm-cache/` keeps its own cleanup (`src/llm/call.rs:86`).
- A job folder from an earlier version has `job.json` at the root and no `.minuta/job.json`. `Job::open` sees no state and treats the folder as new: the sources conflict check does not apply, old files are left in place, new ones are written beside them. Stale old files are harmless and the user can delete them.
- The step fingerprint cache compares recorded outputs by path. Paths in `job.json` are relative to the job folder, so they gain the `assets/` prefix. A `job.json` written by this version always has the new paths.
- A user who deletes `assets/` loses `audio.wav` and the joined recording: the next run sees the missing outputs and rebuilds them, the same as deleting them today.
- `--force` replaces the job: it removes `.minuta/` and `assets/` and the documents it owns, as it removes the flat files today.
- The `--name` clash rule (different sources, same name) reads `.minuta/job.json`.

## Implicit dependencies
A template is *requested* if it is named with `-t` (or implied by `--force`'s target). Any other template that runs because a requested one needs it is *implicit*. An implicit template's outputs go to `.minuta/derived/` (same file names, `.json` included). The dependent template reads them from there. When a template is requested and its `derived/` copies exist, the run writes the normal outputs and removes the copies.

## Data & migrations
No migration (decision recorded in ADR-0013). Cost: one re-transcription and fresh LLM calls per old job that is run again. Old jobs that are only read are unaffected.

## Testing strategy
- Unit: layout module returns the expected paths. `write_atomic` creates a missing parent. `Job::open` cleans `*.tmp` in all three tiers.
- Integration (`tests/cli.rs`): a run with the fake tools produces exactly the root, `assets/` and `.minuta/` listings above, and writes nothing else. A second run skips all steps. `--force` redoes all. The existing assertions on `llm-cache` (`tests/cli.rs:1220`) and on the other moved files move to the new paths.
- Integration: a folder with a flat-layout `job.json` and no `.minuta/` is treated as new, and the run succeeds.
- Integration: one source produces no `joined.*`; two sources produce `assets/joined.<ext>` only. `-t youtube` leaves `chapters.*` under `.minuta/derived/` and none at the root or in `assets/`; a later `-t chapters` moves them.
- Integration: `-t linkedin -t youtube -t instagram` produces no `summary.*` anywhere.
- Existing tests that read `job.json` or `transcript.json` directly use the layout functions, not string literals.

## Decisions
- ADR-0013: job folder layout (three tiers, hidden bookkeeping, no migration).
- CONTEXT.md: **Job** and **Call cache** change their paths. New terms: **Assets folder** (`assets/`) and **State folder** (`.minuta/`).

## FAQ
### Why drop the summary as input of the social templates?
A summary is an internal step the user never asked for, and it forced a file the user did not want. The transcript already goes through chunking for long inputs. Risk: posts may draw on less-condensed text for very long recordings; the template tests and a manual check on a long transcript cover it.

### Why not put everything but the documents in `.minuta/`?
The JSON results, the audio and the joined recording are useful outside minuta (scripting, re-editing, re-transcribing with other tools). A hidden folder makes them hard to find. `assets/` keeps them visible but out of the way.

### Why `.minuta/` and not dot-prefixing each file?
One folder is one `ls -A` entry and one thing to delete or exclude from a sync. Dot-prefixed files would still scatter four hidden entries across the root.

### Why not migrate old jobs?
The project is pre-1.0 and a job is cheap to redo. A migration is one-time code with its own tests and failure modes (partial moves, half-written state). The cost of skipping it is one rerun.

## Open questions
None.

## Implementation status
- [ ] Plan and issues
