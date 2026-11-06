# From a recording to meeting minutes

**English** | [Español](worked-example.es.md)

**Goal:** turn a recording of a meeting into minutes you can send to your team.
You will transcribe the recording, fix a bad transcript, generate the minutes,
check them and share them.

The example is a short recording called `weekly-sync.m4a`. Use your own
recording and change the name in each command. What you see on screen will
differ from this page in the words, because models write differently every time.

## Before you start

- minuta is installed and `minuta doctor` shows no `missing`. If not, follow
  [Fix what doctor reports](fix-doctor.md).
- You have a language model for documents, such as Ollama with the default
  model. If not, follow [Set up an LLM and a whisper model](llm-and-whisper.md).
- You have a recording of a meeting in a file that ffmpeg can read, such as
  `.mp4`, `.m4a`, `.mp3` or `.wav`.
- A terminal open in the folder that holds the recording:

  ```sh
  cd ~/Meetings
  ```

## 1. Check your setup

```sh
minuta doctor
```

Every line shows a version or a path, and the `llm` line ends in `(ok)`:

```console
ffmpeg       ffmpeg version 9.0.2 Copyright (c) 2000-2026 the FFmpeg developers (/opt/homebrew/bin/ffmpeg)
ffprobe      ffprobe version 9.0.2 Copyright (c) 2007-2026 the FFmpeg developers (/opt/homebrew/bin/ffprobe)
whisper-cli  whisper.cpp version: 1.9.5 (/opt/homebrew/bin/whisper-cli)
model        .../.minuta/models/ggml-base.bin
llm          ollama:gemma4:26b (ok)
```

If not, stop here and fix it with [the doctor guide](fix-doctor.md).

## 2. Write the transcript

```sh
minuta transcribe weekly-sync.m4a
```

minuta lists its steps and ends with the path of a new folder, the job folder:

```console
note: VAD disabled (no VAD model configured)
[1/5] probe       0.0s
[2/5] join        single source  0.0s
[3/5] audio       0.0s
[4/5] transcribe  language=en  4 segments  0.5s
[5/5] outputs     0.0s
finished in 0.8s (5 run, 0 cached)
.../minuta-out/weekly-sync
```

The `note: VAD disabled` line is information, not an error. A real meeting takes
longer; the transcription step is the slow one.

The job folder is `minuta-out/weekly-sync/`. It holds `transcript.txt` (the text
with timestamps) and `captions.srt` (subtitles). Read the transcript:

```sh
cat minuta-out/weekly-sync/transcript.txt
```

```text
Welcome to the Weekly Sailing. Maria reported that the website redesign is on track and will launch on Friday.
Tom said the payment book is fixed, but he is locked on a new test server.
We decided to move the launch review to Thursday. Action items. Maria will send a final copy by Wednesday.
Tom will ask Heidi for the test server today. Thanks everyone.
```

## 3. Fix a transcript with mistakes

The example transcript has errors: "Weekly Sailing" should be "Weekly Sync", and
"Heidi" should be "IT". The default model, `base`, is small and fast. A bigger
model hears better.

1. Download the `small` model (465 MiB):

   ```sh
   minuta models pull small
   ```

   ```console
   pulling small (465.0 MiB) into .../.minuta/models
   small downloaded and verified
   .../.minuta/models/ggml-small.bin
   ```

2. Transcribe again with it. minuta sees that the model changed and redoes only
   the transcription:

   ```sh
   minuta transcribe weekly-sync.m4a --model small
   ```

   ```console
   note: VAD disabled (no VAD model configured)
   [1/5] probe       cached
   [2/5] join        cached
   [3/5] audio       cached
   [4/5] transcribe  language=en  8 segments  0.8s
   [5/5] outputs     0.0s
   finished in 1.0s (2 run, 3 cached)
   .../minuta-out/weekly-sync
   ```

3. Read the transcript again:

   ```text
   Welcome to the Weekly Sync.
   Maria reported that the website redesign is on track and will launch on Friday.
   Tom said the payment buck is fixed, but he is locked on the new test server.
   We decided to move the launch review to Thursday.
   Action items.
   Maria will send the final copy by Wednesday.
   Tom will ask IT for the test server today.
   Thanks everyone.
   ```

   Better. One word is still wrong ("buck"). No model is perfect, so you will
   check the minutes against the transcript in step 6.

4. Make `small` the default so you do not need `--model` again. Open the config
   file (`minuta config path` prints where it is; `minuta config init` creates
   it) and set the model under `[whisper]`:

   ```toml
   [whisper]
   model = "small"
   ```

