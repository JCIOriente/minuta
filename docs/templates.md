# Templates

**English** | [Español](templates.es.md)

A template is a prompt that turns the transcript, or another template's output,
into a document. You choose which templates to run with `-t`. Templates they
depend on run first.

## Built-in templates

| Name | Input | Writes |
| --- | --- | --- |
| `summary` | transcript | `summary.md`: overview, key points, decisions, action items, open questions |
| `chapters` | transcript | `chapters.md` and `assets/chapters.json`: timestamped chapters that follow YouTube's rules |
| `youtube` | transcript | `youtube.md` and `assets/youtube.json`: title, description, chapters, hashtags, tags and thumbnail text |
| `linkedin` | transcript | `linkedin.md` and `assets/linkedin.json`: post body and hashtags |
| `instagram` | transcript | `instagram.md` and `assets/instagram.json`: caption and hashtags |

For example, `-t youtube` also runs `chapters`, because `youtube` fills its
chapters field from it. A template that runs only because another one needs it
writes its files in `assets/`, so `chapters.md` shows up at the top only when you
ask for `-t chapters`. `summary` runs only on `-t summary`.

## Where templates come from

minuta loads templates from three places. A template with the same name in a
later place replaces the earlier one:

1. Built in to minuta
2. Your user templates: `~/.minuta/templates/*.md`
3. Project templates: `./templates/*.md` in the current directory

To customize a template, copy it and edit the copy. Write the copy in the current
folder first and then move it: redirecting straight into `~/.minuta/templates` creates an empty
template that replaces the one you are reading.

```sh
minuta templates show summary > summary.md
mkdir -p ~/.minuta/templates
mv summary.md ~/.minuta/templates/
```

> **Security:** templates in `./templates` are read from whatever directory you
> run minuta in. Review the templates of a repository you cloned before running
> minuta in it, the same as you would review its scripts.

## Downloading templates

