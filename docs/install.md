# Installing minuta's requirements

minuta runs on macOS and Linux. It does not bundle its tools. It calls these
programs, which you install once:

| Requirement | Needed for | Required? |
| --- | --- | --- |
| [ffmpeg and ffprobe](#ffmpeg-and-ffprobe) | Reading, joining and converting media | Always |
| [whisper.cpp](#whispercpp) (`whisper-cli`) | Transcription | Always |
| [A whisper model](#a-whisper-model) | Transcription | Always |
| [Ollama](#ollama) | Generating documents locally | For templates, unless you use Claude, Codex or another server |
| [An OpenAI-compatible server](#openai-compatible-servers) | Generating documents with LM Studio, Colibri or similar | No |
| [Claude Code](#claude-code) | Generating documents with Claude | When a template uses a `claude` backend |
| [Codex](#codex) | Generating documents with Codex | When a template uses a `codex` backend |

`minuta transcribe` needs only the first three. `minuta run -t ...` also needs
the LLM backend that its templates use.

When you are done, run `minuta doctor` to check everything (see
[Check your setup](#check-your-setup)).

## The quick way: Homebrew

[Homebrew](https://brew.sh) works on both macOS and Linux and installs
everything except the model:

```sh
brew install ffmpeg whisper-cpp ollama
minuta models pull base
```

Then skip to [Ollama](#ollama) to start it and download a model. If you prefer
your system's package manager, or want to build from source, read on.

## ffmpeg and ffprobe

`ffprobe` comes with `ffmpeg`, so one package installs both.

| System | Command |
| --- | --- |
| macOS | `brew install ffmpeg` |
| Debian, Ubuntu | `sudo apt install ffmpeg` |
| Fedora | Enable [RPM Fusion](https://rpmfusion.org/Configuration), then `sudo dnf install ffmpeg` |
| Arch | `sudo pacman -S ffmpeg` |

Check it:

```sh
ffmpeg -version
ffprobe -version
```

## whisper.cpp

minuta runs the `whisper-cli` program from
[whisper.cpp](https://github.com/ggml-org/whisper.cpp).

### With Homebrew

```sh
brew install whisper-cpp
```

On Apple Silicon this build uses the GPU through Metal.

### From source (Linux)

You need `git`, `cmake` and a C++ compiler. On Debian or Ubuntu, install them
with `sudo apt install git cmake build-essential`.

```sh
git clone https://github.com/ggml-org/whisper.cpp.git
cd whisper.cpp
cmake -B build -DBUILD_SHARED_LIBS=OFF
cmake --build build -j --config Release
sudo install build/bin/whisper-cli /usr/local/bin/
```

`-DBUILD_SHARED_LIBS=OFF` builds one self-contained binary, so it still works
after you copy it out of the build folder. To use a GPU, see the
[whisper.cpp build options](https://github.com/ggml-org/whisper.cpp#readme),
such as CUDA or Vulkan.

If you keep `whisper-cli` somewhere that is not on your `PATH`, point minuta to
it:

```toml
# ~/.minuta/config.toml
[whisper]
bin = "/opt/whisper.cpp/build/bin/whisper-cli"
```

Check it:

```sh
whisper-cli --help
```

## A whisper model

minuta downloads models for you, checks their checksums and stores them in
`~/.minuta/models/` (see the README for changing that folder):

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

Then make it the default, so you do not have to pass `--model` on every run:

```toml
# ~/.minuta/config.toml
[whisper]
model = "base"
```

You can also use a whisper.cpp model you already have. Set `model` to the
file's path instead of a catalog name.

### Optional: voice activity detection

Without voice activity detection (VAD), whisper sometimes writes phrases that
were never said during long silences. A VAD model, under 1 MiB, prevents that:

```sh
minuta models pull silero-v5.1.2
```

```toml
# ~/.minuta/config.toml
[whisper]
model = "base"
vad_model = "silero-v5.1.2"
```

## Ollama

[Ollama](https://ollama.com) runs language models on your machine, so your
transcripts never leave it. It is minuta's default backend.

| System | Command |
| --- | --- |
| macOS | `brew install ollama`, or download the app from [ollama.com/download](https://ollama.com/download) |
| Linux | `curl -fsSL https://ollama.com/install.sh \| sh` |

Start the server and download minuta's default model:

```sh
ollama serve              # leave running; the macOS app and the Linux service start it for you
ollama pull gemma4:26b
```

`gemma4:26b` needs a machine with plenty of memory. To use a smaller model,
pull it and make it the default:

```sh
ollama pull <model>
```

```toml
# ~/.minuta/config.toml
[llm]
default = "ollama:<model>"
```

Browse models at [ollama.com/library](https://ollama.com/library). If Ollama
runs on another machine or port, set `ollama.url`, for example
`url = "http://192.168.1.20:11434"` under `[ollama]`. Transcripts are then sent
to that machine.

## Claude Code

minuta can generate documents with Claude through the
[Claude Code](https://claude.com/claude-code) CLI. It uses your existing
Claude Code login, so there is no API key to configure. Install it with one of
these:

```sh
curl -fsSL https://claude.ai/install.sh | bash     # native installer
brew install --cask claude-code                    # Homebrew
npm install -g @anthropic-ai/claude-code           # npm
```

Run `claude` once to log in, then use it from minuta:

```sh
minuta run meeting.mp4 -t summary --llm claude
```

To use Claude for every run, set `default = "claude"` under `[llm]`. With
Claude, the transcript is sent to Anthropic. minuta prints a notice each time
that happens.

## Codex

minuta can also generate documents with OpenAI's
[Codex](https://github.com/openai/codex) CLI. It uses your existing Codex
login, so there is no API key to configure. Install it with one of these:

```sh
npm install -g @openai/codex                       # npm
brew install --cask codex                          # Homebrew
```

Run `codex login` once, then use it from minuta:

```sh
minuta run meeting.mp4 -t summary --llm codex
minuta run meeting.mp4 -t summary --llm codex:<model>   # a given model
```

To use Codex for every run, set `default = "codex"` under `[llm]`. minuta runs
Codex with a read-only sandbox, no session files and without your
`~/.codex/config.toml`, so plain `codex` uses Codex's built-in default model.
With Codex, the transcript is sent to OpenAI. minuta prints a notice each time
that happens.

## OpenAI-compatible servers

minuta can generate documents with any server that has an OpenAI-compatible API.
You name the server in the config file, then use `<endpoint>:<model>` as the
backend. `minuta config init` writes a commented example, and `minuta config
path` shows where the file is. The
[README](https://github.com/JCIOriente/minuta#openai-compatible-servers) lists
every key.

### LM Studio

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

### Colibri

1. Set up [Colibri](https://github.com/JustVugg/colibri) by following its README,
   and start it with `coli serve`.
2. Run `coli status` to get the model id.
3. Add the server to the config file. Large models can be slow, so give them more
   time:

   ```toml
   [endpoints.colibri]
   url = "http://127.0.0.1:8000/v1"
   timeout_secs = 3600
   ```

4. Use it:

   ```sh
   minuta run meeting.mp4 -t summary --llm colibri:<model id>
   ```

minuta has been tested against LM Studio but not yet against a running Colibri;
this setup comes from Colibri's API documentation. Colibri does not document
`response_format`. If it refuses the field, minuta sends the request again
without it.

### Another server

Set `url` to the base URL the server documents, usually ending in `/v1`. If it
needs a key, put the key in an environment variable and name the variable in
`api_key_env`:

```toml
[endpoints.openrouter]
url = "https://openrouter.ai/api/v1"
api_key_env = "OPENROUTER_API_KEY"
```

With a URL that is not on your machine, the transcript is sent to that server.
minuta prints a notice each time that happens.

## Check your setup

```sh
minuta doctor
```

`doctor` prints each tool with its version and path, the model, and the state
of the default LLM backend. If a tool or model is missing, it exits with status
1 and shows a fix for each one. An unreachable LLM backend is shown but does not
change the exit status.

```console
$ minuta doctor
ffmpeg       ffmpeg version 8.0 ... (/opt/homebrew/bin/ffmpeg)
ffprobe      ffprobe version 8.0 ... (/opt/homebrew/bin/ffprobe)
whisper-cli  whisper.cpp version: 1.8.0 (/opt/homebrew/bin/whisper-cli)
model        missing
llm          ollama:gemma4:26b (ok)
minuta: missing required tools: model
  model: run `minuta models pull base` or set whisper.model
```

Once `doctor` passes, continue with the
[quick start](https://github.com/JCIOriente/minuta#quick-start).
