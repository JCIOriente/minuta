# Templates and LLM backends — Implementation Plan

- Spec: docs/specs/2026-10-24-templates-and-backends.md
- Date: 2026-10-24
- Status: approved

## Overview
New code lives in new modules: `lang`, `template/`, `llm/` and `generate`. Existing modules
change only where the spec requires it: `config`, `job`, `tools` (additive),
`runner`/`cli`/`progress` (integration). T1 declares every new module as an empty file, adds
the one new dependency and fixes the shared types (`Backend`, `BackendId`, the new `Error`
variants, the language table). After T1, no other task edits `lib.rs`, `Cargo.toml` or
`error.rs`.

The leaf tasks are decoupled through plain function signatures, so they can run in parallel:
- `template::graph` works on a name -> input lookup and does not need the `Template` type.
- `llm::chunk::generate` takes a `complete` closure. It needs neither the call cache nor any
  real backend.
- The backends only implement the trait from T1.

`generate` (T10) ties templates, the call layer and job state into one template step.
`runner`/`cli` (T11) wires it into the CLI. T12 adds the end-to-end integration suite.

Shared interfaces, fixed here so parallel tasks agree. Private helpers can be added freely.
A change to a public shape requires updating this plan in the same PR.

```rust
// error.rs (T1): new variants
Error::Llm { backend: String, message: String, transient: bool } // exit 1; Display "{backend}: {message}"
Error::Template { name: String, message: String }                // exit 1; runtime failure of one template
// Resolution problems (bad frontmatter, unknown template or variable, cycle, bad backend id or
// language code) are Error::Usage (exit 2).

// llm/mod.rs (T1)
pub trait Backend: Send + Sync {
    fn id(&self) -> &str;                       // canonical id, e.g. "ollama:gemma4:26b"
    fn context_tokens(&self) -> usize;          // prompt + output budget per call
    fn complete(&self, system: &str, user: &str, interrupt: &AtomicBool) -> Result<String>;
}
pub enum BackendId { Ollama { model: String }, Claude { model: Option<String> } }
impl FromStr for BackendId { type Err = Error; }   // Usage error on anything else
impl Display for BackendId;                        // round-trips: "ollama:gemma4:26b", "claude", "claude:sonnet"
impl BackendId { pub fn sends_off_machine(&self) -> bool; }  // Claude => true
pub const DEFAULT_LLM: &str = "ollama:gemma4:26b";
#[cfg(test)] pub mod fake;   // ScriptedBackend: Vec of replies/errors, records (system, user)

// lang.rs (T1)
pub fn name(code: &str) -> Option<&'static str>;       // "es" -> Some("Spanish"); whisper's table
pub fn validate(code: &str) -> Result<&'static str>;   // Usage error naming the code

// config.rs (T2): new fields, strings like the existing ones, numbers validated in resolve
Config { llm_default: Setting, llm_output_lang: Option<Setting>, llm_concurrency: Option<Setting>,
         llm_retries: Setting, llm_backoff_ms: Setting, ollama_url: Setting,
         ollama_num_ctx: Setting, claude_bin: Setting, ..existing }
Overrides { llm_default: Option<String>, llm_output_lang: Option<String>, ..existing }
impl Config { pub fn retries(&self) -> u32; pub fn backoff(&self) -> Duration;
              pub fn num_ctx(&self) -> usize; pub fn concurrency(&self) -> Option<usize>; }

// template/mod.rs (T3)
pub enum Layer { BuiltIn, User, Project }
pub enum Input { Transcript, Template(String) }
pub struct Template { pub name: String, pub layer: Layer, pub path: Option<PathBuf>, // None = built-in
                      pub raw: String, pub description: Option<String>, pub input: Input,
                      pub llm: Option<BackendId>, pub lang: Option<String>, pub body: String }
pub struct Vars { pub output_lang: String, pub output_lang_code: String, pub speech_lang: String,
                  pub job_name: String, pub duration: String }
pub const VARIABLES: [&str; 5];
impl Template { pub fn parse(name: &str, layer: Layer, path: Option<PathBuf>, text: &str) -> Result<Template>;
                pub fn render(&self, vars: &Vars) -> String; }
// template/discover.rs (T3)
pub struct Dirs { pub user: Option<PathBuf>, pub project: PathBuf }
impl Dirs { pub fn standard() -> Result<Dirs>; }   // <home>/.config/minuta/templates, ./templates
pub struct Invalid { pub path: PathBuf, pub layer: Layer, pub error: String }
pub struct Catalog { pub templates: BTreeMap<String, Template>, pub invalid: Vec<Invalid> }
pub fn discover(dirs: &Dirs) -> Result<Catalog>;
impl Catalog { pub fn get(&self, name: &str) -> Result<&Template>; } // unknown: Usage listing names; invalid: its error

// template/graph.rs (T4)
/// input_of(name): None = unknown template, Some(None) = transcript, Some(Some(dep)) = template dep
pub fn order(requested: &[String], input_of: &dyn Fn(&str) -> Option<Option<String>>) -> Result<Vec<String>>;
pub fn dependents(order: &[String], of: &str, input_of: &dyn Fn(&str) -> Option<Option<String>>) -> Vec<String>;

// llm/ollama.rs (T5)
pub struct OllamaInfo { pub context_length: Option<usize>, pub thinking: bool }
pub fn check(url: &str, model: &str) -> std::result::Result<OllamaInfo, Missing>;  // /api/tags + /api/show
pub struct Ollama;  impl Ollama { pub fn new(url: &str, model: &str, num_ctx: usize, info: &OllamaInfo) -> Ollama;
                                  pub fn with_timeout(self, Duration) -> Ollama; }   // impl Backend

// llm/claude.rs + tools.rs (T6)
pub fn check(bin: &str) -> std::result::Result<ToolInfo, Missing>;
pub struct Claude;  impl Claude { pub fn new(bin: PathBuf, model: Option<String>) -> Claude; }   // impl Backend
// tools.rs, additive: stdin bytes in, stdout returned, stderr lines streamed
pub fn run_capture(argv: &[OsString], stdin: &[u8], opts: RunOpts, on_stderr: &mut dyn FnMut(&str)) -> Result<String>;

// llm/call.rs (T7)
pub struct Policy { pub retries: u32, pub backoff: Duration }
pub struct CallCache { /* <job>/llm-cache */ }
impl CallCache { pub fn new(job_dir: &Path) -> CallCache; }
pub fn call(backend: &dyn Backend, cache: &CallCache, policy: &Policy, system: &str, user: &str,
            refresh: bool, interrupt: &AtomicBool) -> Result<String>;   // refresh = skip cache read
pub fn clean(output: &str) -> String;

// llm/chunk.rs (T8)
pub const PROMPT_VERSION: u32 = 1;
pub fn transcript_text(t: &Transcript) -> String;          // >= 30 s blocks, "[hh:mm:ss] text"
pub fn estimate_tokens(s: &str) -> usize;                  // chars / 3, rounded up
pub fn language_instruction(output_lang: &str) -> String;  // "Write the entire response in {lang}, …"
pub struct Request<'a> { pub body: &'a str, pub output_lang: &'a str, pub input: &'a str,
                         pub context_tokens: usize, pub concurrency: usize }
pub enum Event { Chunk { done: usize, total: usize }, Consolidating }
pub struct Generated { pub text: String, pub calls: usize }
pub fn generate(req: &Request, complete: &(dyn Fn(&str, &str) -> Result<String> + Sync),
                on_event: &mut dyn FnMut(Event)) -> Result<Generated>;

// job/ (T9)
JobState { #[serde(default)] pub templates: BTreeMap<String, StepState>, ..existing }
pub fn template_reason(job: &Job, name: &str, fingerprint: &str, forced: bool) -> Option<Reason>;
impl Job { pub fn mark_template_stale(&mut self, name: &str, reason: &Reason) -> Result<()>;
           pub fn mark_template_done(&mut self, name: &str, fingerprint: &str, seconds: f64,
                                     artifacts: &[PathBuf]) -> Result<()>; }

// generate.rs (T10)
pub fn output_lang(flag: Option<&str>, template: Option<&str>, config: Option<&str>, detected: &str) -> Result<String>; // a code
pub fn backend_id(flag: Option<&BackendId>, template: Option<&BackendId>, default: &BackendId) -> BackendId;
pub struct TemplateRun<'a> { pub template: &'a Template, pub backend: &'a dyn Backend,
                             pub output_lang: String /* code */, pub forced: bool }
impl TemplateRun<'_> {
    pub fn artifact(&self) -> PathBuf;                               // "<name>.md"
    pub fn fingerprint(&self, ctx: &Ctx) -> Result<String>;          // reads the input artifact
    pub fn execute(&self, ctx: &Ctx, job_name: &str, policy: &Policy, concurrency: usize,
                   interrupt: &AtomicBool, on_event: &mut dyn FnMut(Event)) -> Result<Generated>;
}
```

