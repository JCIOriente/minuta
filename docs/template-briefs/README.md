# Template briefs

A brief is the instruction you give an agent (Codex, Claude, anyone) so it writes one template
well. The brief is not the template. It says who the author is, what the platform demands and
what a good result looks like; the agent turns it into a file in the format of
`docs/adr/0004-template-file-format.md`.

One brief per template, named after the template: `instagram.md` -> `templates/instagram.md`.

## What every brief contains
- **Template**: name, output kind (`fields` or Markdown), input chain, and the limits to put in the frontmatter.
- **Persona**: the expert the agent plays. It sets vocabulary and judgment, so be specific.
- **Audience and goal**: who reads the result and what it should make them do.
- **Platform rules**: numbers and behaviors that are facts about the platform.
- **Structure**: what each field or section holds.
- **Tone and style**, and **Must never**.
- **Good and bad**: two good/bad pairs, each on a different aspect (for example the opening and the closing), so the agent can tell the difference.

## What minuta lets a template do
- `output: fields` returns JSON, writes it to `assets/<name>.json` and renders `<name>.md`. The fields are `title`, `body`, `tags`, `hashtags`, `thumbnail_text`, `chapters` and `notes`. `chapters` is filled by minuta from the `chapters` template, not written by the model, and `notes` holds lines for the person publishing, not published text. (The template named `chapters` is the only one with `output: chapters`.) `limits:` enforces `max_chars` and `max` per field, and minuta repairs answers that break them. Use it when the platform has hard limits.
- Markdown output (the default) is free text written to `<name>.md`. Limits cannot be enforced there, so the brief must say what the model has to respect.
- Minuta appends the field list, the limits and the output-language sentence itself. Do not write `{{output_lang}}` into a template.
- Body variables: `{{job_name}}`, `{{duration}}`, `{{speech_lang}}`, `{{output_lang}}`, `{{output_lang_code}}`.
- `input:` is `transcript` (default) or another template's name. Do not recommend `summary` as an input: the social templates read the transcript, and `summary` runs only when someone asks for it.

## Using a brief with Codex
```console
$ codex exec -c model_reasoning_effort=high \
    "Write the minuta template described in docs/template-briefs/instagram.md. \
     Follow docs/adr/0004-template-file-format.md. Output only the file."
```
Then run it: `minuta run sample.mp4 -t instagram` (with the template in `./templates`) and compare
the output with the brief's "Good" example. A second reader should review it against the brief.

## Platform numbers
The numbers in the briefs are the platforms' published limits as the authors knew them in
October 2026. Platforms change them. Re-check before shipping a template, and update the brief and
the `limits:` together.
