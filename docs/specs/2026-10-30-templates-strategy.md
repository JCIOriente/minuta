# Publishing kits: stronger curated templates — Spec

- Status: approved
- Date: 2026-10-30
- Owner: Arnold Gandarillas Castillo
- Related: docs/specs/2026-10-30-templates-pull.md, docs/specs/2026-10-25-chapters.md,
  docs/template-briefs/platform-research-2026-10.md, ADR-0004, ADR-0006, ADR-0007, ADR-0008,
  ADR-0012, `docs/conventions.md` (curated template text)

## TL;DR
- Curated `youtube` gets chapters back, plus hashtags and a structured description. The cause was
  a dropped frontmatter line, and `docs/template-briefs/README.md:20` wrongly says `chapters` is
  reserved, so that gets fixed too.
- A new text field, `notes`, holds what the publisher needs but must not paste into the post:
  pinned comment, title and thumbnail options, alt text, preview text, meta description (ADR-0012).
  It is the only Rust change besides one line in the built-in `youtube`.
- Every curated template is rewritten as a publishing kit from dated platform research, with
  limits corrected where the briefs are stale.
- Three new templates: `youtube-shorts`, `instagram-carousel` and `lesson`.
- A 0.4.0 binary cannot parse a template that declares `notes`, so `templates pull` refuses those
  files until the user upgrades. The templates merge only with the release that adds `notes`.

## Change in a nutshell
`youtube.md` from `Webinar 1` today is a title, three paragraphs, a comma list and a thumbnail
line. After this change `templates/youtube.md` declares:
```yaml
fields: [title, body, chapters, hashtags, tags, thumbnail_text, notes]
limits:
  title: { max_chars: 100 }
  body: { max_chars: 3200 }
  hashtags: { max: 5, max_chars: 80 }
  tags: { max: 15, max_chars: 480 }
  thumbnail_text: { max_chars: 40 }
  notes: { max_chars: 1000 }
format: plain
```
and renders, in the output language:
```text
Carrera en IA: cómo aportar criterio y delegar con éxito

Delegar no es soltar el resultado: aporta criterio y verifica lo que produce la IA.

En este webinar aprenderás:
• Qué tareas delegar y cuáles conservar
• Cómo escribir una instrucción completa

Para quién es: profesionales que ya usan IA en su trabajo.

Suscríbete y cuéntanos qué tarea delegarías primero.

0:00 Introducción
4:12 Qué tareas delegar
17:40 La regla de oro de los prompts

#InteligenciaArtificial #AgentesDeIA #PromptEngineering

agentes de ingeniería, optimización de prompts, modelos Claude

Delega sin perder criterio

Comentario fijado: ¿Qué tarea de tu rutina delegarías primero a un agente?
Títulos alternativos: Qué delegar a la IA y qué no | Cómo aportar criterio en la era de la IA
Miniaturas alternativas: Tu criterio vale más | Qué no delegar
Antes de publicar: añade los enlaces al material y al canal.
```
Body, chapters and hashtags sit together, so they paste as one description. Everything after them
is for the publisher. The same idea applies to every platform: each template delivers the whole
kit, and `notes` separates what is pasted from what is not.

## Problem
The curated templates replaced richer prompts with copy-only output. `youtube` lost chapters when
it overrode the built-in, and it lacks hashtags, a description structure and a pinned comment, all
of which the owner's earlier `youtube-copywriter` prompt delivered. No template gives cover text,
alt text, preview text or a meta description. Several brief numbers are stale, such as TikTok's
six hashtags and X's link penalty. Fields are closed and rendered without labels, and `body` is
what the publisher will post, so these pieces have no place to go.

## Goals / Success criteria
1. A fields template can declare `notes`. It is a text field: the model answers it, `limits` apply,
   it is rejected when empty like every field, and it renders in declared order in `<name>.md` and
   under `fields.notes` in `<name>.json`.
2. Fingerprints of templates that do not declare `notes` do not change (fixed expected values), and
   `FIELDS_VERSION` and the artifact `version` stay 1. Built-in `youtube` is the exception, because
   its file changes.
