# Installing minuta

**English** | [Español](install.es.md)

Choose your system and follow its steps from top to bottom. Then
[download a model](#2-download-a-whisper-model) and
[choose how to generate documents](#3-choose-how-to-generate-documents).

| You need | For | Required? |
| --- | --- | --- |
| Node.js 22 or newer, or Rust 1.88 or newer | Installing minuta (npm needs no Rust) | Always |
| `ffmpeg` and `ffprobe` | Reading, joining and converting media | Always |
| `whisper-cli` from whisper.cpp | Transcription | Always |
| A whisper model | Transcription | Always |
| Ollama, Claude Code, Codex or an OpenAI-compatible server | Generating documents | Only for `minuta run -t ...` |

## 1. Install minuta and its tools

Each step needs the one before it, so follow your system's steps from top to
bottom. If a later command says `brew: command not found` or
`cargo: command not found`, close the terminal, open a new one and run it again.

In every system, `--locked` builds minuta with the dependency versions it was
tested with.

**With Node.js 22 or newer you can skip Rust.** Install ffmpeg and whisper.cpp with
your system's steps, skip the Rust and `cargo install` steps, and run this instead:

```sh
npm i -g @jcioriente/minuta
```

This works on macOS (Apple Silicon and Intel) and Linux (x64 and arm64). On
Windows, run it inside WSL2 (see [Windows](#windows)): npm sees WSL2 as Linux. npm
installs only minuta, not ffmpeg, whisper.cpp or a model. Check it with
`minuta --version`, or try it without installing: `npx @jcioriente/minuta doctor`.

### macOS

1. Install the command line tools, which include the compiler Homebrew needs. A
   window asks you to confirm. If they are already installed, the command says so:

   ```sh
   xcode-select --install
   ```

2. Install [Homebrew](https://brew.sh) if you do not have it. Check it with
   `brew --version`.
3. Install ffmpeg and whisper.cpp. On Apple Silicon, this whisper build uses the
   GPU through Metal:

   ```sh
   brew install ffmpeg whisper.cpp
   ```

   Homebrew also installs the libraries whisper.cpp uses, so the download is
   larger than one program. On an Intel Mac there is no prebuilt package, so
   Homebrew compiles it, which takes several minutes.

4. Install Rust. Follow the prompts, then check it with `cargo --version`:

   ```sh
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   source "$HOME/.cargo/env"
   ```

5. Install minuta:

   ```sh
   cargo install --locked minuta
   ```

   Check it with `minuta --version`. If the build fails, run `rustup update stable`
   and try again.

### Linux

These commands are for Debian and Ubuntu. On another distribution, see
[other distributions](#other-distributions).

1. Install the tools that Homebrew and Rust need:

   ```sh
   sudo apt update
   sudo apt install build-essential curl git procps file
   ```

2. Install [Homebrew](https://brew.sh). When it finishes, it prints a **Next
   steps** section with commands that add `brew` to your `PATH`. Run them, then
   check it with `brew --version`:

   ```sh
   /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
   ```

3. Install ffmpeg and whisper.cpp:

   ```sh
   brew install ffmpeg whisper.cpp
   ```

   This downloads many packages and can take several minutes. If it stops with a
   download error, run the same command again: Homebrew continues where it
   stopped.

   To use a GPU, such as CUDA or Vulkan, build whisper.cpp yourself with the
   [whisper.cpp build options](https://github.com/ggml-org/whisper.cpp#readme)
   and set `whisper.bin` to your `whisper-cli`.

4. Install Rust. Follow the prompts, then check it with `cargo --version`:

   ```sh
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   source "$HOME/.cargo/env"
   ```

5. Install minuta:

   ```sh
   cargo install --locked minuta
   ```

   Check it with `minuta --version`. If the build fails, run `rustup update stable`
   and try again.

#### Other distributions

Install the tools that Homebrew and Rust need with your package manager, then
continue from step 2 above. ffmpeg comes from Homebrew, so you do not need
RPM Fusion on Fedora.

| System | Command |
| --- | --- |
| Fedora | `sudo dnf group install development-tools` and `sudo dnf install curl git procps-ng file libxcrypt-compat` |
| Arch | `sudo pacman -S base-devel curl git procps-ng file` |

### Windows

minuta does not run natively on Windows. It runs inside WSL2, a Linux system
built into Windows.

1. Open PowerShell as administrator, install WSL2 with Ubuntu, and restart if
   asked:

   ```powershell
   wsl --install
   ```

2. Open **Ubuntu** from the Start menu and create a user when asked.
3. Inside Ubuntu, follow the [Linux steps](#linux).

Run every minuta command inside Ubuntu. After you finish sections 2 to 4 of this
guide, you can reach your recordings under `/mnt`. Replace `<your-name>` with your
Windows user name:

```sh
cd /mnt/c/Users/<your-name>/Videos
minuta run meeting.mp4 -t summary
```

The job folder is created there too, so you can open it from Windows. Reading
from `/mnt/c` is slower than from the Linux file system. To process long
recordings faster, copy them to your Linux home folder first.

## 2. Download a whisper model

minuta downloads models, checks their checksums and saves them in
`~/.minuta/models/`:

```sh
minuta models list         # show the catalog and what is installed
minuta models pull base    # download one
```

| Model | Size | Good for |
| --- | --- | --- |
| `tiny` | 74 MiB | Quick tests |
| `base` | 141 MiB | A good first choice |
| `small` | 465 MiB | Better accuracy at a moderate speed |
| `medium` | 1.4 GiB | High accuracy |
| `large-v3` | 2.9 GiB | Best accuracy, slowest |

Then make it the default, so you do not have to pass `--model` on every run. Run `minuta config init` to create the config file and open the path it prints.
Under the `[whisper]` section that is already there, replace `# model = "small"`
with:

```toml
model = "base"
```

You can also use a whisper.cpp model you already have. Set `model` to the file's
path instead of a catalog name.

Every TOML snippet in this guide goes in that same file. A section can appear only once, so if it is already there, edit it instead of adding it again.

### Optional: voice activity detection

Without voice activity detection (VAD), whisper sometimes writes phrases that
were never said during long silences. A VAD model, under 1 MiB, reduces those phrases:

```sh
minuta models pull silero-v5.1.2
```

```toml
[whisper]
model = "base"
vad_model = "silero-v5.1.2"
```

## 3. Choose how to generate documents

Skip this step if you only need transcripts. You need one of these for
`minuta run -t ...`:

| Option | Your transcript goes to | Good if |
| --- | --- | --- |
| [Ollama](#ollama) | Your machine | You want everything local. It is the default |
| [Claude Code](#claude-code) | Anthropic | You already use Claude Code |
| [Codex](#codex) | OpenAI | You already use Codex |
| [An OpenAI-compatible server](#openai-compatible-servers) | Your machine, or the server you choose | You use LM Studio, Colibri or a cloud service |

### Ollama

[Ollama](https://ollama.com) runs language models on your machine.

| System | Command |
| --- | --- |
| macOS | `brew install ollama`, or download the app from [ollama.com/download](https://ollama.com/download) |
| Linux and WSL2 | `curl -fsSL https://ollama.com/install.sh \| sh` |

Start the server and download minuta's default model. The macOS app and the
Linux service start the server for you, so `ollama serve` may not be needed:

```sh
ollama serve              # leave this running
ollama pull gemma4:26b    # in another terminal
```

`gemma4:26b` needs a machine with plenty of memory. To use a smaller model from
[ollama.com/library](https://ollama.com/library), pull it and make it the
default in the config file:

```sh
ollama pull <model>
```

```toml
[llm]
default = "ollama:<model>"
```

If Ollama runs on another machine or port, set its URL. Transcripts are then
sent to that machine:

```toml
[ollama]
url = "http://192.168.1.20:11434"
```

### Claude Code

minuta uses your existing [Claude Code](https://claude.com/claude-code) login,
so there is no API key to configure. Install it with one of these:

```sh
curl -fsSL https://claude.ai/install.sh | bash     # native installer
brew install --cask claude-code                    # Homebrew
npm install -g @anthropic-ai/claude-code           # npm
```

Run `claude` once to log in, then use it from minuta:

```sh
minuta run meeting.mp4 -t summary --llm claude
```

To use Claude for every run, set `default = "claude"` under `[llm]`. The
transcript is sent to Anthropic, and minuta prints a notice each time.

### Codex

minuta uses your existing [Codex](https://github.com/openai/codex) login, so
there is no API key to configure. Install it with one of these:

```sh
npm install -g @openai/codex                       # npm
brew install --cask codex                          # Homebrew
```

Run `codex login` once, then use it from minuta:

```sh
minuta run meeting.mp4 -t summary --llm codex
minuta run meeting.mp4 -t summary --llm codex:<model>   # a given model
```

To use Codex for every run, set `default = "codex"` under `[llm]`. The
transcript is sent to OpenAI, and minuta prints a notice each time.

### OpenAI-compatible servers

Name the server in the config file, then use `<endpoint>:<model>` as the
backend, with `--llm` or as `default` under `[llm]`. `minuta config init` writes a
commented example. See
[every endpoint key](templates.md#openai-compatible-servers).

#### LM Studio

1. Install [LM Studio](https://lmstudio.ai) and download a model.
2. Start the server and load the model with a context of at least
   `context_tokens` (16384 by default):

   ```sh
   lms server start
   lms load google/gemma-4-26b-a4b --context-length 16384
   ```

3. Add the server to the config file:

   ```toml
   [endpoints.lmstudio]
   url = "http://127.0.0.1:1234/v1"
   ```

4. Use it. `lms ls` shows the model names:

   ```sh
   minuta run meeting.mp4 -t summary --llm lmstudio:google/gemma-4-26b-a4b
   ```

#### Colibri

1. Set up [Colibri](https://github.com/JustVugg/colibri) by following its README,
   and start it with `coli serve`.
2. Run `coli status` to get the model id, or look it up at
   `http://127.0.0.1:8000/v1/models`. It can differ from the name you started the
   model with: Qwen3.8-Flash-Next is `qwen3.8-flash-next-colibri`.
3. Add the server to the config file. Colibri can be much slower than the default
   10-minute limit, so raise `timeout_secs`:

   ```toml
   [endpoints.colibri]
   url = "http://127.0.0.1:8000/v1"
   timeout_secs = 3600
   ```

4. Use it:

   ```sh
   minuta run meeting.mp4 -t summary --llm colibri:qwen3.8-flash-next-colibri
   ```

minuta has been tested with Qwen3.8-Flash-Next on Colibri. A 5-minute transcript
took about 15 minutes per call on that setup, so expect long runs. Its `qwen38`
engine refuses `response_format`, so minuta sends those requests again without
it.

#### Another server

Set `url` to the base URL the server documents, usually ending in `/v1`. If it
needs a key, put the key in an environment variable and name the variable in
`api_key_env`:

```toml
[endpoints.openrouter]
url = "https://openrouter.ai/api/v1"
api_key_env = "OPENROUTER_API_KEY"
```

With a URL that is not on your machine, the transcript is sent to that server,
and minuta prints a notice each time.

## 4. Check your setup

```sh
minuta doctor
```

`doctor` prints each tool, the model and the state of the default LLM backend:

```console
$ minuta doctor
ffmpeg       ffmpeg version 8.0 ...
ffprobe      ffprobe version 8.0 ...
whisper-cli  whisper.cpp version: 1.8.0 ...
model        missing
llm          ollama:gemma4:26b (ok)
minuta: missing required tools: model
  model: run `minuta models pull base` or set whisper.model
```

If a tool or model is missing, it exits with status 1 and shows a fix for each
one. An unreachable LLM backend is shown but does not change the exit status.

If a tool is installed somewhere that is not on your `PATH`, point minuta to it
with `ffmpeg.bin`, `ffprobe.bin` or `whisper.bin`
([see Configuration](reference.md#configuration)).

Once `doctor` passes, go to the [quick start](../README.md#quick-start) and start
at step 2, since you already have a model.
