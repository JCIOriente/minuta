# Structured fields and social templates — Implementation Plan

- Spec: docs/specs/2026-10-25-fields-and-social-templates.md
- Date: 2026-10-25
- Status: in backlog

## Overview
New logic goes in a new module, `src/fields/`: the shared field set and its types, answer
parsing and validation, prompts and schema, rendering and the JSON artifact. Existing
modules change only where the spec requires it:
- `template` (frontmatter and built-ins)
- `config` (`llm.repairs`)
- `llm` (`complete_json`, the Ollama `format` field, `call_json`, and a final-call hook in
  chunking)
- `generate` and `runner`: the fields template step and its wiring.

T1 creates `src/fields/` with the shared types and empty submodules, adds the
`Backend::complete_json` default method, adds `Event::Repair` and teaches the scripted
backend to record schemas. After T1, no other task edits `src/lib.rs`, `src/fields/mod.rs`,
`src/llm/mod.rs`, `src/llm/fake.rs` or `src/progress.rs`.

Leaf tasks are decoupled through plain function signatures:
- `fields::validate` and `fields::{prompt, render}` only need the T1 types.
- `call::call_json` takes an `accept` closure. It does not depend on `fields::validate`.
- `chunk::generate_with` takes a `finish` closure for the final call. It knows nothing about
  JSON or repairs.

T10 joins them into the fields template step (the repair loop lives there). T11 adds the
integration suite.

Shared interfaces, fixed here so parallel tasks agree. Private helpers can be added freely.
A change to a public shape requires updating this plan in the same PR.

```rust
// fields/mod.rs (T1)
pub mod prompt; pub mod render; pub mod validate;
pub const FIELDS_VERSION: u32 = 1;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field { Title, Body, Tags, Hashtags, ThumbnailText }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind { Text, List }
impl Field {
    pub const ALL: [Field; 5];                       // spec table order
    pub fn name(self) -> &'static str;               // "thumbnail_text"
    pub fn from_name(s: &str) -> Option<Field>;      // None for "chapters" too; the parser words that error
    pub fn kind(self) -> Kind;
    pub fn description(self) -> &'static str;        // the spec's "Description given to the model"
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Limit { pub max_chars: Option<usize>, pub max: Option<usize> }
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Format { #[default] Plain, Markdown }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSpec { pub field: Field, pub limit: Limit }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldsSpec { pub fields: Vec<FieldSpec>, pub format: Format }   // declared order
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value { Text(String), List(Vec<String>) }
impl Value { pub fn rendered(&self, field: Field) -> String; }  // text as is; tags "a, b"; hashtags "#a #b"; what max_chars measures
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Values(pub Vec<(Field, Value)>);   // declared order
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation(pub String);             // Display prints the message

// fields/validate.rs (T4)
/// `answer` is already cleaned by `call::clean`. All violations in one pass, in field order.
pub fn parse_answer(answer: &str, spec: &FieldsSpec) -> std::result::Result<Values, Vec<Violation>>;

// fields/prompt.rs (T5)
pub fn fields_block(spec: &FieldsSpec) -> String;                     // spec "Prompts", no language instruction
pub fn repair_system(spec: &FieldsSpec, output_lang: &str) -> String;  // intro + fields block + chunk::language_instruction(output_lang), last
pub fn repair_user(previous: &str, violations: &[Violation]) -> String;
pub fn schema(spec: &FieldsSpec) -> serde_json::Value;
// fields/render.rs (T5)
pub fn render(values: &Values, format: Format) -> String;              // blank-line separated, trailing newline
pub fn artifact_json(template: &str, output_lang_code: &str, values: &Values) -> String; // ADR-0007, pretty, trailing newline

// llm/mod.rs (T1): additive, default method
fn complete_json(&self, system: &str, user: &str, schema: &serde_json::Value,
                 interrupt: &AtomicBool) -> Result<String> { self.complete(system, user, interrupt) }
// llm/fake.rs (T1): ScriptedBackend overrides complete_json, records the schema
impl ScriptedBackend { pub fn schemas(&self) -> Vec<Option<serde_json::Value>>; }  // one entry per call, None for complete

// llm/chunk.rs (T1)
pub enum Event { Chunk { done: usize, total: usize }, Consolidating, Repair { attempt: usize, total: usize } }
// progress.rs (T1): Event::Repair renders "repair 1/2"

// config.rs (T2)
Config { llm_repairs: Setting, ..existing }   // "llm.repairs", env MINUTA_LLM_REPAIRS, default "2", min 0
impl Config { pub fn repairs(&self) -> u32; }

// template/mod.rs (T3)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output { Markdown, Fields(FieldsSpec) }
Template { pub output: Output, ..existing }

// llm/call.rs (T7)
pub enum Answer<T> { Accepted(T), Rejected { text: String, violations: Vec<Violation> } }
/// Cache key adds the schema (compact JSON). Writes the cache only for an accepted answer;
/// a cache hit that `accept` rejects counts as a miss. Transient errors retried per policy.
pub fn call_json<T>(backend: &dyn Backend, cache: &CallCache, policy: &Policy, system: &str,
                    user: &str, schema: &serde_json::Value, refresh: bool, interrupt: &AtomicBool,
                    accept: &dyn Fn(&str) -> std::result::Result<T, Vec<Violation>>) -> Result<Answer<T>>;

// llm/chunk.rs (T8)
/// Like `generate`, but `final_block` (if any) goes before the language instruction in the
/// final system prompt (single call or consolidation), and the final call is made by
/// `finish(system, user, on_event)` instead of `complete`. Map calls are unchanged.
/// `generate` becomes `generate_with(req, None, complete, <calls complete>, on_event)`.
pub fn generate_with(req: &Request, final_block: Option<&str>, complete: Complete,
                     finish: &mut dyn FnMut(&str, &str, &mut dyn FnMut(Event)) -> Result<String>,
                     on_event: &mut dyn FnMut(Event)) -> Result<Generated>;

// generate.rs (T10)
TemplateRun { pub repairs: u32, ..existing }
impl TemplateRun<'_> { pub fn artifacts(&self) -> Vec<PathBuf>; }  // replaces artifact(): ["<n>.md"] or ["<n>.json", "<n>.md"]
```