## Contract
No HTTP API of minuta's own. Public contracts are the CLI and config keys (spec Interfaces and
Configuration), the template file format (ADR-0004) and the `job.json` `templates` map
(ADR-0005). Owners: T2 config keys, T3 template format, T9 `job.json`, T11 CLI. The Ollama
`/api/chat`, `/api/tags` and `/api/show` shapes that T5 relies on are mirrored by the fake
server T5 writes in `tests/support/fake_ollama.rs`. T12 reuses it.

## Impact
- **`job.json`** (shared module `job`): additive optional `templates` map, version stays 1.
  Consumers are `runner` and the integration tests. `tests/job.rs` and
  `tests/integration/pipeline.rs` must stay green (T9 acceptance). Rollback: an older binary
  ignores the map.
- **Config** (shared module `config`): new keys. `deny_unknown_fields` stays, so an older
  binary rejects a config file that uses the new keys. That is acceptable before 1.0.
  `tests/config.rs` must stay green (T2).
- **`tools`**: `run_capture` is a new function. `run` and `RunOpts` do not change, so no
  existing caller is touched. `tests/tools.rs` must stay green (T6).
- **CLI**: `--force` now takes a step or template name. Progress totals now include
  templates. `run` without `-t` must behave exactly as before. The existing
  `tests/integration` and `tests/cli.rs` suites must pass unchanged (T11 acceptance).