**If it did not work.** `unknown model` means the name is not in
`minuta models list`. A long recording with silences can make whisper invent
text; [the models guide](llm-and-whisper.md) shows the fix (step 5 of Part 1).

## 4. Get a minutes template

minuta has a built-in `summary` template. For minutes, with decisions and
action items first, download the ready-made `meeting-minutes` template. It goes
into a `templates` folder in the current folder:

```sh
minuta templates pull meeting-minutes --yes
```

```console
meeting-minutes.md  written
.../templates/meeting-minutes.md
```

Check that minuta sees it:

```sh
minuta templates list
```

The list now has a row with the layer `project`:

```console
meeting-minutes  project   transcript  markdown  (default)  Meeting minutes with decisions and actions first, followed by brief discussion
```

**If it did not work.** `pull` needs an internet connection. Without one, write
your own template instead: [Add and override your own templates](custom-templates.md).

## 5. Generate the minutes

```sh
minuta run weekly-sync.m4a -t meeting-minutes
```

The transcript is already there, so minuta reuses it and only asks the language
model to write the minutes. The last lines are:

```console
finished in 2.6s (1 run, 5 cached)
.../minuta-out/weekly-sync
```

The result is `minuta-out/weekly-sync/meeting-minutes.md`:

```sh
cat minuta-out/weekly-sync/meeting-minutes.md
```

This is an example. Your wording will differ:

```markdown
# weekly-sync

**Duration:** 0:00:25
**Attendees:** Maria, Tom

## Overview
The meeting covered the status of the website redesign, a fix for the payment bug, and a scheduling change for the launch review.

## Decisions
* The launch review was moved to Thursday. [00:00:00]

## Action Items
* [ ] Maria: Send the final copy by Wednesday. [00:00:00]
* [ ] Tom: Ask IT for the test server today. [00:00:00]

## Discussion
* Maria reported that the website redesign is on track for a launch on Friday. [00:00:00]
* Tom stated that the payment bug is fixed but noted he is locked out of the new test server. [00:00:00]
```

**If it did not work.**

- `unknown template 'meeting-minutes'`: the template is not in the current
  folder. Run `minuta templates list`; if `meeting-minutes` is not there, go to
  the folder where you ran `pull`.
- `not reachable` or `model missing`: your language model is not ready. See
  [the doctor guide](fix-doctor.md#5-llm-is-not-ok).

## 6. Check the minutes before you share them

A model can make mistakes, so read the minutes once against the transcript.
Check these, in order:

1. **Names.** Does each name match what people said?
2. **Numbers and dates.** "Wednesday" and "Thursday" in the example match the
   transcript.
3. **Owners.** Is each action next to the right person?
4. **Nothing invented.** Every line should be in the transcript. Delete what is
   not.

The template tells the model not to invent anything. It still helps to check.

If you want a change, edit `meeting-minutes.md` by hand. It is a plain text file.
To have the model write it again, run the command from step 5 with
`--force meeting-minutes` after the file name. That reruns this template only:

```sh
minuta run weekly-sync.m4a -t meeting-minutes --force meeting-minutes
```

## 7. Write the minutes in another language

The documents use the language of the recording. To get them in another
language, name it with `--output-lang`:

```sh
minuta run weekly-sync.m4a -t meeting-minutes --output-lang es
```

Only the minutes are written again. The headings and the text come out in
Spanish:

```markdown
# weekly-sync

**Duración:** 0:00:25
**Asistentes:** Maria, Tom

## Resumen de la reunión
```

Running the command again without `--output-lang es` writes the minutes in
English again.

## 8. Share the minutes

The folder `minuta-out/weekly-sync/` holds everything:

```console
$ ls -A minuta-out/weekly-sync
.minuta  assets  captions.srt  meeting-minutes.md  transcript.txt
```

| File | Use it for |
| --- | --- |
| `meeting-minutes.md` | The minutes. Paste them into an email or a wiki, or send the file |
| `transcript.txt` | The full text, for people who want to check a detail |
| `captions.srt` | Subtitles, if you share the recording as a video |

Do not send `.minuta` or `assets`: they are for minuta. To share the minutes
from the terminal, `cat` the file or copy it. On macOS, `pbcopy` puts it in the
clipboard:

```sh
pbcopy < minuta-out/weekly-sync/meeting-minutes.md
```

The recording and transcript are stored on your computer only, unless you chose
an LLM that is not local. See [the models guide](llm-and-whisper.md) for where
your transcript goes.

## Next

- Make the template yours: [Add and override your own templates](custom-templates.md).
- Run it on every meeting: `minuta run` accepts several files. See
  [Reference](../reference.md#commands).