## Contract
No HTTP API of minuta's own. Public contracts and owners:
- Frontmatter keys `fields`, `limits` and `format` (ADR-0004, ADR-0006): T3.
- Config key `llm.repairs`: T2.
- `templates list` OUTPUT column: T3.
- The `<name>.json` layout (ADR-0007): T5 writes it, T10 puts it in the job folder.
- The Ollama `format` request field: T6. The fake server already records full request
  bodies (`tests/support/fake_ollama.rs:17`), so it needs no change.

## Impact
- **`Backend` trait** (shared): additive default method. `claude` and `ScriptedBackend` behave
  as before for `complete`. `tests/claude.rs` and `tests/ollama.rs` must stay green (T1, T6).
- **Call cache** (`src/llm/call.rs:779`): keys of `complete` calls must not change, or every
  existing job folder loses its cache. T7 adds a test pinning the key of a fixed input to
  the value computed on `main` before the change.
- **Chunking** (`src/llm/chunk.rs:107`): `generate` must behave exactly as before. All
  existing chunk tests and `tests/generate.rs` must stay green (T8).
- **Fingerprint of `markdown` templates** (`src/generate.rs:79`): must not change. T10 adds a
  test pinning a markdown template's fingerprint to the value computed on `main`.
- **Template parsing**: `Template` gains `output`. It is only built by `Template::parse`
  (`src/template/mod.rs:177`), so no other constructor breaks. `tests/templates.rs` stays green.
- **CLI**: `templates list` gains a column. The header assertion in `tests/cli.rs:1190` is
  updated in T3, deliberately.
- **Config**: new key under `deny_unknown_fields`. An older binary rejects a config file
  that sets `llm.repairs`, which is acceptable before 1.0.
- `job.json`: no schema change. A fields template records two artifacts in the existing list.
- Rollout and rollback: one crate release. Reverting the merges restores sub-project 2.
  `<name>.json` files left in job folders are ignored by the older binary.