- Rollout and rollback: a single crate release. Reverting the merge commits restores
  sub-project 1 behavior. Job folders stay readable either way.

## Dependency graph
```
T1 ──┬─> T3 ──┐
     ├─> T4 ──┼──────────────────────┐
     ├─> T5 ──┼──────────────────────┤
     ├─> T6 ──┼──────────────────────┤
     ├─> T7 ──┼─> T10 ──> T11 ──> T12│
     ├─> T8 ──┤    ^         ^       │
     └─> T9 ──┘    │         └───────┘ (T11 also needs T2, T4, T5, T6)
T2 (independent) ──────────────┘
```
- Start immediately: T1 and T2.
- After T1, in parallel: T3, T4, T5, T6, T7, T8, T9.
- T10 needs T3, T7, T8 and T9.
- T11 needs T2, T4, T5, T6 and T10.
- T12 needs T11.

## Suggested agent assignment
| Task | Model | Effort | Why |
|---|---|---|---|
| T1 | sonnet | medium | scaffolding plus shared types; the language table must be exact |
| T2 | haiku | low | follows the existing config pattern key by key |
| T3 | sonnet | medium | parsing, validation, layered discovery |
| T4 | sonnet | low | small graph algorithm, precise messages |
| T5 | sonnet | high | HTTP, interrupt, error classification, fake server |
| T6 | sonnet | medium | subprocess with stdin, fixture, flags |
| T7 | sonnet | medium | cache and retry loop |
| T8 | opus | high | chunking invariants, map/reduce, concurrency |
| T9 | sonnet | medium | additive job state with tests |
| T10 | opus | high | fingerprint semantics, the core invariant of ADR-0005 |
| T11 | opus | high | integration, preflight, interrupt, CLI |
| T12 | sonnet | medium | integration tests on fixtures |
Reviews run on opus.

## Tasks

### T1: Foundations: module skeleton, error variants, Backend trait, language table
- Priority: 0
- Depends on: none
- Owns: `Cargo.toml`, `src/lib.rs`, `src/error.rs`, `src/lang.rs`, `src/llm/mod.rs`, `src/llm/fake.rs`, empty `src/template/{mod,discover,graph}.rs`, empty `src/llm/{ollama,claude,call,chunk}.rs`, empty `src/generate.rs`
- Touches (read-only or additive): none
- Acceptance:
  - [ ] `serde_yaml_ng` added to `[dependencies]`. No later task edits `Cargo.toml` `[dependencies]`, `lib.rs` or `error.rs`.
  - [ ] `Error::Llm` and `Error::Template` exist with the Display formats above and exit code 1
  - [ ] `"ollama:gemma4:26b".parse::<BackendId>()` gives model `gemma4:26b`; `claude` and `claude:sonnet` parse; `ollama:`, `ollama`, `gpt:x` and the empty string are `Error::Usage`; `Display` round-trips each valid id
  - [ ] `BackendId::sends_off_machine` is true only for Claude
  - [ ] `lang::name` covers every language code whisper.cpp accepts, copied from whisper.cpp's `g_lang` table (cite the source in a comment), e.g. `es` -> Spanish, `en` -> English, `pt` -> Portuguese; `lang::validate("xx")` is a Usage error naming `xx`
  - [ ] `llm::fake::ScriptedBackend` (cfg(test)) returns scripted replies or errors in order and records each (system, user)
  - [ ] `make ci` passes
- Tests first: `backend_id_parses_ollama_with_tagged_model`, `backend_id_parses_claude`, `backend_id_parses_claude_model`, `backend_id_rejects_invalid` (one assertion per case), `backend_id_display_round_trips`, `only_claude_sends_off_machine`, `lang_name_known_codes` (one assertion per sampled code), `lang_validate_unknown_is_usage`, `llm_error_exit_code_is_1`, `template_error_exit_code_is_1`
- Notes: the module docs in each empty file state what the module will hold (one line), so `cargo doc` and reviewers see the layout.

