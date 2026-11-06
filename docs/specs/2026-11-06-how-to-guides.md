# How-to guides — Spec

- Status: approved (decisions made by the maintainer in #343)
- Date: 2026-11-06
- Owner: Arnold Gandarillas Castillo
- Related: #343, [npm distribution](2026-11-03-npm-distribution.md) (the main package ships the user docs)

## TL;DR
- minuta gets four task-oriented how-to guides under `docs/guides/`, in English and Spanish, linked from both READMEs and shipped in the npm package.
- There is no GitHub Wiki. The guides live in the repository so that pull requests review them, the release tag pins them and the npm package carries them.
- The guides are how-to pages in the Diataxis sense: one goal per page, numbered steps for a newcomer. They link to `docs/templates.md` and `docs/reference.md` for reference material and never copy it.

## Goal
A newcomer who has installed minuta can finish each common task by following one page from top to bottom, without prior knowledge and without reading the source.

## The pages
Each page has an English file and a Spanish twin (`.es.md`) in `docs/guides/`.

| File | Goal |
| --- | --- |
| `README.md` | Index: what each guide is for and which one to read first |
| `custom-templates.md` | Add your own templates and override a built-in one, locally |
| `llm-and-whisper.md` | Set up an LLM (Ollama and other endpoints) and a whisper model |
| `fix-doctor.md` | Fix what `minuta doctor` reports, tool by tool |
| `worked-example.md` | One example from a recording to publishable meeting minutes |

Each page has the same shape: the goal in one sentence, what you need first, numbered steps with a copyable command and the result to expect, and an "If it did not work" line where a step is likely to fail.

## Rules
- **Verified.** Every command, flag, config key and output in a guide was run against the real CLI built from the same commit, or read from the source under `src/`. Nothing is invented. Output that varies (an LLM's wording, a tool's version) is marked as an example.
- **No duplication.** Anything already in `docs/templates.md`, `docs/install.md` or `docs/reference.md` is linked, not repeated.
- **Spanish policy.** Every page has a `.es.md` twin written as natural Spanish (second person, "tú", the register of the existing Spanish docs), not a literal translation. Commands, config keys, file names and tool output stay as they are; the prose around them is translated. The two versions have the same steps in the same order.
- **Shipping.** The files are added to the fixed list in `npm/scripts/build-packages.mjs`, so the main npm package ships them, with its test updated first. The link-check test keeps passing: relative links between shipped files stay relative and links to unshipped paths become GitHub URLs pinned to the release tag.
- **Index.** `README.md` and `README.es.md` link the guides index from their documentation section.

## Non-goals
- A GitHub Wiki, or a CI job that mirrors docs to one.
- Reference material (every flag, every key): that stays in `docs/reference.md` and `docs/templates.md`.
- Installation steps per operating system: they stay in `docs/install.md`.
- Workflow pages for podcasts, social posts or long-form content, and npm-versus-cargo upgrade notes. They were candidates in the issue and can become later guides.
- Changing the behavior of the CLI. If writing a guide shows a defect, it is filed as its own issue.

## Testing
- `npm/scripts/test/build-packages.test.mjs` lists the guides among the shipped files, checks that they are copied, and checks that every relative link in the built package resolves.
- `make ci` and `baton verify` pass.
- Review checks each command in the guides against the CLI.