## Dependency graph
```
T1 ──┬─> T3 ──┬─> T9 ─────────────┐
     ├─> T4 ──┤                   │
     ├─> T5 ──┼─> T10 ──> T11 <───┘
     ├─> T6 ──┼───────────^
     ├─> T7 ──┤
     └─> T8 ──┘
T2 (independent) ──> T10
```
- Start immediately: T1 and T2.
- After T1, in parallel: T3, T4, T5, T6, T7, T8.
- T9 needs T3.
- T10 needs T2, T3, T4, T5, T7 and T8.
- T11 needs T6, T9 and T10.

## Suggested agent assignment
| Task | Model | Effort | Why |
|---|---|---|---|
| T1 | sonnet | medium | shared types and a trait default; names must match the plan exactly |
| T2 | haiku | low | one key following the existing config pattern |
| T3 | sonnet | medium | typed frontmatter and a list of precise errors |
| T4 | sonnet | high | untrusted input, normalization, boundary counts |
| T5 | sonnet | medium | pure string and JSON builders |
| T6 | sonnet | medium | one request field and a hint, against the fake server |
| T7 | opus | high | accepted-only caching is the core invariant of ADR-0006 |
| T8 | opus | high | budget invariants of chunking must hold with the extra block |
| T9 | sonnet | high | prompt quality and exact limits |
| T10 | opus | high | repair loop, artifacts invariant, fingerprint, wiring |
| T11 | sonnet | medium | integration tests on the existing fakes |
Reviews run on opus.

## Tasks

### T1: Foundations: fields types, `complete_json`, repair event  (#56)
- Priority: 0
- Depends on: none
- Owns: `src/lib.rs`, `src/fields/mod.rs`, empty `src/fields/{validate,render,prompt}.rs`, `src/llm/mod.rs`, `src/llm/fake.rs`, `src/progress.rs`
- Touches (read-only or additive): `src/llm/chunk.rs` (the `Event::Repair` variant only)
- Acceptance:
  - [ ] Every shape under `fields/mod.rs (T1)`, `llm/mod.rs (T1)`, `llm/fake.rs (T1)`,
        `llm/chunk.rs (T1)` and `progress.rs (T1)` in "Shared interfaces" exists with that
        signature.
  - [ ] `Field::from_name` round-trips `name()` for each of the 5 fields (one case each) and
        returns `None` for `chapters` and `hook`.
  - [ ] `Field::kind`: title, body and thumbnail_text are Text; tags and hashtags are List
        (one case each).
  - [ ] `Value::rendered`: text unchanged, tags `a, b`, hashtags `#a #b`, a single item, and
        an empty list as `""`.
  - [ ] The default `complete_json` calls `complete` (tested with a minimal backend that only
        implements `complete`).
  - [ ] `ScriptedBackend::schemas()` records `Some(schema)` for `complete_json` and `None` for
        `complete`.
  - [ ] Progress shows `repair 1/2` for `Event::Repair { attempt: 1, total: 2 }`.
  - [ ] `make ci` passes with no other behavior change.
- Tests first: `field_names_round_trip`, `field_kinds`, `chapters_is_not_a_field`,
  `rendered_tags_comma_separated`, `rendered_hashtags_with_hash`,
  `complete_json_defaults_to_complete`, `scripted_backend_records_schema`,
  `repair_event_text`
- Notes: the descriptions come verbatim from the spec's shared-fields table. Keep
  `fields/{validate,render,prompt}.rs` as empty files with a module doc comment only.

### T2: Config key `llm.repairs`  (#57)
- Priority: 1
- Depends on: none
- Owns: `src/config.rs`, `tests/config.rs`
- Acceptance:
  - [ ] `llm.repairs` defaults to 2; the config file sets it; `MINUTA_LLM_REPAIRS` overrides
        the file (one test each).
  - [ ] `0` is accepted; a negative or non-integer value is a config error naming the key.
  - [ ] `config show` lists `llm.repairs` with its source.
  - [ ] `Config::repairs()` returns the resolved value.
- Tests first: `repairs_default_is_two`, `repairs_from_file`, `repairs_env_overrides_file`,
  `repairs_zero_allowed`, `repairs_rejects_non_integer`, `config_show_lists_repairs`
