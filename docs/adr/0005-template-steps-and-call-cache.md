# ADR-0005: Template steps with content-addressed fingerprints and a per-job call cache

- Status: accepted
- Date: 2026-10-24
- Deciders: Arnold Gandarillas Castillo
- Related: docs/specs/2026-10-24-templates-and-backends.md, ADR-0003

## Context
ADR-0003 caches pipeline steps by fingerprint in `job.json`, keyed by the closed, ordered
`StepName` enum. Templates are open-ended (user-named), form a DAG, and produce
nondeterministic LLM output. The bash pipeline lost every chunk result when one LLM call
failed. Both the `job.json` layout and the cache semantics are hard to change once users
have job folders.

## Options considered
1. **Separate `templates` map, fingerprint from input content**: pipeline code stays as it
   is. A template's fingerprint includes the hash of its input artifact's bytes, so it is
   computed just before the template runs. Dependents rerun exactly when their input text
   changed.
2. **One open step graph**: make step names open strings and run pipeline steps and templates
   as one DAG. A single model, but it rewrites stale planning, `--force` and progress, all of
   them shipped and unrelated to templates.
3. **Templates in the up-front fingerprint chain**: simplest. But an upstream template
   regenerated with the same fingerprint and different text leaves its dependents cached on
   stale input.

For the call cache:
1. **Per job, `<job>/llm-cache/`**: deleted with the job and as sensitive as the transcript.
   Cannot be shared across jobs, which is rarely useful because inputs differ per recording.
2. **Global, `~/.cache/minuta/llm/`**: shared, but it accumulates every recording's content
   in one place outside the job folder.

## Decision
Option 1 for steps: `job.json` gains an optional `templates: {name: StepState}` map, still
version 1. Fingerprints cover the template file, the prompt-wrapper version, the backend, the
context budget, the output language and the input's content hash. LLM calls are cached per
job, keyed by (backend id, context budget, system, user). A forced template skips cache reads.

## Consequences
- Sub-project 1 job folders load unchanged. A sub-project 1 binary drops the map on save,
  which only causes templates to rerun.
- Planning cannot say up front whether a downstream template will be cached. Progress reports
  it when that template's turn comes.
- `llm-cache/` grows with every changed prompt. Nothing prunes it in #2.
- Bumping `PROMPT_VERSION` reruns every template, which is intended when wrappers change.
- Revisit if templates need to run before or between pipeline steps (for example an edit plan
  in sub-project 5); that is when option 2 would pay off.