3. Built-in `youtube` declares `hashtags` after `chapters`. Built-in `instagram` and `linkedin` do
   not change.
4. `templates/youtube.md` and `templates/podcast-notes.md` declare `chapters`. A run of either
   writes real chapter lines, and `youtube.md` has body, chapters and hashtags adjacent.
5. Each curated template declares the fields and limits in the table under "Domain model".
   `templates/index.toml` and `tests/templates_catalog.rs` agree.
6. Each template has a brief in `docs/template-briefs/`, kept in sync with it. Each platform number
   in a brief cites a source and date, or is marked a minuta choice.
7. `docs/template-briefs/README.md` lists `notes` and says any fields template may declare
   `chapters`.
8. Every changed or new template has the evidence `docs/conventions.md` asks for: runs on `Webinar 1`
   and `Clase 3.1`, with `ollama:gemma4:26b` and `claude:haiku`, read against the brief by an independent
   reviewer. The PR records what was run and found.
9. No output in the evidence runs invents a fact, computes a number, names a framework the input
   does not, ends mid-sentence, or puts a placeholder in `body`.

## Non-goals
- A `posts` field or per-post limits. `x-thread` stays Markdown and targets at most 240 characters
  per post, so a post can still exceed 280.
- Labels added by minuta in `<name>.md`, a `preview` field, or a `min_version` in the catalog.
- `threads`, Facebook and Reddit templates.
- Clip suggestions in `instagram` and `tiktok`, which read the summary and have no reliable times.
  `youtube-shorts` covers this.
- Channel boilerplate in YouTube descriptions. YouTube's default description holds it; `notes`
  reminds the user to add links.
- An AI-use disclosure line in `blog-post`.
- Changing `stamp` or the `chapters` limits for Spotify.

## Users & permissions
| Role | Can | Cannot |
|---|---|---|
| Local user | Pull templates, declare `notes` in their own fields templates | Use the new templates on a 0.4.0 binary |
| Repo contributor | Add a template using `notes` | Change curated text without the evidence run |

`notes` is untrusted model output, handled like `body`: normalized, written to files, never printed.
The publisher (sub-project 6) must never post it.

## Tenancy & localization
n/a for tenancy. Templates stay in English. Output follows `output_lang`. minuta adds no fixed text
to `notes`, so the model writes its labels in the output language, as in the example above.

## Domain model
**`notes`**: text field for the publisher, never part of the published text. It holds short labeled
lines. Templates ask only for what the model can derive from the input, plus an "add before
publishing" reminder for human-supplied items. This is the one allowed mention of missing items.

| Template | Input | Output | Fields | Limits that change | `notes` holds |
|---|---|---|---|---|---|
| youtube | summary | fields, plain | title, body, chapters, hashtags, tags, thumbnail_text, notes | see nutshell | pinned comment, 2 alternative titles, 2 alternative thumbnails, reminder |
| youtube-shorts (new) | transcript | fields, plain | title, body, hashtags, notes | title 100, body 500, hashtags max 3, notes 500 | clip start and end from transcript stamps, on-screen hook |
| instagram | summary | fields, plain | body, hashtags, thumbnail_text, notes | hashtags max 5, thumbnail_text 40, notes 500 | alt text, on-screen hook |
| instagram-carousel (new) | summary | fields, plain | thumbnail_text, notes, body, hashtags | hashtags max 5, notes 2500 | one line per slide, 5-10 slides; cover in `thumbnail_text`, caption in `body` |
| tiktok | summary | fields, plain | body, hashtags, thumbnail_text, notes | body 500, hashtags max 5, notes 300 | on-screen hook |
| linkedin | summary | fields, plain | body, hashtags, notes | hashtags max 3 and 80 chars, notes 500 | first comment, reminder |
| x-thread | summary | Markdown | n/a | target 240 per post | n/a |
| newsletter | summary | fields, Markdown | title, body, notes | notes 300 | preview text, alternative subject |
| blog-post | summary | fields, Markdown | title, body, tags, notes | tags max 5, notes 500 | meta description, slug, image alt text |
| podcast-notes | summary | fields, plain | title, body, chapters, tags, notes | body 2400, tags max 8 and 200 chars, notes 300 | alternative title, reminder |
| meeting-minutes | transcript | Markdown | n/a | n/a | n/a |
| action-items | transcript | Markdown | n/a | n/a | n/a |
| lesson (new) | transcript | Markdown | n/a | n/a | n/a |