- Notes: copy the `llm.retries` pattern (`src/config.rs:78`, `:302`, `:312`), but the minimum
  is 0, not 1.

### T3: Frontmatter `output: fields`, `fields`, `limits`, `format`; `templates list` OUTPUT  (#58)
- Priority: 1
- Depends on: T1
- Owns: `src/template/mod.rs`, `tests/templates.rs`, `src/cli.rs` (`templates_list` only)
- Touches (read-only or additive): `tests/cli.rs` (the header assertion at line 1190)
- Acceptance:
  - [ ] `output: fields` with `fields: [title, body, tags]` parses into
        `Output::Fields` with fields in declared order, the limits given, and
        `format: Plain` by default; `format: markdown` gives `Markdown`.
  - [ ] Each spec frontmatter error is a usage error naming the template (one test each):
        `fields`/`limits`/`format` without `output: fields` (3 cases); `output: fields` with
        no `fields`; an empty list; an unknown field (message lists the 5 names); `chapters`
        (message says "arrives in a later version"); a duplicate; a limit for an undeclared
        field; an unknown limit key; `max` on a text field; `max_chars: 0`; `max: 0`; an
        unknown format.
  - [ ] A markdown template has `Output::Markdown`, and its `raw` and `body` are unchanged.
  - [ ] `templates list` prints `NAME  LAYER  INPUT  OUTPUT  LLM  DESCRIPTION`, with `fields`
        for a fields template and `markdown` otherwise.
- Tests first: `fields_frontmatter_parses`, `format_defaults_to_plain`, `format_markdown`,
  one `fields_error_*` test per error above, `markdown_template_output_markdown`,
  `templates_list_shows_output_column`
- Notes: use typed `deny_unknown_fields` structs (ADR-0004). Limits are a map from field name
  to `{ max_chars, max }`. Validate the names against `Field::from_name` after
  deserializing, so errors read in minuta's words. Remove the "arrives in a later version"
  rejection for `output: fields` (`src/template/mod.rs:193`), but keep that wording for
  `chapters`.

### T4: Answer parsing, normalization and violations  (#59)
- Priority: 1
- Depends on: T1
- Owns: `src/fields/validate.rs`
- Acceptance:
  - [ ] Parses a bare object; an object surrounded by prose (first `{` to last `}`); text
        with no object gives exactly `the answer is not a JSON object (…)`.
  - [ ] Each violation message in the spec, word for word, one test each: missing key,
        not a string, not a list of strings, empty text, empty list, hashtag with
        whitespace, over `max_chars` (text), over `max`, over `max_chars` (rendered list).
  - [ ] Violations from several fields come back together, in declared field order.
  - [ ] Boundaries: exactly `max_chars` passes and `max_chars + 1` fails; `é` and an emoji
        made of one scalar value count as one character each.
  - [ ] Normalization: `\r\n` becomes `\n`; C0 controls other than `\n`/`\t` and DEL are
        removed; values are trimmed; empty list items are dropped; leading `#`s are stripped
        from hashtags (`##rust` becomes `rust`); keys that were not declared are dropped from
        `Values`.
  - [ ] `max_chars` is measured after normalization, using `Value::rendered`.
- Tests first: `parses_bare_object`, `parses_object_inside_prose`, `not_json_violation`,
  one `violation_*` test per message, `violations_in_field_order`,
  `max_chars_boundary_inclusive`, `multibyte_counts_as_one`, `normalizes_crlf`,
  `strips_control_chars`, `drops_empty_list_items`, `strips_leading_hashes`,
  `ignores_undeclared_keys`
- Notes: the hashtag message is the only one that quotes model text. Print it with `{:?}`
  quoting as in the spec; the CLI sanitizes it later. Never panic on any input: add a
  test with a non-object JSON value (`[1]`, `"x"`, `null`).