### T2: Config keys for LLMs and output language
- Priority: 1
- Depends on: none
- Owns: `src/config.rs`, `tests/config.rs`
- Touches (read-only or additive): none
- Acceptance:
  - [ ] TOML keys `llm.default`, `llm.output_lang`, `llm.concurrency`, `llm.retries`, `llm.backoff_ms`, `ollama.url`, `ollama.num_ctx`, `claude.bin` load, with env vars `MINUTA_LLM`, `MINUTA_OUTPUT_LANG`, `MINUTA_LLM_CONCURRENCY`, `MINUTA_LLM_RETRIES`, `MINUTA_LLM_BACKOFF_MS`, `MINUTA_OLLAMA_URL`, `MINUTA_OLLAMA_NUM_CTX`, `MINUTA_CLAUDE_BIN` (added to `ENV_NAMES`)
  - [ ] precedence flag > env > file > default is checked for each new key, one assertion per key (flags exist only for `llm.default` and `llm.output_lang`)
  - [ ] defaults: `ollama:gemma4:26b`, retries `3`, backoff `2000`, url `http://127.0.0.1:11434`, num_ctx `16384`, claude bin `claude`; output_lang and concurrency unset
  - [ ] a non-integer or zero `llm.retries`, `llm.backoff_ms` (zero allowed here), `llm.concurrency`, `ollama.num_ctx` is `Error::Config` naming the key; one case per key
  - [ ] `entries()` lists every new key in documented order, so `config show` prints them
  - [ ] unknown keys in `[llm]`, `[ollama]`, `[claude]` are rejected
- Tests first: `llm_default_flag_beats_env`, `each_new_key_env_beats_file` (table, one assertion per key), `each_new_key_default`, `numeric_keys_reject_garbage` (table), `backoff_zero_allowed`, `entries_list_new_keys`, `unknown_llm_key_rejected`
- Notes: follow `pick`/`pick_opt`. `backoff_ms = 0` must be valid because the integration tests use it.

### T3: Template parsing, variables and discovery
- Priority: 1
- Depends on: T1
- Owns: `src/template/mod.rs`, `src/template/discover.rs`, `src/template/builtin/summary.md`, `tests/templates.rs`
- Touches (read-only or additive): `src/lang.rs`, `src/llm/mod.rs` (uses `BackendId`, `lang::validate`)
- Acceptance:
  - [ ] frontmatter between a first line `---` and the next `---` line parses into typed fields with `deny_unknown_fields`; a file without a leading `---` line is all body with defaults (`input: transcript`, `output: markdown`)
  - [ ] `output: fields` is a Usage error saying it arrives in a later version; an unknown key, an invalid `llm` id and an unknown `lang` code are Usage errors naming the template; a YAML error includes the file path and line
  - [ ] names must match `^[a-z0-9][a-z0-9_-]*$`; each reserved name (`transcript`, `chapters`, `probe`, `join`, `audio`, `transcribe`, `outputs`, `job`) is rejected
  - [ ] `{{ x }}` and `{{x}}` render; each of the five variables renders (one assertion each); an unknown variable is a Usage error at parse time listing the known ones
  - [ ] `discover` loads built-in, then `<user>/*.md`, then `<project>/*.md`; a later layer replaces an earlier one by name (one test per layer); a missing directory is an empty layer; a directory that cannot be read is an error naming its path; files with invalid stems or contents go to `Catalog::invalid`, not to an error
  - [ ] `Catalog::get` of an unknown name is a Usage error listing available names; of an invalid template, it returns that template's error
  - [ ] the built-in `summary` template (English prompt, uses `{{job_name}}` and `{{duration}}`) parses, and asks for a short overview, key points, decisions and action items when present
  - [ ] a non-UTF-8 file is an error naming the path
- Tests first: `frontmatter_defaults`, `frontmatter_each_key` (table), `no_frontmatter_is_body`, `fields_output_rejected`, `unknown_key_rejected`, `yaml_error_has_line`, `invalid_names_rejected` (table), `reserved_names_rejected` (table), `renders_each_variable` (table), `unknown_variable_rejected`, `project_overrides_user`, `user_overrides_builtin`, `missing_dir_is_empty`, `unreadable_dir_is_error`, `invalid_file_listed_not_fatal`, `get_unknown_lists_names`, `builtin_summary_parses`
- Notes: `Dirs` makes discovery testable without touching the real HOME or current directory. Use `try_exists`/`read_dir` errors per conventions; only `NotFound` means an empty layer.

### T4: Template dependency graph
- Priority: 1
- Depends on: T1
- Owns: `src/template/graph.rs`
- Touches (read-only or additive): none
- Acceptance:
  - [ ] `order(["b"], …)` with `b: input a`, `a: transcript` returns `["a", "b"]`
  - [ ] a diamond (`d` needs `b` and `c`, which both need `a`) puts `a` first and `d` last, with each template once
  - [ ] independent templates keep `-t` order
  - [ ] an unknown requested template, and an unknown input, are Usage errors naming both sides (`template b: unknown input template z`)
  - [ ] self-reference and longer cycles are Usage errors `template cycle: a -> b -> a`, with the path starting at the first requested template on the cycle
  - [ ] `dependents(order, "a", …)` returns `a` plus every template in `order` that transitively depends on it, in order
