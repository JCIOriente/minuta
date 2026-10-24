# ADR-0003: Per-step fingerprint cache in job.json

- Status: accepted
- Date: 2026-10-23
- Deciders: Arnold Gandarillas Castillo
- Related: docs/specs/2026-10-23-core-pipeline.md

## Context
Reruns must reuse finished work (the bash pipeline lost chunk results on one failure) but
must not serve stale output when the source or settings change. The cache format is hard to
change once users have job folders.

## Options considered
1. **Existence only**: skip a step if its output file exists. Simple; stale after any change.
2. **Per-step fingerprint** in `job.json`: hash of source content, step parameters and tool
   version. A mismatch reruns that step and all later ones.

## Decision
Per-step fingerprints, stored in `job.json` with a `version` field. Outputs are written to a
temp file and renamed, so an output exists only if its step finished.

## Consequences
Changing the model, language or ffmpeg version reruns the affected steps, which is correct but
can surprise users; `job.json` records why a step reran. Template steps in #2 reuse the same
mechanism. A `job.json` with an unknown `version` is rejected with a message, not migrated
silently.
