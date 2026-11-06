# From a recording to meeting minutes

**English** | [Español](worked-example.es.md)

**Goal:** turn a recording of a meeting into minutes you can send to your team.
You will transcribe the recording, fix a bad transcript, generate the minutes,
check them and share them.

The example is a short English recording called `weekly-sync.m4a`. Use your own
recording: replace the folder and the file name in each command with yours. If a
name has spaces, put it in quotes, as in `minuta transcribe "weekly sync.m4a"`.

The output on this page is an example. On your screen the words, versions,
paths, timings and counts will differ, and in an interactive terminal the
progress shows as bars instead of plain lines.

## Before you start

- minuta is installed and `minuta doctor` shows no `missing`. If not, follow
  [Fix what doctor reports](fix-doctor.md).
- You have a language model for documents, such as Ollama with the default
  model. If not, follow [Set up an LLM and a whisper model](llm-and-whisper.md).
- You have a recording of a meeting in a file that ffmpeg can read, such as
  `.mp4`, `.m4a`, `.mp3` or `.wav`.
- A terminal open in the folder that holds the recording. Replace `~/Meetings`
  with that folder:

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

The job folder is named after the recording: here, `minuta-out/weekly-sync/`.
It holds `transcript.txt` (plain text, without timestamps) and `captions.srt`
(subtitles, with timestamps). Read the transcript:

```sh
cat minuta-out/weekly-sync/transcript.txt
```

```text
Welcome to the Weekly Sailing. Maria reported that the website redesign is on track and will launch on Friday.
Tom said the payment book is fixed, but he is locked on a new test server.
We decided to move the launch review to Thursday. Action items. Maria will send a final copy by Wednesday.
Tom will ask Heidi for the test server today. Thanks everyone.
```

**If it did not work.**

- `source file not found`: the file is not in this folder, or its name is
  spelled differently. Run `ls` to see the exact name, and use quotes if it has
  spaces.
- `holds a job made from different sources`: another recording already used the
  name `weekly-sync`. Choose a new name with `--name`, such as
  `minuta transcribe weekly-sync.m4a --name weekly-sync-2`, then use the folder
  minuta prints in the next commands.

More errors are in [Troubleshooting](../reference.md#troubleshooting).

## 3. Fix a transcript with mistakes

The example transcript has errors: "Weekly Sailing" should be "Weekly Sync", and
"Heidi" should be "IT". It was made with `base`, the model the installation
guide sets up. `base` is small and fast. A bigger model hears better.

If your transcript is good enough, or you already use `small` or a larger
model, skip to step 4.

1. Download the `small` model (465 MiB):

   ```sh
   minuta models pull small
   ```

   ```console
   pulling small (465.0 MiB) into .../.minuta/models
   small downloaded and verified
   .../.minuta/models/ggml-small.bin
   ```

2. Transcribe again with it. minuta sees that the model changed, and redoes only
   the transcription and the files made from it:

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

   ```sh
   cat minuta-out/weekly-sync/transcript.txt
   ```

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

   Better, but "buck" and "locked on" are still wrong. No model is perfect, so
   you will check the minutes against the recording in step 7.

4. Make `small` the default so you do not need `--model` again. Run
   `minuta config path` to see where the config file is, and open it. Under
   `[whisper]`, change the `model` line:

   ```toml
   [whisper]
   model = "small"
   ```

   Change the line that is already there. Do not add a second `[whisper]`
   section: minuta stops with `duplicate key`. If you have no config file yet,
   run `minuta config init` first.

**If it did not work.**

- `unknown model`: the name is not in `minuta models list`.
- `config error: ... invalid TOML at line ...`: the config file has a mistake.
  Open the file that `minuta config path` prints and fix the line the message
  names.
- On a long recording with silences, whisper can invent text.
  [The models guide](llm-and-whisper.md) shows the fix (step 5 of Part 1).

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

## 6. Choose the language of the minutes

Skip this step if the language is right.

Without any setting, documents use the language spoken in the recording. To get
them in another language, name it with `--output-lang`:

```sh
minuta run weekly-sync.m4a -t meeting-minutes --output-lang es
```

Only the minutes are written again, and the new file replaces the old one. The
headings and the text come out in Spanish:

```markdown
# weekly-sync

**Duración:** 0:00:25
**Asistentes:** Maria, Tom

## Resumen de la reunión
```

Running the command without `--output-lang` writes the minutes in your usual
language again: the one set in the template or the config file, or else the
language of the recording. In this example, that is English.

Choose the language before you edit the minutes in step 7. Each new run of the
template replaces the file, and your edits with it.

## 7. Check the minutes before you share them

A model can make mistakes, and so can the transcript. The template lets the
model correct an obvious mishearing, as it did with "buck", so the minutes and
the transcript can differ. Check the facts against the recording itself. The
timestamps in the minutes, such as `[00:00:00]`, and in `captions.srt` tell you
where to listen.

Check these, in order:

1. **Names.** Does each name match what people said?
2. **Numbers and dates.** "Wednesday" and "Thursday" in the example match the
   recording.
3. **Owners.** Is each action next to the right person?
4. **Nothing invented.** Every factual claim must be supported by the
   recording. Delete what is not.

To make a change, edit `minuta-out/weekly-sync/meeting-minutes.md` by hand. It is
a plain text file. Before you run the template again, save a copy of your edited
file, for example:

```sh
cp minuta-out/weekly-sync/meeting-minutes.md weekly-sync-minutes.md
```

To have the model write the minutes again from scratch, run the command from
step 5 with `--force meeting-minutes`. That reruns this template only, and
replaces your edits:

```sh
minuta run weekly-sync.m4a -t meeting-minutes --force meeting-minutes
```

## 8. Share the minutes

The folder `minuta-out/weekly-sync/` holds everything:

```console
$ ls -A minuta-out/weekly-sync
.minuta  assets  captions.srt  meeting-minutes.md  transcript.txt
```

| File | Use it for |
| --- | --- |
| `meeting-minutes.md` | The minutes. Paste them into an email or a wiki, or send the file |
| `transcript.txt` | The full text, without timestamps, for people who want to check a detail |
| `captions.srt` | Subtitles with timestamps, if you share the recording as a video |

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
- For your next meeting, run the same command on its recording. Each meeting
  gets its own job folder. Pass several files to one command only when they are
  successive parts of the same meeting: minuta joins them into one job. See
  [Reference](../reference.md#commands).