- Tests first: `chain_runs_dependency_first`, `diamond_runs_each_once`, `independent_keep_request_order`, `unknown_requested_is_usage`, `unknown_input_is_usage`, `self_cycle_named`, `cycle_named_with_path`, `dependents_transitive`
- Notes: pure functions, unit tests in the module.

### T5: Ollama backend and fake Ollama server
- Priority: 1
- Depends on: T1
- Owns: `src/llm/ollama.rs`, `tests/support/fake_ollama.rs`, `tests/ollama.rs`
- Touches (read-only or additive): none
- Acceptance:
  - [ ] `check` reports an unreachable server as `Missing { name: "ollama", hint: "start Ollama (`ollama serve`)…" }` and an absent model as `Missing { name: "ollama:<model>", hint: "ollama pull <model>" }`; a bare model `x` matches `x:latest`
  - [ ] `check` reads `context_length` from `/api/show` `model_info` (any key ending in `.context_length`) and `thinking` from `capabilities`
  - [ ] `complete` posts to `/api/chat` with `stream: false`, system and user messages, `options.num_ctx`, and `think: false` only when the model can think (asserted on recorded requests)
  - [ ] `context_tokens()` is min(num_ctx, context_length), or num_ctx when the length is unknown
  - [ ] connection refused, timeout, HTTP 429 and 5xx are `Error::Llm { transient: true }`; HTTP 404 is non-transient with a pull hint; other 4xx are non-transient; `done_reason: "length"` is non-transient with the hint "raise ollama.num_ctx"
  - [ ] setting `interrupt` while a request hangs returns `Error::Interrupted` within 500 ms (the request thread is abandoned)
  - [ ] `tests/support/fake_ollama.rs` is a self-contained module (std only, plus serde_json): `FakeOllama::start(models, show_info, responder)`, with `url()`, `requests()` (method, path, JSON body) and a chat count; the responder can return any status and body or hang; each connection is handled on its own thread
- Tests first: `check_unreachable_is_missing`, `check_model_absent_hints_pull`, `check_bare_name_matches_latest`, `check_reads_context_and_thinking`, `chat_request_shape`, `think_false_only_for_thinking_models`, `context_is_min_of_num_ctx_and_model`, `status_classification` (table: 429, 500, 503 transient; 400, 404 not), `length_done_reason_is_error`, `interrupt_abandons_hanging_request`
- Notes: use the existing `ureq` 2 (blocking). Read timeout is 10 minutes by default, and `with_timeout` lets tests shorten it. Tests include the fake with `#[path = "support/fake_ollama.rs"] mod fake_ollama;`.

### T6: Claude backend, `tools::run_capture` and fake claude fixture
- Priority: 1
- Depends on: T1
- Owns: `src/llm/claude.rs`, `src/tools.rs`, `tests/fixtures/bin/claude`, `tests/claude.rs`
- Touches (read-only or additive): none (`tests/tools.rs` must stay green)
- Acceptance:
  - [ ] `run_capture` writes `stdin` to the child, returns stdout as a String (lossy UTF-8), streams stderr lines to `on_stderr`, honors `opts.interrupt` and `opts.cwd`, and a non-zero exit is `Error::Tool` with the stderr tail; `run` is unchanged
  - [ ] the argv is `[bin, "-p", "--output-format", "text", "--system-prompt", <system>, <flags that disable tools, session persistence and setting sources>, ("--model", m)?]`, with flags confirmed against `claude --help` on the installed version and recorded verbatim in a test; the user text arrives on stdin
  - [ ] the child runs with `cwd` set to a fresh empty temp dir, removed afterwards
  - [ ] a non-zero exit is `Error::Llm { transient: true }`, and so is empty stdout; interrupt kills the child and returns `Error::Interrupted`
  - [ ] `check` finds the binary (configured path or PATH) and its `--version`; absent is `Missing { name: "claude", hint: "install Claude Code: https://claude.com/claude-code" }`
  - [ ] `context_tokens()` is 150,000
  - [ ] `tests/fixtures/bin/claude` is committed executable (ETXTBSY-safe): it records argv (one arg per line) to `$FAKE_CLAUDE_ARGV` and stdin to `$FAKE_CLAUDE_STDIN`, prints `$FAKE_CLAUDE_REPLY` (or the fixture default), exits with `$FAKE_CLAUDE_EXIT` when set, and answers `--version`
  - [ ] a system prompt containing `$(touch pwned)` and quotes is passed literally and creates no file
- Tests first: `run_capture_round_trips_stdin`, `run_capture_streams_stderr`, `run_capture_nonzero_is_tool_error`, `claude_argv_flags`, `claude_input_on_stdin`, `claude_runs_in_empty_cwd`, `claude_nonzero_is_transient`, `claude_empty_output_is_transient`, `claude_shell_metacharacters_inert`, `check_missing_claude_hints_install`
- Notes: follow the stdin-writer-thread pattern so a large stdin cannot deadlock with stdout reading.

