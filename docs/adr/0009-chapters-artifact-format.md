# ADR-0009: `chapters.json` holds whole-second starts; `chapters.md` holds YouTube chapter lines

- Status: proposed
- Date: 2026-10-25
- Deciders: Arnold Gandarillas Castillo
- Related: docs/specs/2026-10-25-chapters.md, ADR-0007, ADR-0008

## Context
`chapters.json` is read by fields templates that declare `chapters`, later by the edit plan
(sub-project 5, which remaps timestamps) and by the YouTube publisher (sub-project 6). Templates
with `input: chapters` read `chapters.md`, as chaining always reads `<name>.md`. Once job folders
and readers exist, the layout and time unit are hard to change.

## Options considered
Time unit:
1. **Whole seconds (u64), floored**: what YouTube reads from the description, so the chapter
   rules are checked on the values that get published.
2. **Fractional seconds from the segments**: more precise for remapping, but every consumer
   rounds again and the 10 s rule could pass on fractions and fail on YouTube.
3. **Rendered strings (`"4:12"`)**: no parsing for the publisher, but every other consumer parses
   them.

Rendered file:
1. **YouTube lines (`0:00 Title`)**: paste-ready and readable as template input.
2. **Transcript-style `[00:04:12] Title`**: matches the transcript input, but is not what
   YouTube shows.

## Decision
`{"version": 1, "output_lang": "<code>", "chapters": [{"start": <u64>, "title": "<string>"}]}`,
pretty-printed, readers reject an unknown `version`. `chapters.md` is one `<stamp> <title>` line
per chapter, with `m:ss` below one hour and `h:mm:ss` from one hour. In a fields artifact the
`chapters` field holds the same list of objects, and the fields artifact stays `version: 1`
because adding a field is additive (ADR-0007).

## Consequences
- Sub-project 5 remaps whole seconds. Sub-second precision is lost, which YouTube does not use.
- Changing the time unit or the envelope bumps `version`. Adding keys to a chapter (for example
  an end time) is additive.
- Templates that read `chapters.md` see YouTube-style stamps, not the transcript's.
