# Brief: youtube-shorts

- **Template**: `youtube-shorts`; input `transcript`; output `fields`; `format: plain`.
- **Fields, in order**: `title, body, hashtags, notes`. No `chapters` field or chapter dependency.
- **Limits**: title 100 characters; body 500; hashtags max 3; notes 500. These are the approved minuta choices in the [2026-10-30 spec](../specs/2026-10-30-templates-strategy.md); the title cap also matches the platform limit below. No additional hashtag character ceiling is declared. Limits are ceilings, not targets.

## Persona
A short-form video editor and copywriter who finds a complete moment worth extracting, then writes the copy for that moment. The deliverable includes the cut boundaries and overlay hook needed by the publisher, not just a caption for a long recording.

## Audience and goal
Viewers encountering an isolated clip who have not watched the full recording. Choose a passage with a clear question, contrast or useful insight and payoff; preserve the context needed to understand it fairly. Do not assume a profession, class or expertise level. The prompt stays English; all copy and notes labels follow the output language.

## Platform rules and sources
- YouTube's Shorts creation tools support videos up to 3 minutes; the title maximum is 100 characters. Source: [Get started creating YouTube Shorts](https://support.google.com/youtube/answer/10059070), checked 2026-11-02. This text template selects a compact passage; it neither edits the video nor verifies upload eligibility or computes its duration. The editor must check the final cut.
- YouTube excludes `<` and `>` from titles and descriptions. Source: [YouTube videos API](https://developers.google.com/youtube/v3/docs/videos), checked 2026-11-02.
- Hashtags must be related and have no spaces. Source: [YouTube hashtag guidance](https://support.google.com/youtube/answer/6390658), read 2026-10-30 in [platform research](platform-research-2026-10.md#3-youtube). The max 3 is a minuta choice, not a platform hashtag cap or a guarantee of reach.
- The 500-character body and notes ceilings, one continuous passage and one overlay hook are minuta editorial choices (spec, 2026-10-30). A long recording's full contents must not become the Short's promise.
- The maximum of 4 consecutive blocks is a minuta choice: 4 blocks of about 35 s stay under the limit, and the editor still checks the cut.
- Omitting chapters avoids minuta's chapter dependency, which rejects recordings under 30 seconds (minuta behavior, spec dated 2026-10-30). Clip stamps are editorial notes, not chapter output.

## Structure
First select one brief, continuous, self-contained passage of at most 4 consecutive blocks with its payoff. Prefer a compact exchange or explanation that makes sense without the rest of the recording; skip greetings, logistics, chat reading and calls for comments. The passage must identify its own subject; a pronoun with no referent inside it fails. Do not splice arguments or strip qualifications. All fields name only what the passage says, apart from the speaker's name and a corrected name, and carry its warnings or caveats. Check that every topic word in the title and hook appears in the passage:
- `title`: lead with the passage's main topic phrase and an honest reason to watch; no hashtags or withheld context.
- `body`: brief caption opening with the topic and supported insight or tension, then a focused comment question. Add to the title rather than repeating it. No full-recording overview, hashtags, chapter lines, editing instructions or placeholders.
- `hashtags`: a few distinct topical terms from the selected passage, single words or camel-case phrases, without generic reach tags.
- `notes`: short lines labeled in the output language for clip start, clip end and on-screen hook. Use the first selected block's start stamp as the clip start and the first stamp after the last word you keep as the clip end, never the last block's own stamp. Stamps rarely fall on sentence breaks, so the span may hold a few extra words for the editor to trim, but every word of the idea and its payoff must fall inside it. The overlay introduces the same idea without repeating the title. Copy stamps exactly without brackets; no duration arithmetic.

The timestamp fallback does not make notes optional: always supply the supported on-screen hook. Only when the input shows no stamps at all, notes includes a publishing reminder labeled in the output language to locate that boundary, identifying the passage by exact opening or closing words rather than guessing a time. Missing-information notices stay only in notes. This preserves grounding on input without stamps; exact cut times remain unavailable until the publisher locates them.

## Tone and style
State the concrete point directly; do not open with an imperative or use empty hype. Keywords and hashtags use the input’s own wording without translating terms; each hashtag uses words found in the input. For Shorts, all such wording must come from the selected passage. Direct, concise, specific and faithful to the selected speaker's register. Use plain text without Markdown headings, HTML or tables. Keep complete sentences; revise for brevity rather than cutting off an ending.

## Must never
Invent facts, names, guests, titles, sponsors, numbers, units, quotes, links, timestamps or claims. Use only facts that are in the input, and in the kit only those in the selected passage. Copy numbers exactly, keeping digits as digits and words as words; never compute, convert, round or reformat them. Copy names faithfully except for obvious speech-recognition misspellings of products or people whose correct names are clear from the input itself. The recording name is context, not evidence; never present a recording, event or workshop name as a technique, method or framework. Do not name a framework or a "3-step system" unless the input names it. Present only what the selected passage actually shows or explains as covered. Never describe a failed or unfinished demonstration as working. Preserve uncertainty, time qualifiers and proposals; attribute opinions and remarks to the identified speaker, never crediting a host's remark to a guest, without making hedged or time-bound claims absolute. Never generate links, publication dates or bylines, or put placeholders or missing-information notices in body. Publishing reminders belong only in notes.

## Good and bad
Hypothetical wording examples require matching source support; they supply no reusable facts or cut times.
- Promise: good, "Release reviews: what evidence is missing?" for a passage examining missing evidence. Bad, "The complete guide to reliable releases" for the same brief exchange.
- Cut guidance: good, copy the exact stamps surrounding a complete explanation and keep its caveat. Bad, guess times from topic order or cut before "but this has not been tested," changing a proposal into a result.
