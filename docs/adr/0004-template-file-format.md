# ADR-0004: Template files are Markdown with YAML frontmatter, named by file stem

- Status: accepted
- Date: 2026-10-24
- Deciders: Arnold Gandarillas Castillo
- Related: docs/specs/2026-10-24-templates-and-backends.md, ADR-0001, docs/brief.md

## Context
Users write templates by hand and keep them in `~/.config/minuta/templates/` and
`./templates/`. Once people have templates, changing the file format or how names are
resolved breaks every one of them. Sub-project 3 adds nested settings
(`limits: { body: { max_chars: 3000 } }`), so the header must handle nested maps.

## Options considered
1. **YAML frontmatter between `---` lines**: the format Jekyll, Obsidian and Claude skills
   use, and what the brief shows. Editors highlight it. Needs one crate (`serde_yaml_ng`, the
   maintained fork of the archived `serde_yaml`). YAML has well-known surprises (`no` read as
   a boolean). With typed, `deny_unknown_fields` structs these become errors instead of
   wrong values.
2. **TOML frontmatter between `+++` lines**: no new dependency, since config already uses
   `toml`. Less familiar to template authors, and most editors do not treat it as
   frontmatter.
3. **Separate header file** (`acta.toml` + `acta.md`): no frontmatter parsing, but two files
   per template to keep in sync.

For naming:
1. **File stem is the name**: one source of truth. Overriding a template means creating a
   file with the same name in a higher layer.
2. **`name` field in the frontmatter**: the name can differ from the file, so two files can
   claim one name and `show` cannot locate a template by name alone.

## Decision
YAML frontmatter between `---` lines, parsed into typed structs that reject unknown keys.
The template name is the file stem (`^[a-z0-9][a-z0-9_-]*$`). Discovery layers are built-in,
then `~/.config/minuta/templates/`, then `./templates/`; a later layer wins.

## Consequences
- The brief's `name` and `flag` fields are dropped (`flag` already by ADR-0001).
- A template written for a later sub-project (with `fields:`) fails on an older minuta with
  an "unknown key" error instead of being half-understood.
- An unrelated `./templates/` folder in the current directory is scanned. Only `*.md` files
  with a valid stem count, and a broken file fails only when requested.
- Revisit if YAML parsing problems keep showing up in user reports.
