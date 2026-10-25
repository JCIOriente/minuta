# ADR-0007: Fields templates write a versioned JSON artifact plus a rendered `<name>.md`

- Status: accepted
- Date: 2026-10-25
- Deciders: Arnold Gandarillas Castillo
- Related: docs/specs/2026-10-25-fields-and-social-templates.md, ADR-0005, ADR-0006

## Context
The YouTube publisher (sub-project 6) and later tools need a fields template's values
separately, not as Markdown to parse. People need paste-ready text. Chained templates read
`<input>.md` (sub-project 2). Once job folders and consumers exist, the file names and the
JSON layout are hard to change.

## Options considered
1. **`<name>.json` + `<name>.md` for both formats**: chaining is unchanged. A `plain` render
   in a `.md` file is valid Markdown with no syntax in it.
2. **`<name>.json` + `<name>.txt` or `<name>.md` by format**: the extension matches the
   content, but the chaining input file depends on the upstream template's format.
3. **Rendered file only**: no JSON, so consumers parse text or regenerate.

## Decision
Option 1. The JSON layout is
`{"version": 1, "template": "<name>", "output_lang": "<code>", "fields": {…}}`, holding
exactly the declared fields after normalization (hashtags without `#`). Readers reject an
unknown `version`. Both files are template-step artifacts in `job.json`; the step is done
only when both exist.

## Consequences
- Sub-project 6 reads `<name>.json` and never parses Markdown.
- Adding a field to the shared set keeps `version: 1`. Changing a field's type or the
  envelope bumps it.
- Chained templates see only the rendered text. A template that needs the JSON would need a
  new input kind; revisit then.