Curated templates are plain files in the repository's
[`templates`](https://github.com/JCIOriente/minuta/tree/main/templates) folder,
listed in `templates/index.toml` (the catalog). `minuta templates pull` downloads
the ones you choose into `./templates`:

```console
$ minuta templates pull --list
templates from https://raw.githubusercontent.com/JCIOriente/minuta/main/templates/
NAME       PLATFORM   STATE             DESCRIPTION
instagram  instagram  new               Caption with a hook in the first 125 characters
youtube    youtube    update available  Title, description, tags and thumbnail text
linkedin   linkedin   modified          Post with a hook and a closing question

$ minuta templates pull instagram youtube
minuta will download from:
  https://raw.githubusercontent.com/JCIOriente/minuta/main/templates/
  instagram.md  (new)
  youtube.md    (update available)

into .../templates

Continue? [y/N] y
instagram.md  written
youtube.md    updated
```

The names above are examples. `--list` shows what the catalog has now, fetched
fresh each time, so a new template shows up without a new minuta release. A
pulled file is an ordinary project template: `minuta templates list` shows it
with the layer `project`, and it replaces a user or built-in template of the same
name. The built-in templates stay in the binary, so offline use does not change.

| Option | What it does |
| --- | --- |
| `NAME...` | Download these templates |
| `--all` | Download every template in the catalog |
| `--filter TEXT` | Download the templates whose name, platform or tag contains `TEXT` |
| `--list` | Show the catalog with the state of each template and download nothing |
| `--to DIR` | Put the files in `DIR` instead of `./templates`. `--to ~/.minuta/templates` makes them available in every folder |
| `--force` | Replace templates you edited. The old file is saved as `NAME.md.bak` first |
| `--yes` | Do not ask for confirmation |
| `--ref REF` | Branch, tag or commit to download from (default `main`) |

With nothing chosen, on a terminal, `pull` opens a numbered list to pick from.
Before downloading it shows the source URL, each file with its state and the
destination, and asks `Continue? [y/N]`. A progress bar on stderr counts the
files; it is not drawn when output is piped or `NO_COLOR` is set. Without a
terminal, name the templates (or use `--all` or `--filter`) and pass `--yes`;
otherwise it refuses before downloading anything. The question and messages go to
stderr, and stdout carries only the paths written.

Each template in the destination folder is in one of four states:

| State | Meaning | `pull` does |
| --- | --- | --- |
| new | The file is not there | Downloads it |
| up to date | The file is the same as the repository's | Nothing |
| update available | The file is as minuta wrote it and the repository has a newer version | Updates it |
| modified | You edited the file, or minuta did not write it | Skips it. With `--force`, saves it as `NAME.md.bak`, then replaces it |

minuta remembers what it wrote in `.pulled.toml` in the destination folder: the
hash of each pulled file. That is how it tells your edits from a newer upstream
version. If you delete `.pulled.toml`, or it is invalid, a file that differs
from the catalog reads as modified and nothing is overwritten. To remove a template, delete its file.

Before writing anything, minuta checks that every file matches the SHA-256 in
the catalog and is a valid template, and writes none of them if one is not.
Downloads use HTTPS only, and a symlinked destination, `.pulled.toml` or template
is refused. The hash comes from the same repository version as the file, so it
catches corrupted downloads, not a malicious change to the repository. To get
fixed content, pin a tag or commit with `--ref v0.3.0`. If a pulled template reads
another template that you do not have, minuta prints a `note:` naming it.

## LLM backends

| ID | Backend |
| --- | --- |
| `ollama:<model>` | A model served by your local Ollama, such as `ollama:gemma4:26b` |
| `claude` | Claude Code (`claude -p`) with its default model |
| `claude:<model>` | Claude Code with a given model, such as `claude:sonnet` |
| `codex` | Codex (`codex exec`) with its default model |
| `codex:<model>` | Codex with a given model, one your Codex account supports |
| `<endpoint>:<model>` | A model on an [OpenAI-compatible server](#openai-compatible-servers) you named in the config, such as `lmstudio:google/gemma-4-26b-a4b` |

The backend is chosen in this order: the `--llm` flag, then the template's `llm`
key, then `llm.default`, set in the config file or with `MINUTA_LLM` (default:
`ollama:gemma4:26b`). Unlike the flag, `MINUTA_LLM` does not override a template's
`llm` key.

**Ollama** keeps the transcript on your machine, unless `ollama.url` points to
another machine.

**Claude.** minuta runs `claude -p` using your existing Claude Code login. It
runs Claude without tools, sessions or MCP servers, from an empty temporary
directory. Claude sends the transcript to Anthropic, and minuta prints a notice
whenever a template uses it.

**Codex.** minuta runs `codex exec` using your existing Codex login (run
`codex login` once). It runs Codex with a read-only sandbox, no session files and
none of your `~/.codex/config.toml` settings, from an empty temporary directory,
so plain `codex` uses Codex's built-in default model. Codex sends the transcript
to OpenAI, and minuta prints a notice whenever a template uses it.

Long transcripts are split to fit the model's context window and then combined.
For Ollama, the window is the smaller of `ollama.num_ctx` and the model's own
limit. For an endpoint, it is `context_tokens`.

For setup steps, see [the installation guide](install.md#3-choose-how-to-generate-documents).

## OpenAI-compatible servers

minuta can use any server with an OpenAI-compatible API, such as
[LM Studio](https://lmstudio.ai), [Colibri](https://github.com/JustVugg/colibri),
llama.cpp or vLLM, or a cloud service. Name the server once in the config file
(`minuta config path` shows where it is):

```toml
[endpoints.lmstudio]
url = "http://127.0.0.1:1234/v1"
```

Then use `<endpoint>:<model>` anywhere a backend goes:

```sh
minuta run meeting.mp4 -t summary --llm lmstudio:google/gemma-4-26b-a4b
```

The same id works in a template's `llm` key, in `llm.default` and in `MINUTA_LLM`.
To check the server and the model, set it as the default and run `minuta doctor`.
If the model is wrong, `doctor` lists what the server offers.

| Key under `[endpoints.<name>]` | Required | Default | Meaning |
| --- | --- | --- | --- |
| `url` | yes | | Base URL as the server documents it, including `/v1` |
| `context_tokens` | no | `16384` | Prompt and reply budget per call. It must fit the context the server gives the model |
| `timeout_secs` | no | `600` | Seconds to wait for one reply. Raise it for slow models |
| `api_key_env` | no | | The name of the environment variable holding the API key |

`<name>` starts with a lowercase letter and uses lowercase letters, digits, `-`
and `_`. It can be anything except `ollama`, `claude` or `codex`. Declare as many
endpoints as you like.

**API keys** never go in the config file. Point `api_key_env` at a variable, and
minuta sends its value as `Authorization: Bearer`:

```toml
[endpoints.openrouter]
url = "https://openrouter.ai/api/v1"
api_key_env = "OPENROUTER_API_KEY"
```

**Privacy.** A URL on `localhost`, `127.0.0.0/8` or `::1` keeps the transcript on
your machine. With any other URL, a LAN address included, minuta prints a notice
such as `summary sends its input to openrouter (openrouter.ai)`.

Endpoints make one call at a time unless you set `llm.concurrency`. minuta asks
for structured output with `response_format` and, if the server refuses it, sends
the request again without it. See [setup for LM Studio and
Colibri](install.md#openai-compatible-servers).

## Writing a template

Create a `templates` folder in the directory where you run minuta, and save the
example below as `templates/weekly-sync.md`. The file name is the template's name,
so this one is `weekly-sync`. Names start with a lowercase letter or digit and use only lowercase letters,
digits, `-` and `_`. The names `transcript`, `probe`, `join`, `audio`,
`transcribe`, `outputs` and `job` are reserved. The file is a Markdown prompt with
an optional YAML header:

```markdown
---
description: Notes for our weekly sync
input: transcript
---
These are the notes of "{{job_name}}" ({{duration}}).

List what each person reported, then the blockers and who owns each one.
Use only what is said in the transcript.
```

```sh
minuta run sync.mp4 -t weekly-sync
```

minuta tells the model which language to write in, so the prompt itself can be in
any language.

### Header keys

The header is optional, and so is every key, except that `output: fields` needs
`fields`. Unknown keys are an error.

| Key | Values | Default |
| --- | --- | --- |
| `description` | Free text, shown by `templates list` | none |
| `input` | `transcript`, or the name of another template whose output this one reads | `transcript` |
| `output` | `markdown`, or `fields` for structured output (see below) | `markdown` |
| `fields` | With `output: fields`, the fields to generate | none |
| `limits` | With `output: fields`, the limits for each field: `{ max_chars: N, max: N }` | none |
| `format` | With `output: fields`, how fields render in the `.md` file: `plain` or `markdown` | `plain` |
| `llm` | The backend for this template, such as `ollama:qwen3:14b`, `claude` or `codex` | the configured default |
| `lang` | The language this template writes in, such as `en`. `--output-lang` overrides it | `llm.output_lang`, else the detected speech language |

The `chapters` template is special: it uses `output: chapters`, which no other
template may use. If you override `chapters`, keep `output: chapters`. Its only
allowed limit is `limits: { chapters: { max: N, max_chars: N } }`, with `max` at
least 3.

### Variables

| Variable | Value |
| --- | --- |
| `{{job_name}}` | The job name |
| `{{duration}}` | The end time of the last transcript segment, as `h:mm:ss` |
| `{{output_lang}}` | The output language's name, such as `English` |
| `{{output_lang_code}}` | The output language's code, such as `en` |
| `{{speech_lang}}` | The detected speech language's name, such as `Spanish` |

Any other `{{...}}` is an error that lists these.

### Structured output (`fields`)

A `fields` template writes `assets/<name>.json` for scripts and `<name>.md` for
reading. Fields come from a fixed set:

- Text: `title`, `body`, `thumbnail_text`, `notes` (for the person publishing, not
  part of the published text)
- Lists: `tags`, `hashtags`
- `chapters`: filled in from the `chapters` template's result, not written by the
  model

`max_chars` limits the length of a field as rendered. `max` limits the number of
items in a list. Local models often ignore length instructions, so when a field
breaks a limit, minuta asks the model to repair it, up to `llm.repairs` times
(default 2). If the field still breaks the limit, the template fails.

```markdown
---
description: Short post for our team channel
input: transcript
output: fields
fields: [title, body, hashtags]
limits:
  title: { max_chars: 80 }
  body: { max_chars: 600 }
  hashtags: { max: 3 }
---
Write a short post about "{{job_name}}" from the transcript in the input.
```

## Contributing a template

A template is added to the repository, not to minuta's code, so it needs no
release:

1. Write `templates/NAME.md` in the [format above](#writing-a-template). The
   briefs in [`docs/template-briefs`](template-briefs) show how to describe what
   a good template for a platform looks like.
2. Add an entry to `templates/index.toml` with its `name`, `description` and
   `sha256`, and optionally `platform`, `tags` and `input`.
3. Get the `sha256` with `shasum -a 256 templates/NAME.md`. The consistency test
   `tests/templates_catalog.rs` prints this command when a hash is wrong.
4. Run `make ci`. The test fails if an entry has no file, a hash differs, a file
   is not a valid template, a name differs from its file name, or a
   `templates/*.md` file has no entry.

Change the hash again whenever you edit the file.
