# minuta

**English** | [Español](https://github.com/JCIOriente/minuta/blob/main/README.es.md)

Turn recordings of meetings, classes and webinars into transcripts and written
documents, such as a summary, timestamped chapters, YouTube copy or a LinkedIn
post.

```console
$ minuta run standup.mp4 -t summary -t youtube
.../minuta-out/standup

$ ls minuta-out/standup
assets  captions.srt  summary.md  transcript.txt  youtube.md
```

- **Local by default.** Transcription always runs on your machine with
  [whisper.cpp](https://github.com/ggml-org/whisper.cpp). Documents are written
  by a local [Ollama](https://ollama.com) model unless you choose
  [Claude Code](https://claude.com/claude-code),
  [Codex](https://github.com/openai/codex) or another server.
- **Templates are plain Markdown files.** Write your own, or chain them so one
  template reads another's output.
- **Resumable.** Every step is cached. If you interrupt a run or change a
  template, the next run redoes only what changed.

## Install

minuta runs on macOS, Linux and Windows (through WSL2).

**[Follow the installation guide](https://github.com/JCIOriente/minuta/blob/main/docs/install.md)**
for your system. It covers Rust, ffmpeg, whisper.cpp and an LLM backend.

If you already have Rust 1.88 or newer:

```sh
cargo install --locked minuta
```

Then check your setup. `doctor` tells you what is missing and how to fix it:

```sh
minuta doctor
```

## Quick start

Install minuta and its tools first. Replace `meeting.mp4` with the path to your
recording, and put quotes around a path with spaces.

1. **Download a whisper model and make it the default.** `base` is a good first
   choice. Skip this step if you did it in the installation guide; otherwise follow
   [Download a whisper model](https://github.com/JCIOriente/minuta/blob/main/docs/install.md#2-download-a-whisper-model).

2. **Transcribe a recording.**

   ```sh
   minuta transcribe meeting.mp4
   ```

   You get `transcript.txt` and `captions.srt` in `minuta-out/meeting/`.

3. **Generate a document.** This uses your default backend, Ollama with
   `gemma4:26b` unless you changed it. For another backend, add its id, for
   example `--llm claude`.


   ```sh
   minuta run meeting.mp4 -t summary
   ```

   The transcript is already cached, so only the summary runs. The result is
   `minuta-out/meeting/summary.md`.

## More examples

```sh
# Join two parts of one class, in order, into a job named "class-4"
minuta run part1.mp4 part2.mp4 --name class-4 -t summary

# YouTube title, description, tags and chapters, written with Claude
minuta run webinar.mov -t youtube --llm claude

# A Spanish meeting, with the notes written in English
minuta run reunion.m4a --lang es --output-lang en -t summary

# The job folder's path goes to stdout, so you can use it in scripts
cat "$(minuta run talk.mp4 -t linkedin -q)/linkedin.md"
```

The built-in templates are `summary`, `chapters`, `youtube`, `linkedin` and
`instagram`.

## Documentation

| Guide | What it covers |
| --- | --- |
| [Installation](https://github.com/JCIOriente/minuta/blob/main/docs/install.md) | macOS, Linux and Windows setup, models and LLM backends |
| [Templates](https://github.com/JCIOriente/minuta/blob/main/docs/templates.md) | Built-in templates, downloading more, writing your own, choosing a backend |
| [Reference](https://github.com/JCIOriente/minuta/blob/main/docs/reference.md) | Commands, the job folder, caching, configuration and troubleshooting |

Run `minuta help <command>` for the full usage of any command.

## Development

```sh
git clone https://github.com/JCIOriente/minuta.git
cd minuta
make ci        # rustfmt, clippy and tests, the same checks CI runs
```

The design is in
[`docs/specs`](https://github.com/JCIOriente/minuta/tree/main/docs/specs), and
the decisions behind it are in
[`docs/adr`](https://github.com/JCIOriente/minuta/tree/main/docs/adr).

## License

minuta is licensed under the
[Apache License, Version 2.0](https://github.com/JCIOriente/minuta/blob/main/LICENSE).