### T7: LLM call layer: cache, retries, cleanup
- Priority: 1
- Depends on: T1
- Owns: `src/llm/call.rs`
- Touches (read-only or additive): `src/job/mod.rs` (uses `write_atomic`, `fingerprint`)
- Acceptance:
  - [ ] the cache key is `fingerprint([backend.id(), context_tokens, system, user])`; changing any one part changes the key (one assertion per part)
  - [ ] a hit returns the stored text without calling the backend; `refresh: true` calls the backend and overwrites the entry
  - [ ] entries are `<job>/llm-cache/<key>.txt`, written with `write_atomic`; the directory is created on first write; a read error other than NotFound is returned, not treated as a miss
  - [ ] transient errors are retried up to `policy.retries` attempts in total, sleeping `backoff * 2^(n-1)` between them; non-transient errors and `Interrupted` are returned at once; after the last attempt the last error is returned
  - [ ] the backoff sleep wakes within 100 ms of `interrupt` and returns `Error::Interrupted`
  - [ ] `clean` trims, removes a leading `<think>…</think>` block, and unwraps output that is one fenced block (with or without a language tag); output that is empty after cleaning is a transient `Error::Llm`; only cleaned, non-empty text is cached
- Tests first: `cache_key_sensitive_to_each_part` (table), `cache_hit_skips_backend`, `refresh_skips_read_and_overwrites`, `cache_read_error_is_error`, `transient_retried_until_success`, `retries_exhausted_returns_last_error`, `non_transient_not_retried`, `backoff_interrupt_returns_quickly`, `clean_strips_think`, `clean_unwraps_fence`, `clean_keeps_inner_fences`, `empty_after_clean_is_transient`
- Notes: use `llm::fake::ScriptedBackend`. Tests use `Duration::ZERO` backoff except the interrupt test.

### T8: Prompt assembly, chunking and consolidation
- Priority: 0
- Depends on: T1
- Owns: `src/llm/chunk.rs`
- Touches (read-only or additive): `src/transcript.rs` (uses `Transcript`)
- Acceptance:
  - [ ] `transcript_text` merges segments into blocks that span at least 30 s (the last block may be shorter), one line each, `[hh:mm:ss] text`, with the block start as its timestamp; an empty transcript gives an empty string
  - [ ] `estimate_tokens` is `ceil(chars / 3)` over Unicode scalar values
  - [ ] the system prompt is the rendered body, a blank line, then `language_instruction(output_lang)` last; the instruction text matches the spec verbatim
  - [ ] when `estimate(system) + estimate(input) + context/4 <= context`, exactly one call is made with (system, input)
  - [ ] otherwise every map call's `estimate(system) + estimate(user) + context/4 <= context` (property test over random inputs and budgets); chunks cut at line boundaries, are filled greedily, keep order, and a single overlong line is split at whitespace
  - [ ] map calls run concurrently, at most `concurrency` at a time (asserted with a counting `complete`); notes are joined in order under `## Part i` headings for the reduce call, whose system prompt holds the body, the consolidation sentence and the language instruction
  - [ ] when the joined notes do not fit, they are merged in rounds until they do; a round that does not reduce the total estimate fails with `Error::Template { message: "notes do not shrink below the context budget" }` (the caller fills in `name`)
  - [ ] the first error from `complete` stops scheduling new calls and is returned after the in-flight calls end
  - [ ] `on_event` receives `Chunk { done, total }` after each map call and `Consolidating` before the reduce call, always from the calling thread
  - [ ] `Generated.calls` counts every call made
- Tests first: `blocks_span_30_seconds`, `block_timestamp_format_over_one_hour`, `empty_transcript_empty_text`, `estimate_rounds_up`, `language_instruction_last`, `fits_makes_one_call`, `chunks_never_exceed_budget` (property), `overlong_line_split_at_whitespace`, `chunk_order_kept`, `concurrency_limit_respected`, `reduce_joins_parts_in_order`, `multi_round_reduce`, `non_shrinking_notes_fail`, `first_error_stops_scheduling`, `events_in_order`
- Notes: `std::thread::scope` with a shared work index. No async runtime. Keep the map and reduce wrapper wording in constants near `PROMPT_VERSION`; changing them requires bumping it.

### T9: Job state for template steps
- Priority: 1
- Depends on: T1
- Owns: `src/job/**`, `tests/job.rs`
- Touches (read-only or additive): none
- Acceptance:
  - [ ] `job.json` without `templates` loads (sub-project 1 fixture) and saves it back with an empty map; version stays 1
  - [ ] `template_reason` returns `NotRun` when unrecorded, `Unfinished` when stale, `Forced` when forced, `FingerprintChanged` on mismatch, `MissingArtifact` when the artifact is gone, and `None` when done and current (one test each)
  - [ ] `mark_template_done` fails with `Error::Job` recording nothing when an artifact is missing, and stores artifact paths relative to the job dir; `mark_template_stale` records the reason text
  - [ ] pipeline `steps` handling is unchanged: every existing test in `tests/job.rs` passes unmodified
