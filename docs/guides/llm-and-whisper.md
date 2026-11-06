# Set up an LLM and a whisper model

**English** | [Español](llm-and-whisper.es.md)

**Goal:** choose the whisper model that turns speech into text, and the language
model (LLM) that writes your documents, then check that both work.

minuta uses two kinds of model:

- A **whisper model** listens to the recording and writes the transcript. It
  runs on your computer. You always need one.
- An **LLM** reads the transcript and writes documents, such as a summary. You
  need one only when you use `-t`. It can run on your computer or on a service.

## Before you start

- minuta, `ffmpeg` and `whisper-cli` are installed. See the
  [installation guide](../install.md).
- A short recording to test with. This page calls it `meeting.mp4`; replace it
  with your file name.
- A terminal open in the folder that holds the recording.

The output on this page is an example. Your versions, paths, installed models
and timings will differ, and in an interactive terminal a download shows a
progress bar.

## Part 1: choose a whisper model

1. See the models minuta can download, and which ones you already have:

   ```sh
   minuta models list
   ```

   ```console
   models in .../.minuta/models
   tiny           available  74.1 MiB
   base           installed  141.1 MiB
   small          available  465.0 MiB
   medium         available  1.4 GiB
   large-v3       available  2.9 GiB
   silero-v5.1.2  available  0.8 MiB
   ```

   A bigger model is more accurate and slower. `base` is a good first choice.
   Use `small` or larger if names and technical words come out wrong.
   `silero-v5.1.2` is not a whisper model: it is the optional VAD model of step 5.

2. Download one. minuta checks the file after the download:

   ```sh
   minuta models pull small
   ```

   ```console
   pulling small (465.0 MiB) into .../.minuta/models
   small downloaded and verified
   .../.minuta/models/ggml-small.bin
   ```

3. Make it the default, so you do not have to name it on every run. Create the
   config file, which has every setting commented out:

   ```sh
   minuta config init
   ```

   ```console
   created .../.minuta/config.toml
   ```

   If the file exists already, the command says `already exists; nothing
   changed`, and you can go on. `minuta config path` prints where the file is.

4. Open that file in a text editor. Under the `[whisper]` line, find the
   `model` line. If it starts with `# `, delete the `# `. Set the model you
   downloaded:

   ```toml
   [whisper]
   model = "small"
   ```

   Change the lines that are already there. Do not add a second `[whisper]`
   section: minuta stops with `duplicate key`.

   Save the file. To try a model once without changing the file, add
   `--model small` to a command instead: `minuta run meeting.mp4 --model small`.

5. Optional: reduce invented text in silences. On long recordings with quiet
   parts, whisper sometimes writes phrases nobody said. A small extra model,
   called a VAD model, helps:

   ```sh
   minuta models pull silero-v5.1.2
   ```

   Then, in the same `[whisper]` section, add the line `vad_model`:

   ```toml
   [whisper]
   model = "small"
   vad_model = "silero-v5.1.2"
   ```

6. Check it:

   ```sh
   minuta doctor
   ```

   The `model` line shows the path of the file instead of `missing`:

   ```console
   model        .../.minuta/models/ggml-small.bin
   ```

**If it did not work.**

- `model missing`: the file is not downloaded. `doctor` prints the command that
  fixes it, such as ``run `minuta models pull small` ``.
- `unknown model`: use one of the names in `minuta models list`.
- `is a VAD model; set it as whisper.vad_model`: you put `silero-v5.1.2` in
  `model`. Move it to `vad_model`.
- `config error: ... invalid TOML at line ...`: the config file has a mistake,
  such as a second `[whisper]` section. Fix the line the message names.
- You already have a whisper.cpp model file? Set `model` to its full path,
  for example `model = "/Users/ana/models/ggml-medium.bin"`.

## Part 2: choose an LLM

The table shows where your transcript goes. Pick one and follow its section.

| Option | Your transcript goes to | Pick it if |
| --- | --- | --- |
| Ollama | Your computer | You want everything private. This is the default |
| Claude Code | Anthropic | You already use Claude Code |
| Codex | OpenAI | You already use Codex |
| Another server | That server | You use LM Studio, Colibri or a cloud service |

Whatever you pick, you name the LLM with an id: `ollama:<model>`, `claude`,
`codex` or `<server-name>:<model>`. The parts in angle brackets are
placeholders: replace them, brackets included. You can pass the id for one run
with `--llm`, or save it as the default under `[llm]` in the config file.

### Ollama (on your computer)

