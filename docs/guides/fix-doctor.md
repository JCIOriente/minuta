# Fix what `minuta doctor` reports

**English** | [Español](fix-doctor.es.md)

**Goal:** make `minuta doctor` report no problems, by fixing each missing tool
one at a time.

`minuta doctor` checks the programs and files minuta needs and prints one line
for each. It is the first command to run when something does not work.

## Before you start

- minuta is installed: `minuta --version` prints a version number. If it says
  `command not found`, go back to the [installation guide](../install.md).
- A terminal.

## 1. Run doctor and read the result

```sh
minuta doctor
```

When everything is fine, every line shows a version or a path, and the command
ends without an error message. This is an example; your versions and paths will
differ:

```console
ffmpeg       ffmpeg version 9.0.2 Copyright (c) 2000-2026 the FFmpeg developers (/opt/homebrew/bin/ffmpeg)
ffprobe      ffprobe version 9.0.2 Copyright (c) 2007-2026 the FFmpeg developers (/opt/homebrew/bin/ffprobe)
whisper-cli  whisper.cpp version: 1.9.5 (/opt/homebrew/bin/whisper-cli)
model        .../.minuta/models/ggml-small.bin
llm          ollama:gemma4:26b (ok)
```

If anything is missing, the line says `missing` and `doctor` adds a list at the
end that says how to fix each item:

```console
minuta: missing required tools: model
  model: run `minuta models pull base` or set whisper.model
```

What each line is:

| Line | What it is | Needed for |
| --- | --- | --- |
| `ffmpeg` | Reads and converts recordings | Always |
| `ffprobe` | Reads the length and format of a recording (comes with ffmpeg) | Always |
| `whisper-cli` | Writes the transcript (part of whisper.cpp) | Always |
| `model` | The whisper model file | Always |
| `llm` | The language model that writes documents | Only when you use `-t` |

If a tool or the model is missing, `doctor` exits with status 1. A problem with
the `llm` line is shown but does not change the exit status, so check that line
yourself.

If `doctor` prints `config error: ...` instead of the table, the config file
has a mistake. Run `minuta config path`, open the file it prints, and fix the
line the message names. A common cause is a section, such as `[whisper]`,
written twice: keep one and move its lines together.

Fix the problems one at a time, from the top. After each fix, run
`minuta doctor` again. Find your problem below.

## 2. `ffmpeg` or `ffprobe` is missing

Both come in the same package.

