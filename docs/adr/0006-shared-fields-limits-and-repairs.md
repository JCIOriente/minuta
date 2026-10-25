# ADR-0006: Shared closed field set, rendered-length limits, repair calls, accepted-only caching

- Status: proposed
- Date: 2026-10-25
- Deciders: Arnold Gandarillas Castillo
- Related: docs/specs/2026-10-25-fields-and-social-templates.md, ADR-0004, ADR-0005

## Context
`output: fields` templates ask the model for structured values with platform limits. Field
names become part of every user template and of the YouTube publisher (sub-project 6), so
removing or retyping one later breaks both. Local models often ignore length instructions,
and the call cache (ADR-0005) replays answers deterministically, so how rejected answers are
cached decides whether a rerun can ever succeed.

## Options considered
For the field set:
1. **Closed shared set** (`title`, `body`, `tags`, `hashtags`, `thumbnail_text`; `chapters`
   reserved): known kinds and descriptions; the publisher can rely on names. Less flexible.
2. **Open user-defined fields with a declared type**: flexible, but no shared meaning, and
   rendering and publishing cannot rely on any name.

For limits on lists:
1. **`max_chars` measured on the rendered field** (`a, b, c`, `#a #b`): one rule bounds a
   whole caption by adding up fields.
2. **Per-item length plus a separate total-caption limit**: more limit kinds to explain.

For limit violations:
1. **Repair calls**: send the previous answer and the violations back, bounded by
   `llm.repairs`, then fail.
2. **Cut the text to fit**: always passes, but cuts mid-sentence.
3. **Fail at once**: simple, but local models would fail often.

For caching:
1. **Cache only accepted answers**: a failed run gets a fresh final answer next time and
   keeps map notes.
2. **Cache every answer**: fully resumable, but a failed template fails the same way on every
   rerun with zero calls until `--force`.

## Decision
A closed shared field set; `max_chars` measures the rendered field and `max` counts list
items; violations (including invalid JSON) trigger edit-only repair calls up to `llm.repairs`
(default 2); JSON-mode calls are cached only once accepted, and their cache key adds the
schema.

## Consequences
- Adding a field is additive. Removing or retyping one needs a new ADR and a new JSON
  artifact version (ADR-0007).
- A run that fails on limits repeats the final call (and its repairs) on the next run. That is
  the intended trade-off.
- Interrupted repairs are redone.
- Revisit if users keep asking for fields outside the set, or if a backend offers reliable
  length control.