- Tests first: `old_job_json_loads_without_templates`, `template_not_run`, `template_unfinished`, `template_forced`, `template_fingerprint_changed`, `template_missing_artifact`, `template_current_is_none`, `mark_template_done_requires_artifacts`, `mark_template_stale_records_reason`
- Notes: reuse `own_reason`'s logic by having it work on `Option<&StepState>`; this is the only refactor allowed in `stale.rs`. Use `try_exists` for the artifact check in new code.

### T10: Template step
- Priority: 0
- Depends on: T3, T7, T8, T9
- Owns: `src/generate.rs`, `tests/generate.rs`
- Touches (read-only or additive): `src/template/**`, `src/llm/**`, `src/job/**`, `src/lang.rs`, `src/transcript.rs`
- Acceptance:
  - [ ] `output_lang` precedence flag > template > config > detected, one test per layer; an unknown detected code with no other source is a Usage error suggesting `--output-lang`; every chosen code is validated with `lang::validate`
  - [ ] `backend_id` precedence flag > template > default, one test per layer
  - [ ] `fingerprint` covers `"template"`, the name, `raw`, `PROMPT_VERSION`, `backend.id()`, `context_tokens()`, the output language code and the SHA-256 of the input artifact bytes (`transcript.json` or `<input>.md`); changing any one changes it (one assertion per part); a missing input artifact is an error naming it
  - [ ] `execute` builds `Vars` (output and speech language names, job name, `duration` as `h:mm:ss` from the last segment end), renders the body, feeds `transcript_text` (or the upstream Markdown) to `chunk::generate` with `call` as `complete` (refresh = `forced`), and writes `<name>.md` with `write_atomic`
  - [ ] a transcript with zero segments fails with `Error::Template { message: "transcript is empty" }` and makes no call
  - [ ] `Error::Template` from `generate` carries the template name
  - [ ] with a `ScriptedBackend`, a second `execute` with an unchanged fingerprint inputs reads every call from the cache (backend receives no calls)
- Tests first: `output_lang_flag_wins`, `output_lang_template_beats_config`, `output_lang_config_beats_detected`, `output_lang_detected_default`, `output_lang_unknown_detected_is_usage`, `backend_flag_wins`, `backend_template_beats_default`, `backend_default`, `fingerprint_sensitive_to_each_part` (table), `fingerprint_missing_input_is_error`, `execute_writes_markdown`, `execute_renders_variables`, `empty_transcript_fails_without_calls`, `second_execute_hits_cache`, `forced_execute_refreshes_cache`
- Notes: this is the heart of ADR-0005. The fingerprint is computed only after the input exists. Never precompute dependents' fingerprints.

### T11: Runner, CLI, progress, preflight and doctor
- Priority: 0
- Depends on: T2, T4, T5, T6, T10
- Owns: `src/runner.rs`, `src/cli.rs`, `src/progress.rs`, `src/main.rs`, `tests/cli.rs`
- Touches (read-only or additive): every module above (read-only)
- Acceptance:
  - [ ] `run` accepts repeatable `-t/--template`, `--llm ID` and `--output-lang CODE`; `transcribe` accepts none of them (usage error exit 2)
  - [ ] templates are discovered, each `-t` resolved, the graph ordered, backend ids and languages chosen (flag-provided ones validated) before preflight; any problem exits 2 with no job folder created
  - [ ] preflight adds one check per distinct backend in the run (`ollama::check`, `claude::check`) to the tool check and reports all missing items in one `Error::MissingTools`; `run` without `-t` checks no backend
  - [ ] after the pipeline, templates run in graph order: for each, compute the fingerprint, get `template_reason`, report `cached` or run it, then `mark_template_stale`/`mark_template_done`; the first failure stops the run with exit 1
  - [ ] `--force` with no value forces all steps and templates in the run; `--force <step>` also forces every template in the run; `--force <template>` forces it and its `dependents`, no pipeline step; an unknown value is exit 2
  - [ ] for each template whose backend `sends_off_machine`, one note `<name> sends its input to claude (Anthropic)` is printed per run
  - [ ] Ctrl-C during a template exits 130, writes no `<name>.md`, leaves the template not done
  - [ ] progress: total = pipeline steps + templates in the run; template lines show `<backend id>  <n> call(s)  <time>`; the spinner shows `chunk i/n` or `consolidating` with elapsed time; step labels are strings (the `StepName`-specific API is replaced)
  - [ ] `templates list` prints `NAME LAYER INPUT LLM DESCRIPTION` rows, then invalid entries with path and error; exit 0
  - [ ] `templates show <name>` prints the path (or `built-in`) to stderr and the content to stdout; unknown name exit 2
  - [ ] `doctor` prints `llm  <default id> (ok | not reachable: … | model missing: …)` and its exit code ignores that line
  - [ ] concurrency: `llm.concurrency` or 1 for Ollama and 3 for Claude
  - [ ] every existing test in `tests/cli.rs` and `tests/integration/` passes unmodified (only additions)