1. Install it with the command that `doctor` prints next to the name. On macOS
   and Linux, with Homebrew, it is:

   ```sh
   brew install ffmpeg
   ```

   On Windows, or without Homebrew, follow the
   [installation guide](../install.md#1-install-minuta-and-its-tools).

2. Run `minuta doctor` again. The `ffmpeg` and `ffprobe` lines now show a
   version.

**If it did not work.**

- `brew: command not found`: install [Homebrew](https://brew.sh) first, as the
  installation guide explains, then open a new terminal.
- The tool is installed but `doctor` still says it is missing:
  1. Open a new terminal and run `minuta doctor` again. A terminal opened
     before the installation may not see the new program.
  2. Run `minuta config show` and look at the `ffmpeg.bin` line. If it ends in
     `(file)` or `(env)`, minuta uses that value instead of searching for the
     program. Correct it, or remove it.
  3. Otherwise the program is in a folder your terminal does not search. Find
     its full path. Homebrew installs programs in `/opt/homebrew/bin` on Apple
     silicon Macs, `/usr/local/bin` on Intel Macs and
     `/home/linuxbrew/.linuxbrew/bin` on Linux. Then set that path in the config
     file (`minuta config path` shows where it is; `minuta config init` creates
     it). Use `[ffprobe]` for ffprobe in the same way:

     ```toml
     [ffmpeg]
     bin = "/opt/homebrew/bin/ffmpeg"
     ```

  When the path is wrong, the error reads `configured path ... does not exist
  or is not executable`, so you can correct it.
- ``found at ... but `-version` failed``: the program is there but does not run.
  Reinstall it.

## 3. `whisper-cli` is missing

1. Install whisper.cpp, which includes `whisper-cli`, with the command that
   `doctor` prints. With Homebrew it is:

   ```sh
   brew install whisper.cpp
   ```

   Without Homebrew, follow the
   [installation guide](../install.md#1-install-minuta-and-its-tools).

2. Run `minuta doctor` again. The `whisper-cli` line shows a version.

**If it did not work.** Follow the same checks as in section 2, with the
`whisper.bin` line and the `[whisper]` section instead.

## 4. `model` is missing

The whisper model is a file you download once.

1. Download it:

   ```sh
   minuta models pull base
   ```

   ```console
   pulling base (141.1 MiB) into .../.minuta/models
   base downloaded and verified
   .../.minuta/models/ggml-base.bin
   ```

   In an interactive terminal, the download shows a progress bar instead.

2. Tell minuta to use it. Downloading is not enough: the config file must name
   the model. If you have no config file yet, run `minuta config init` to
   create one. Then open the file that `minuta config path` prints, and under
   `[whisper]` set the `model` line (delete the `# ` at its start):

   ```toml
   [whisper]
   model = "base"
   ```

   Change the line that is already there. Do not add a second `[whisper]`
   section.

3. Run `minuta doctor` again. The `model` line shows a path.

**If it did not work.** Read the second line of the error:

| Message | Cause and fix |
| --- | --- |
| ``run `minuta models pull base` or set whisper.model`` | No model is set. Do step 2 |
| ``run `minuta models pull small` `` | The config names a model you have not downloaded. Run that command |
| `configured path /some/file.bin is not a file` | `model` is a path and nothing is there. Fix the path, or use a name from `minuta models list` |
| `... is a VAD model; set it as whisper.vad_model` | The voice-detection model is in `model`. Move it to `vad_model` |
| ``vad model: run `minuta models pull silero-v5.1.2` `` | `vad_model` is set but not downloaded. Run that command, or remove the line |

To see where each model is kept, run `minuta models list`. The first line shows
the folder.

## 5. `llm` is not `ok`

The `llm` line shows the id of the default language model, and its state in
brackets. The state depends on which model it is. Find your id in the first
column.

| Id starts with | State in `doctor` | What to do |
| --- | --- | --- |
| `ollama:` | `not reachable: start Ollama ...` | Start Ollama: `ollama serve`. If Ollama is on another computer, set `url` under `[ollama]` |
| `ollama:` | `model missing: ollama pull <model>` | Run the command it prints |
| `claude` | `not reachable: install Claude Code: ...` | Install [Claude Code](../install.md#claude-code), then run `claude` once to log in |
| `codex` | `not reachable: install Codex: ...` | Install [Codex](../install.md#codex), then run `codex login` |
| a name you chose, such as `lmstudio:` | `invalid: unknown endpoint ...` | Add the server to the config file. The message shows the lines to add |
| the same | `not reachable: start the server or fix endpoints.<name>.url ...` | Start the server, or correct its `url` |
| the same | `not reachable: set <VARIABLE> to the API key ...` | Set that environment variable to your key, in the same terminal |
| the same | `model missing: not offered by the server; it offers: ...` | Use one of the model names in that list |
| anything else | `invalid: invalid LLM backend ...` | The id is not written correctly. The message lists the valid forms |

For `claude` and `codex`, `(ok)` only means the program starts. `doctor` does
not check that you are logged in. To test them end to end, generate a short
summary, such as `minuta run meeting.mp4 -t summary --llm claude` with your own
recording. If it fails with a login error, run `claude` (or `codex login`)
again.

If you do not use `-t`, you can ignore this line. The full steps to set up each
LLM are in [Set up an LLM and a whisper model](llm-and-whisper.md).

To check a different LLM without changing the config file, set it for one
command. Replace `claude` with the id you want to check:

```sh
MINUTA_LLM=claude minuta doctor
```

## 6. Check that you are done

```sh
minuta doctor
echo $?
```

`doctor` prints no `missing` and the number printed by `echo` is `0`. If you
want to generate documents, the `llm` line also ends in `(ok)`.

If a problem is still there, run `minuta config show`. It lists every setting,
its value and where the value came from, such as `(file)`, `(env)` or
`(default)`. A value that is not the one you expect usually explains it.

## Next

- Try a real recording: [From a recording to meeting minutes](worked-example.md).
- More errors and fixes: [Troubleshooting](../reference.md#troubleshooting).
