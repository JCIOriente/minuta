# Add and override your own templates

**English** | [Español](custom-templates.es.md)

**Goal:** write a template of your own, run it, and then replace one of minuta's
built-in templates with your version.

A template is a text file with a prompt in it. minuta sends the prompt and your
transcript to a language model, and saves the answer as a document. You choose a
template with `-t`, for example `-t summary`.

## Before you start

- minuta is installed and `minuta doctor` passes. If not, see the
  [installation guide](../install.md) and [fix what doctor reports](fix-doctor.md).
- You have a recording. This page uses `meeting.mp4`; use your own file name.
- A terminal open in the folder that holds the recording.

## Part 1: write a new template

1. See which templates minuta already has:

   ```sh
   minuta templates list
   ```

   You get a table. Each row is a template. The `LAYER` column says where it
   comes from: `built-in` for the ones inside minuta.

   ```console
   NAME       LAYER     INPUT       OUTPUT    LLM        DESCRIPTION
   chapters   built-in  transcript  chapters  (default)  Timestamped chapters (YouTube rules)
   instagram  built-in  transcript  fields    (default)  Instagram caption about the recording
   linkedin   built-in  transcript  fields    (default)  LinkedIn post about the recording
   summary    built-in  transcript  markdown  (default)  Short summary of the recording
   youtube    built-in  transcript  fields    (default)  YouTube title, description, tags and thumbnail text
   ```

2. Make a folder called `templates` next to your recording. minuta reads
   templates from a folder with this name in the folder where you run it:

   ```sh
   mkdir templates
   ```

3. Create the file `templates/weekly-sync.md` with this content. The file name
   is the template's name, so this template is called `weekly-sync`.

   ```markdown
   ---
   description: Notes for our weekly sync
   input: transcript
   ---
   These are the notes of "{{job_name}}" ({{duration}}).

   List what each person reported, then the blockers and who owns each one.
   Use only what is said in the transcript.
   ```

   The part between the two `---` lines is the header. `description` is the
   text that `templates list` shows. `input: transcript` means the template
   reads the transcript. Everything after the header is the prompt.
   `{{job_name}}` and `{{duration}}` are filled in by minuta.

   A template name starts with a lowercase letter or a digit and uses only
   lowercase letters, digits, `-` and `_`.

4. Check that minuta sees it:

   ```sh
   minuta templates list
   ```

   There is a new row, with the layer `project`:

   ```console
   weekly-sync  project   transcript  markdown  (default)  Notes for our weekly sync
   ```

5. Run it:

   ```sh
   minuta run meeting.mp4 -t weekly-sync
   ```

   minuta prints its progress and, at the end, the path of the job folder. The
   document is `weekly-sync.md` in that folder. The wording of the answer
   depends on your model; this is an example:

   ```text
   ### Weekly Sync Report

   **Reports**
   * **Maria:** The website redesign is on track and will launch on Friday.
   * **Tom:** The payment bug is fixed.

   **Blockers**
   * **Tom:** Locked out of the new test server.
   ```

**If it did not work.** Run `minuta templates list`. A file minuta cannot read
is listed at the bottom, in a line that starts with `invalid`, with the reason.
For example, a typo such as `{{who}}` gives:

```console
invalid  .../templates/oops.md  template oops: unknown variable 'who' (known: output_lang, output_lang_code, speech_lang, job_name, duration)
```

Fix the file and run the command again. The full list of header keys and
variables is in [Writing a template](../templates.md#writing-a-template).

## Part 2: override a built-in template

You can replace a built-in template by creating a template with the same name.
The easiest way to start is to copy the built-in one and change the copy.

1. Copy the built-in `summary` template into a file in the current folder:

   ```sh
   minuta templates show summary > summary.md
   ```

   Do not write the copy straight into the `templates` folder. The shell empties
   the file before minuta reads the template, and you would get an empty
   `summary` that replaces the real one.

2. Open `summary.md` in any text editor and change the prompt. For example, add
   a line at the end: `Write at most five bullet points.` Keep the header at
   the top.

3. Move the file to where you want it to apply.

   - **Only for this folder:** put it in `templates`.

     ```sh
     mkdir -p templates
     mv summary.md templates/
     ```

   - **Everywhere:** put it in your user templates folder.

     ```sh
     mkdir -p ~/.minuta/templates
     mv summary.md ~/.minuta/templates/
     ```

4. Check that your version is the one in use. The layer is no longer `built-in`:

   ```sh
   minuta templates list
   ```

   ```console
   summary    user      transcript  markdown  (default)  Short summary of the recording
   ```

   It says `user` if you used `~/.minuta/templates`, or `project` if you used
   `templates`.

5. Run it as always. The name `summary` now means your version:

   ```sh
   minuta run meeting.mp4 -t summary
   ```

To go back to the built-in template, delete your file.

**How minuta chooses.** There are three places, and a later one wins over an
earlier one: built in, then `~/.minuta/templates`, then `./templates` in the
current folder.

**If it did not work.**

- The `summary` row still says `built-in`: the file is not in the right folder
  or does not end in `.md`. Run `minuta templates show summary` to see which file
  minuta uses; its path is printed to the error output.
- The `summary` row has no description and the document is not a summary: the
  file is empty, because you wrote the copy straight into the `templates`
  folder. Delete the file and start again from step 1.
- A job folder already exists and nothing changed: minuta reuses finished work.
  After you edit a template, run `minuta run meeting.mp4 -t summary` again and
  only that template runs. To force it, add `--force summary` after the file
  name.

> **Careful.** minuta reads `./templates` from whatever folder you run it in. If
> you clone a project from somebody else, read its templates before you run
> minuta there.

## Part 3: download ready-made templates

The minuta repository has a catalog of ready-made templates.

1. List them. This needs an internet connection:

   ```sh
   minuta templates pull --list
   ```

   ```console
   templates from https://raw.githubusercontent.com/JCIOriente/minuta/main/templates/
   NAME                PLATFORM   STATE  DESCRIPTION
   action-items        meeting    new    Flat action checklist sorted by owner with stated deadlines, blockers and timestamps
   meeting-minutes     meeting    new    Meeting minutes with decisions and actions first, followed by brief discussion
   ...
   ```

   The catalog changes over time, so your list will differ.

2. Download one into `./templates`:

   ```sh
   minuta templates pull meeting-minutes --yes
   ```

   ```console
   meeting-minutes.md  written
   .../templates/meeting-minutes.md
   ```

   Without `--yes`, minuta shows what it will download and asks
   `Continue? [y/N]`. Type `y` to continue.

3. Use it like any other template: `minuta run meeting.mp4 -t meeting-minutes`.

A downloaded file is an ordinary template: you can edit it. If you edit it and
later run `pull` again, minuta skips your file instead of overwriting it. Add
`--force` to replace it; your version is saved first as `meeting-minutes.md.bak`.

## Next

- Every header key, variable and the structured `fields` output:
  [Templates](../templates.md).
- Choose which language model writes the documents:
  [Set up an LLM and a whisper model](llm-and-whisper.md).
- A complete run with a ready-made template:
  [From a recording to meeting minutes](worked-example.md).