- Tests first: `run_accepts_repeated_templates`, `transcribe_rejects_template_flag`, `force_accepts_template_name`, `force_unknown_is_usage`, `unknown_template_exits_2_before_job_folder`, `templates_list_shows_layers`, `templates_show_unknown_exits_2`, `progress_template_line_format`, `progress_total_includes_templates`, `off_machine_note_printed_once`
- Notes: keep `Pipeline` for the five steps. Add a separate template loop rather than generalizing `Runnable`. Update the spec's Implementation status in this PR.

### T12: Integration suite and opt-in LLM e2e
- Priority: 1
- Depends on: T11
- Owns: `tests/integration/templates.rs`, `tests/integration/main.rs` (one `mod` line), `tests/e2e_llm.rs`
- Touches (read-only or additive): `tests/integration/common.rs` (additive helpers only), `tests/support/fake_ollama.rs`, `tests/fixtures/bin/claude`
- Acceptance (each runs the real binary with the fake whisper, fake Ollama or fake claude, a temp HOME and an explicit `MINUTA_CONFIG`, with backoff 0):
  - [ ] `run -t summary` writes `summary.md` with the fake reply and records `templates.summary` done; a second run makes zero chat requests and prints `cached`
  - [ ] editing the project template reruns only it (and its dependent); others stay cached
  - [ ] chain `b: input a`: forcing `a` with a different fake reply reruns `b`; with an identical reply `b` stays cached
  - [ ] `--force summary` makes new chat requests despite the cache
  - [ ] with a small `ollama.num_ctx`, the fake fails the 3rd chunk once, so the run exits 1; the next run requests only the unfinished chunks plus the reduce (counted)
  - [ ] Ctrl-C (SIGINT to the child) while the fake hangs a chat exits 130, and no `summary.md` exists
  - [ ] 500 then 200 succeeds; 404 exits 1 with `ollama pull` in stderr
  - [ ] the recorded system prompt contains "in Spanish" for the `es` sample transcript and "in English" with `--output-lang en`; `num_ctx` is sent
  - [ ] an unreachable Ollama plus a missing whisper are reported in one message, exit 1, no job folder
  - [ ] `run` without `-t` makes no request to the fake (and works with no Ollama at all)
  - [ ] a template with `llm: claude` runs the fake claude: input on stdin, off-machine note in stderr
  - [ ] a cycle between two project templates exits 2 naming the cycle, no job folder
  - [ ] `templates list` and `show` reflect built-in, user (temp HOME) and project (temp cwd) layers
  - [ ] `MINUTA_E2E_LLM=1` runs `-t summary` against the real local Ollama on the e2e clip; skipped otherwise
- Tests first: the cases above, named `templates_<case>`
- Notes: the integration suite must not rely on test-thread ordering, and it asserts per stream (conventions). The fake server binds `127.0.0.1:0`.

## Coverage check
| Spec requirement | Task(s) |
|---|---|
| Goal 1: `-t summary` writes `summary.md`, records state | T3 (built-in), T10, T11, T12 |
| Goal 2: layer precedence | T3, T12 |
| Goal 3: chaining, content-based rerun, cycle error | T4, T10, T11, T12 |
| Goal 4: second run makes zero requests | T7, T9, T10, T12 |
| Goal 5: change reruns only affected templates | T10, T12 |
| Goal 6: chunking within budget, consolidation | T8 |
| Goal 7: resume repeats only unfinished calls | T7, T12 |
| Goal 8: retries with backoff | T5, T6, T7 |
| Goal 9: explicit language instruction, `--output-lang` | T1 (table), T8, T10, T11, T12 |
| Goal 10: combined preflight with hints | T5, T6, T11, T12 |
| Goal 11: `templates list` / `show` | T3, T11, T12 |
| Goal 12: no real network in tests | T5 (fake server), T6 (fake claude), T12 |
| Config keys and precedence | T2 |
| Backend ids, trait | T1 |
| Ollama specifics (num_ctx, think, length) | T5 |
| claude sandboxing (cwd, tools, stdin, argv) | T6 |
| Off-machine note | T11, T12 |
| `job.json` `templates` map, version 1 | T9 |
| Call cache, forced bypass | T7, T10, T11 |
| Output cleanup | T7 |
| Interrupt during templates | T5, T6, T7, T11, T12 |
| Empty transcript | T10 |
| `doctor` llm line | T11 |
| `--force` with step or template | T4 (`dependents`), T11 |
| Opt-in e2e | T12 |