### T5: Fields prompts, schema, rendering and JSON artifact  (#60)
- Priority: 1
- Depends on: T1
- Owns: `src/fields/prompt.rs`, `src/fields/render.rs`
- Acceptance:
  - [ ] `fields_block` lists each declared field in order with its kind wording ("string" /
        "array of strings"), description and limits ("At most 100 characters.", "At most 15
        items, at most 500 characters joined with \", \"." for tags, joined with " " and
        prefixed `#` for hashtags), then the plain or markdown sentence and the keys sentence.
  - [ ] `repair_system` ends with `chunk::language_instruction(output_lang)` and contains the
        fields block; `repair_user` is `Previous answer:\n<previous>\n\nProblems:\n- …`.
  - [ ] `schema` has `type: object`, a property per declared field (string, or array of
        strings), `required` = the declared names in order, and no length keywords.
  - [ ] `render`: each field kind in each format; `markdown` changes only `title` to
        `# …`; blank-line separation; one trailing newline; declared order kept.
  - [ ] `artifact_json` matches ADR-0007 exactly (a golden string), round-trips through
        `serde_json`, and holds only the given fields.
- Tests first: `fields_block_lists_fields_in_order`, `fields_block_text_limit`,
  `fields_block_tags_limit`, `fields_block_hashtags_limit`, `fields_block_plain_sentence`,
  `fields_block_markdown_sentence`, `repair_system_language_last`, `repair_user_layout`,
  `schema_text_and_list`, `schema_required_in_order`, `render_plain_*`,
  `render_markdown_title_heading`, `render_trailing_newline`, `artifact_json_golden`
- Notes: prompt text is the spec's, verbatim, in English.

### T6: Ollama structured output (`format` schema)  (#61)
- Priority: 1
- Depends on: T1
- Owns: `src/llm/ollama.rs`, `tests/ollama.rs`
- Acceptance:
  - [ ] `complete_json` sends the same body as `complete` plus `"format": <schema>`;
        `complete` sends no `format` (both asserted on fake-server request bodies).
  - [ ] An HTTP 400 on a `complete_json` request is a non-transient error whose message
        contains "structured output needs Ollama 0.5 or later"; a 400 on `complete` keeps
        today's message.
  - [ ] Interrupt, retry classification and `think: false` behave the same for
        `complete_json` (one test each, reusing the existing cases).
- Tests first: `complete_json_sends_format`, `complete_sends_no_format`,
  `json_400_hints_ollama_version`, `json_interrupt_returns_interrupted`
- Notes: build the body once (`src/llm/ollama.rs:177`) and add the field; keep one request
  path. Every test sets the fake server URL explicitly (docs/common-issues.md).

### T7: `call_json`: JSON calls cached only when accepted  (#62)
- Priority: 0
- Depends on: T1
- Owns: `src/llm/call.rs`
- Acceptance:
  - [ ] Before any change, record the key `cache_key` gives on `main` for a fixed
        (backend id, context, system, user), and add a test pinning it.
  - [ ] `call_json` uses `complete_json` with the schema. Its key differs from the `complete`
        key for the same inputs and changes when the schema changes.
  - [ ] Accepted answer: written to the cache; a second call with the same inputs makes no
        backend request and returns `Accepted`.
  - [ ] Rejected answer: `Rejected { text, violations }` with the cleaned text, and no cache
        file is written (asserted on the directory).
  - [ ] A cache entry that `accept` rejects counts as a miss: the backend is called and a
        newly accepted answer overwrites the entry.
  - [ ] `refresh = true` skips the read and writes on acceptance.
  - [ ] Transient errors and empty output are retried as in `call`; interrupt returns
        `Error::Interrupted`.
- Tests first: `complete_cache_key_unchanged`, `json_key_includes_schema`,
  `accepted_answer_cached`, `rejected_answer_not_cached`, `rejected_cache_hit_is_miss`,
  `json_refresh_skips_read`, `json_transient_retried`
