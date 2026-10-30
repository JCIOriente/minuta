# ADR-0012: `notes` joins the shared field set

- Status: proposed
- Date: 2026-10-30
- Deciders: Arnold Gandarillas Castillo
- Related: docs/specs/2026-10-30-templates-strategy.md, ADR-0006, ADR-0007

## Context
Publishing needs text that is not the post: a pinned comment, alternative titles, alt text, preview
text, a meta description. `body` is what the YouTube publisher will send, so these cannot go there.
ADR-0006 keeps the field set closed and says adding a field is additive.

## Options considered
1. **One `notes` text field**: one call, one output, one idea for every template. Its content is not
   machine-readable per piece.
2. **A chained `-extras` template per platform**: no Rust and works on 0.4.0, but one more LLM call
   and output file per platform, and the catalog doubles.
3. **A named field per piece** (`pinned_comment`, `preview`, ...): machine-readable, but grows the
   closed set for text that nothing consumes yet.

## Decision
Add `notes`, a text field the model answers: short labeled lines for the person publishing, never
part of the published text. `FIELDS_VERSION` and the artifact version stay 1.

## Consequences
- A binary older than this change rejects templates that declare `notes`, and `templates pull` fails
  for that selection. Templates that use it merge with the release that adds it.
- `notes` follows the non-empty rule, so a template declares it only when it can always fill it.
- The publisher must ignore `notes`.
- Revisit if a publisher needs one of these pieces as data; that piece then becomes its own field.