1. Install Ollama by following [Ollama](../install.md#ollama) in the
   installation guide.
2. Start it. The macOS app and the Linux service start it for you; if not, run
   this in a terminal and leave it open:

   ```sh
   ollama serve
   ```

3. In another terminal, download one model. Pick one of these and download only
   that one:

   - **minuta's default, `gemma4:26b`.** It is large (about 17 GB) and needs a
     computer with plenty of memory. There is nothing to configure after the
     download:

     ```sh
     ollama pull gemma4:26b
     ```

   - **A smaller model**, from [ollama.com/library](https://ollama.com/library).
     Check its size against your computer's memory first. For example, for
     `qwen3:14b`:

     ```sh
     ollama pull qwen3:14b
     ```

     Then make it the default. In the config file, find `# [llm]` and the line
     `# default = ...` below it. Delete the `# ` at the start of both and write
     your model after `ollama:`:

     ```toml
     [llm]
     default = "ollama:qwen3:14b"
     ```

4. Run `minuta doctor`. The `llm` line ends in `(ok)` (see
   [Check your setup](#check-your-setup)).

If Ollama runs on another computer, set `[ollama]` `url` to its address. Then
your transcripts go to that computer. If answers are cut off, raise
`[ollama]` `num_ctx` if the model supports a larger window.

### Claude Code or Codex

minuta uses the login you already have. There is no key to set.

1. Install the tool and log in once, as described in
   [Claude Code](../install.md#claude-code) or [Codex](../install.md#codex) in
   the installation guide.
2. Use it for one run:

   ```sh
   minuta run meeting.mp4 -t summary --llm claude
   ```

   Use `--llm codex` for Codex. minuta prints a notice that the transcript is
   sent to the company. When it finishes, the summary is in the job folder that
   minuta prints, here `minuta-out/meeting/summary.md`:

   ```sh
   cat minuta-out/meeting/summary.md
   ```

   This run is the real test. `minuta doctor` only checks that the program
   starts; it does not check that you are logged in. If the run fails with a
   login or authentication error, run `claude` (or `codex login`) again.
3. To make it the default, set `default = "claude"` (or `"codex"`) under `[llm]`.

### Another server (LM Studio, Colibri, a cloud service)

Use a server with an OpenAI-compatible API: it must answer `/models` and
`/chat/completions`. LM Studio, Colibri, llama.cpp and vLLM do. You give the
server a name once, in the config file, and then use that name in the id.
Setup steps for LM Studio and Colibri are in
[OpenAI-compatible servers](../install.md#openai-compatible-servers).

1. Start the server and load a model with a context of at least 16384 tokens.
   Note the server's address, which usually ends in `/v1`, and the exact model
   name.
2. In the config file, add a section with a name you choose. This example calls
   the server `lmstudio`:

   ```toml
   [endpoints.lmstudio]
   url = "http://127.0.0.1:1234/v1"
   ```

   The name starts with a lowercase letter (`a` to `z`) and uses only lowercase
   letters, digits, `-` and `_`. It cannot be `ollama`, `claude` or `codex`.
   If the server gives the model a context smaller than 16384 tokens, add
   `context_tokens` with that number.
3. If the server needs a key, do not write the key in the file. Write the name
   of an environment variable in `api_key_env`. This example is a cloud service
   called `openrouter`:

   ```toml
   [endpoints.openrouter]
   url = "https://openrouter.ai/api/v1"
   api_key_env = "OPENROUTER_API_KEY"
   ```

   Then set the variable in the terminal where you run minuta. Replace the text
   in quotes with your key:

   ```sh
   export OPENROUTER_API_KEY='replace-with-your-key'
   ```

   The variable lasts until you close this terminal. To keep it, add the same
   line to your shell's startup file, such as `~/.zshrc` or `~/.bashrc`.
4. Check the server and the model. The id is the name, a colon, and the model
   name exactly as the server lists it:

   ```sh
   MINUTA_LLM=lmstudio:google/gemma-4-26b-a4b minuta doctor
   ```

   The last line ends in `(ok)`:

   ```console
   llm          lmstudio:google/gemma-4-26b-a4b (ok)
   ```

5. Use it:

   ```sh
   minuta run meeting.mp4 -t summary --llm lmstudio:google/gemma-4-26b-a4b
   ```

   The summary is in the job folder that minuta prints, here
   `minuta-out/meeting/summary.md`.

A server that is not on your computer receives your transcript, and minuta
prints a notice each time. Slow servers need more time: raise `timeout_secs`
in the section (the default is 600 seconds). All the keys are in
[OpenAI-compatible servers](../templates.md#openai-compatible-servers).

## Check your setup

```sh
minuta doctor
```

The last line is your default LLM, and it ends in `(ok)`:

```console
llm          ollama:gemma4:26b (ok)
```

To check another LLM without changing the config file, set it for one command
with the environment variable `MINUTA_LLM`. Replace `claude` with the id you
want to check:

```sh
MINUTA_LLM=claude minuta doctor
```

For Claude Code and Codex, `(ok)` means the program starts, not that you are
logged in. Run a short `minuta run ... -t summary` to test them end to end.

To see which value minuta uses and where it came from, run
`minuta config show`. A `(file)` or `(env)` at the end of a line tells you.

**If it did not work.** The text in brackets says what is wrong:

| `doctor` says | What to do |
| --- | --- |
| `not reachable: start Ollama ...` | Start Ollama with `ollama serve`. If it runs elsewhere, set `ollama.url` |
| `model missing: ollama pull <model>` | Run the command it prints |
| `not reachable: install Claude Code: ...` or `install Codex: ...` | Install that tool and log in |
| `invalid: unknown endpoint "lmstudio"; add it to the config file...` | Add the `[endpoints.lmstudio]` section |
| `model missing: not offered by the server; it offers: ...` | Use one of the names in the list |
| `not reachable: start the server or fix endpoints.lmstudio.url...` | Start the server, or correct `url` |
| `not reachable: set OPENROUTER_API_KEY to the API key ...` | Run the `export` line from step 3 in the same terminal |
| `... 401 ...; check the key in endpoints.openrouter.api_key_env` (or `403`) | The server refused the key. Check that you copied it whole, and that it is still valid |

More fixes: [Fix what doctor reports](fix-doctor.md).

## Next

- Run everything on a real recording:
  [From a recording to meeting minutes](worked-example.md).
- All settings and environment variables:
  [Configuration](../reference.md#configuration).