- Notes: reuse `call_with`'s retry loop rather than copying it (pass the backend call and the
  commit decision in). Keep the atomic cache write unchanged, including the directory fsync
  (lesson from #44).

### T8: Final-call hook in chunking (`generate_with`)  (#63)
- Priority: 0
- Depends on: T1
- Owns: `src/llm/chunk.rs`
- Acceptance:
  - [ ] `generate` keeps its signature and behavior: every existing chunk test and
        `tests/generate.rs` pass unchanged.
  - [ ] Single-call path: the final system prompt is body + `\n\n` + final block + `\n\n` +
        language instruction, and it goes to `finish`, not `complete`.
  - [ ] Chunked path: map prompts contain no final block. The consolidation prompt is body
        + consolidation sentence + final block + language instruction, and it goes to
        `finish`.
  - [ ] The fit check and the reduce loop measure the final prompt with the block, so no
        final request exceeds the budget (property test as in
        `chunks_never_exceed_budget`).
  - [ ] Events that `finish` emits through its `on_event` argument reach the caller, after
        `Consolidating`.
  - [ ] `Generated.calls` counts the `finish` call as 1.
- Tests first: `single_call_final_block_before_language`, `finish_used_for_single_call`,
  `map_prompts_have_no_final_block`, `consolidation_has_final_block`,
  `final_with_block_never_exceeds_budget`, `finish_events_forwarded`
- Notes: the block makes the final prompt longer, so an input that fit without it may need
  chunking. That is correct; test the boundary.

### T9: Built-in `youtube`, `linkedin` and `instagram` templates  (#64)
- Priority: 2
- Depends on: T3
- Owns: `src/template/builtin/{youtube,linkedin,instagram}.md`
- Touches (read-only or additive): `src/template/mod.rs` (the `BUILTIN` array only), `tests/templates.rs` (additive tests)
- Acceptance:
  - [ ] Each built-in parses with `input: summary`, `output: fields`, `format: plain` and the
        fields in the spec table (one test per template).
  - [ ] Each limit in the spec's built-in table, one assertion per limit (11 cases).
  - [ ] Prompts meet the spec's "Prompt requirements", including the `youtube` chapters
        placeholder line in the output language, and "do not invent facts".
  - [ ] Prompts use only known variables and are written in English.
  - [ ] `templates list` shows the three as `built-in` with OUTPUT `fields`.
- Tests first: `builtin_youtube_shape`, `builtin_linkedin_shape`,
  `builtin_instagram_shape`, one `builtin_<template>_<field>_limit` test per limit
- Notes: keep prompts short and concrete. Optionally try each prompt once by hand against
  local Ollama (`gemma4:26b`) through T10's code once it lands, and report the result in the
  PR. That is not a CI check.

### T10: Fields template step: repair loop, artifacts, fingerprint, wiring  (#65)
- Priority: 0
- Depends on: T2, T3, T4, T5, T7, T8
- Owns: `src/generate.rs`, `src/runner.rs`, `tests/generate.rs`
- Acceptance:
  - [ ] Before any change, record a markdown template's fingerprint on `main` and add a test
        pinning it. The test passes after the change.
  - [ ] Fields fingerprint = the markdown parts + `FIELDS_VERSION`; changing the constant
        changes it.
  - [ ] `artifacts()` is `["<n>.md"]` for markdown and `["<n>.json", "<n>.md"]` for fields;
        the runner records exactly these in `job.json`.
  - [ ] With a `ScriptedBackend`: a valid first answer writes both files, makes 1 call and
        sends the schema.
  - [ ] Over the limit, then valid: 2 calls, the second with `repair_system`/`repair_user`
        naming the violation, `Event::Repair { attempt: 1, total: 2 }` emitted,
        `Generated.calls == 2`.
  - [ ] Always invalid with `repairs = 2`: 3 calls, then `Error::Template` whose message
        follows the spec ("still breaks N limit(s) after 2 repairs: …") with the hint.
        Neither file exists.
  - [ ] `repairs = 0`: the first violation fails.
  - [ ] Invalid JSON, then valid: 2 calls.
  - [ ] `forced` passes `refresh = true` to every `call_json`.
  - [ ] JSON is written before Markdown, both with `write_atomic`.
  - [ ] The runner sets `repairs` from `Config::repairs()`.
- Tests first: `markdown_fingerprint_unchanged`, `fields_fingerprint_includes_version`,
  `fields_artifacts`, `fields_first_answer_valid`, `fields_repair_then_valid`,
  `fields_repairs_exhausted_writes_nothing`, `fields_zero_repairs_fails_first`,
  `fields_invalid_json_then_valid`, `fields_forced_refreshes`
- Notes: the repair loop is the `finish` closure passed to `generate_with`. First
  `call_json(final system, user)`; on `Rejected`, emit `Event::Repair` and call
  `call_json(repair_system, repair_user(text, violations))`. Each answer is checked with
  `parse_answer`. Map calls keep using `call`. The user message of the final call is the
  input as today. Stop on `Error::Interrupted` without writing.

### T11: Integration suite and opt-in e2e for fields templates  (#66)
- Priority: 2
- Depends on: T6, T9, T10
- Owns: `tests/integration/fields.rs`, `tests/e2e_llm.rs`
- Touches (read-only or additive): `tests/integration/main.rs` (a `mod fields;` line), `tests/integration/common.rs` (additive helpers)
- Acceptance (each item in the spec's integration list is one test):
  - [ ] `run -t youtube` writes `summary.md`, `youtube.json` and `youtube.md` and records both
        youtube artifacts. A second run makes zero chat requests.
  - [ ] Over the limit, then valid: 2 youtube chat requests, the second a repair naming the
        violation.
  - [ ] Always over the limit: exit 1, stderr lists the violations, no youtube artifacts.
        A rerun makes a new final request (count checked).
  - [ ] Invalid JSON, then valid.
  - [ ] Chunked input: map requests carry no `format`, the final request does, with the
        expected schema; repair requests carry it too.
  - [ ] Fake claude: `--system-prompt` contains the fields block, and a scripted JSON reply
        produces both artifacts.
  - [ ] A project template with `input: youtube` receives `youtube.md` as its user message.
  - [ ] Editing a limit in a project override of `youtube` reruns only it and its
        dependents.
  - [ ] `templates list` shows OUTPUT for built-in and project templates.
  - [ ] With `MINUTA_E2E_LLM=1`, `-t youtube` against the real local Ollama produces a
        `youtube.json` that parses and meets the built-in limits. Skipped otherwise.
- Tests first: the test names mirror the bullets (`fields_run_then_cached`,
  `fields_repair_request`, `fields_repairs_exhausted`, `fields_invalid_json_repaired`,
  `fields_chunked_format_only_final`, `fields_on_claude`, `fields_chained_reads_md`,
  `fields_limit_edit_reruns`, `templates_list_output_column`, `e2e_youtube_limits`)
- Notes: script fake answers per request index (`chat_index`). Every environment sets
  `MINUTA_OLLAMA_URL`. No sleeps; poll with generous deadlines (docs/common-issues.md).

## Coverage check
| Spec requirement | Task(s) |
|---|---|
| Goal 1: `-t youtube` runs summary then youtube; both artifacts recorded | T9, T10, T11 |
| Goal 2: JSON layout, only declared fields, hashtags without `#`, limits met | T4, T5, T10 |
| Goal 3: repair call names field, length and limit; 2 calls | T4, T5, T10, T11 |
| Goal 4: fail after `1 + llm.repairs` answers; message; no artifacts | T10, T11 |
| Goal 5: not JSON, missing key and wrong type count as violations | T4, T10, T11 |
| Goal 6: rejected answers never read from cache; map calls stay cached | T7, T11 |
| Goal 7: Ollama `format` on final and repair calls only; claude prompt only | T6, T10, T11 |
| Goal 8: chunked input asks for JSON only in the final call | T8, T11 |
| Goal 9: cached rerun; edits rerun; markdown fingerprints unchanged | T7, T10, T11 |
| Goal 10: frontmatter errors exit 2 | T3 |
| Goal 11: built-ins and their limits | T9 |
| Goal 12: `templates list` OUTPUT column | T3, T11 |
| Goal 13: language instruction last; keys stay English | T5, T8 |
| Goal 14: hermetic tests | all (T11 checks the environment) |
| Normalization and security notes | T4 |
| `llm.repairs` config | T2, T10 |
| Progress `repair 1/2`, calls count includes repairs | T1, T10 |
| HTTP 400 hint for old Ollama | T6 |
| Opt-in e2e | T11 |