Other template rules:
- `youtube`: body is a two-line hook, "what you'll learn" bullets, who it is for, and one call to
  action. Tags target 5-10; `notes` carries the pinned comment. The prompt bans `<` and `>`.
- `meeting-minutes`: decisions and action items come before a short discussion section; add date,
  next meeting and deferred items only when stated.
- `action-items`: a flat list sorted by owner, without the five-item grouping rule; add
  `(blocked by: ...)` only when stated.
- `lesson`: from the owner's `instructional-designer` prompt. Title, learning objectives, summary,
  core sections in teaching order, key concepts, common mistakes (only if discussed), key points,
  practice questions. Headings follow the output language. It fixes obvious speech-recognition
  errors in technical terms, and marks unclear passages instead of guessing.

Writing rules, from the research report's cross-cutting section: keep the grounding blocks and the
named bans; never ask the model for links, dates or bylines; put no conditional content in a declared
field; keywords come from the input; `limits:` replaces counting instructions; new text replaces old
instead of adding to it.

## Interfaces
Frontmatter gains one accepted value: `notes` in `fields` and `limits`. CLI commands do not change.
`templates list` and `templates pull` show the new templates.

## Behavior & edge cases
- A model that returns empty `notes` gets a repair call, as for any field.
- `chapters` fails on a transcript under 30 s, so `youtube` and `podcast-notes` fail there too, as
  the built-in does today. `youtube-shorts` declares no chapters.
- 0.4.0 parsing a file with `notes` fails the pull for the whole selection, naming the file, and
  writes nothing (existing all-or-nothing rule). Release notes say to upgrade first.
- The 3200 body limit keeps the pasted description under YouTube's 5000 bytes: 3200 + 1500 chapters
  + 80 hashtags + separators is about 4784 characters. The 480 tags limit absorbs the quotes
  YouTube counts around multi-word tags.
- `youtube-shorts` on a long recording relies on chunked map calls, which keep timestamps
  (`src/llm/chunk.rs:19`).

## Data & migrations
`<name>.json` gains an optional key, which is additive under ADR-0007. Pulled copies show as
*update available*. Edited copies stay *modified*. Existing job folders rerun only the changed
templates.

## Testing strategy
- Unit: `notes` name, kind, description, not filled, rendered in declared order, limits, empty.
- Fingerprint test with fixed values for criterion 2.
- Integration: a fake-backend run of a template with `chapters`, `hashtags` and `notes` asserts the
  whole `.md` and `.json`.
- `tests/templates_catalog.rs` covers the new files. The evidence run covers prompt quality.

## Decisions
- ADR-0012: `notes` joins the shared field set.
- Owner, 2026-10-30: use `notes`, keep `x-thread` in Markdown, send evidence transcripts to Anthropic only through `claude:haiku` run by a Haiku subagent, add `youtube-shorts`,
  `instagram-carousel` and a class template.
- Defaults taken: LinkedIn hashtags 1-3, TikTok `body` 500 and `hashtags` 5, podcast-notes plain with
  chapters, short-discussion minutes, flat action items, no minuta labels, preview text in `notes`,
  "add before publishing" reminders allowed in `notes`.
- CONTEXT.md: add Notes.

## FAQ
### Why not sections inside `body`?
`body` is what the publisher posts, so a pinned comment there would be published.

### Why one `notes` field and not `pinned_comment`, `preview` and others?
Each would grow a closed set that the publisher must support, for text nothing consumes yet.

### Why does the carousel put slides in `notes`?
The caption is the published text. Slides go into a design tool, and they fit the field's meaning:
content for the person publishing.

## Open questions
- Spotify documents `MM:SS` and `00:00`, and minuta writes `0:00`. This needs a real upload, which
  this work does not do. If Spotify rejects it, a later change adds a stamp format.

## Implementation status
- [ ] Plan: docs/plans/2026-10-30-templates-strategy.md
