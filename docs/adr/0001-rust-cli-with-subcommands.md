# ADR-0001: Rust CLI with subcommands

- Status: accepted
- Date: 2026-10-23
- Deciders: Arnold Gandarillas Castillo
- Related: docs/specs/2026-10-23-core-pipeline.md, docs/brief.md

## Context
The brief sketches one command with short flags that come from template frontmatter
(`minuta v.mp4 -cy`). User templates can declare any short flag, so they can collide with
built-in flags (`-n`, `-f`, `-t`). Changing the CLI grammar after users script it is
breaking.

## Options considered
1. **Subcommands** (`minuta run`, `doctor`, `models`, `config`): no collisions, discoverable
   `--help`, room for new commands. More typing than the sketch.
2. **One command, flags from templates**: terse, but collisions need runtime detection and
   `--help` depends on loaded templates.
3. **Hybrid**: both surfaces to maintain and document.

## Decision
Subcommands. Templates (sub-project #2) are selected with a repeatable `-t <name>`; they
declare no single-letter flags.

## Consequences
The brief's `-cy` shorthand is dropped. Shell aliases can restore it. The `flag` field in the
template frontmatter is not needed and should be removed from the template design in #2.
Revisit if users ask for the shorthand often enough to justify the hybrid.
