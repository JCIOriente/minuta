# Installing minuta's requirements

minuta runs on macOS and Linux. It does not bundle its tools. It calls these
programs, which you install once:

| Requirement | Needed for | Required? |
| --- | --- | --- |
| [ffmpeg and ffprobe](#ffmpeg-and-ffprobe) | Reading, joining and converting media | Always |
| [whisper.cpp](#whispercpp) (`whisper-cli`) | Transcription | Always |
| [A whisper model](#a-whisper-model) | Transcription | Always |
| [Ollama](#ollama) | Generating documents locally | For templates, unless you use Claude |
| [Claude Code](#claude-code) | Generating documents with Claude | When a template uses a `claude` backend |

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
# ~/.config/minuta/config.toml
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
# ~/.config/minuta/config.toml
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
# ~/.config/minuta/config.toml
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
# ~/.config/minuta/config.toml
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
