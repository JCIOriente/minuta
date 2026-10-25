# ADR-0008: Chapters are a built-in template with output kind `chapters`, used through a filled field

- Status: proposed
- Date: 2026-10-25
- Deciders: Arnold Gandarillas Castillo
- Related: docs/specs/2026-10-25-chapters.md, ADR-0004, ADR-0005, ADR-0006

## Context
Chapters are computed once per job from the transcript and used by other templates, first by
`youtube`, whose description must carry them. YouTube ignores the whole chapter list when one
rule breaks (first at 0:00, at least 3, each at least 10 s, ascending), and local models round,
invent and reorder timestamps. Where chapters live decides the `job.json` layout, the names
users can override, and how templates depend on them, which is hard to change once job folders
and user templates exist. The name `chapters` has been reserved since sub-project 2, and the
field `chapters` since ADR-0006.

## Options considered
Where chapters are computed:
1. **Built-in template `chapters` with a new output kind**: reuses the template graph,
   fingerprints, the call cache, `--force`, backend and language precedence, listing and
   overriding by file. The output kind is valid only for that name, which is a special case in
   frontmatter validation.
2. **Fixed stage in the template phase**: compiled prompt, config keys, its own `job.json`
   key. Not overridable, and it duplicates force, progress and listing code.
3. **Pipeline step after `outputs`**: every `run` would need a backend, and the closed
   `StepName` enum would have to change (ADR-0005).

How a fields template gets chapters:
1. **Filled field**: declaring `chapters` makes minuta copy the list from `chapters.json` and
   adds a graph edge to `chapters`. The model never writes it.
2. **`{{chapters}}` variable**: the model copies the lines into `body`, which it does
   unreliably, and the body would have to be checked again.
3. **Appended by the publisher**: `youtube.md` would not be paste-ready.

How broken rules are handled:
1. **Snap and move the first start in code, repair the rest**: the two fixes have one right
   answer. Merging, dropping or reordering is left to the model, in ADR-0006 repair calls.
2. **Fix everything in code**: one call, but code would pick titles and drop chapters.
3. **Fail at once**: local models would fail often.

## Decision
Option 1 in each group. The template named `chapters` must have `output: chapters` and
`input: transcript`, and no other template may have that output. `chapters` is no longer a
reserved name. Template dependencies become a list: the `input` template, then `chapters` when
a fields template declares the `chapters` field. The `chapters` call uses `call_json` with the
repair loop and accepted-only caching of ADR-0006.

## Consequences
- `-t youtube` now also runs `chapters`, so it needs a transcript of at least 30 s, and it
  costs one more LLM call (more when chunked or repaired).
- For long recordings the transcript is chunked twice, by `summary` and by `chapters`.
- A user can tune the chapter prompt and limits by overriding `chapters.md`, but cannot add a
  second chapter list. Revisit if users want several (for example per platform).
- The graph now supports several dependencies, which a future input kind can reuse.
- A field filled by minuta is a new concept in the shared field set. Further filled fields
  need their own ADR.
