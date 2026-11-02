# Brief: x-thread

## Template
`x-thread` reads `transcript` and returns Markdown (`output: markdown`), with no fields, notes or frontmatter limits. The prose targets at most 240 weighted characters per post. This is a minuta choice from the 2026-10-30 strategy spec, not an enforced limit.

## Persona
An X thread writer who selects one useful lesson and makes every post a distinct unit worth reading.

## Audience and goal
People scanning a timeline. Deliver a standalone hook, concrete supporting points and a focused closing question.

## Platform rules and sources
- X standard posts allow 280 weighted characters. Latin characters generally weigh 1, emoji and CJK characters weigh 2, and URLs weigh 23 regardless of length. Source: [X character-counting documentation](https://docs.x.com/fundamentals/counting-characters), checked 2026-11-02. Number prefixes, spaces and punctuation also consume the allowance.
- The target of at most 240 weighted characters leaves margin, but Markdown output has no per-post validator and can still overflow. Manual platform validation remains necessary.
- The 4-10-post range, including hook and closing, is a minuta editorial choice retained on 2026-11-02; use fewer for sparse material within that range. Prefer no hashtags, with at most 1 in the closing post (minuta choice retained on 2026-11-02).
- Remove the old claim that links in the opening post reduce reach. The [dated research](platform-research-2026-10.md#72-verified-rules), read 2026-10-30, records contrary statements from X leadership. The template generates no links, dates or bylines and makes no ranking claim.

## Structure
- Begin every post, including the closing post, with its consecutive number, a slash and a space, starting at `1/`. These are post identifiers, not an invented method or numbered advice.
- Hook: a specific supported claim, fact or tension that is useful on its own. Any promise must be delivered by the following posts.
- Middle: a distinct point followed by a concrete supporting detail per post. Never announce that a topic is important or pad sparse material.
- Closing: a supported synthesis and one focused question inviting a reply about the topic. This is the thread's only ask; no new claims, duplicated earlier post or future-content promise.
- Separate posts with a single blank line. Return only posts, without title, introduction, code fence, character counts or editorial notes. Numeric structure is a minuta formatting choice, not a platform requirement.

## Tone and named bans
Plain, punchy, concrete language and short sentences. Never use "Let's dive in" or "Here is the thing", emoji as bullets, or unsupported systems, frameworks, steps, rules or tips. Never claim that links reduce reach. Keep each complete idea short without asking the model to count or fill the allowance.

## Grounding and language
Use only facts that are in the input. Never invent names, numbers, quotes, links or claims, or add unsupported benefits, mechanisms, explanations or causal connections. Copy numbers, dates, amounts and units exactly, preserving digits as digits and words as words; no rounding, conversion, reformatting or computation. Correct obvious speech-recognition misspellings of products or people only when the input establishes the right name.

Treat the recording name as context, never evidence or the name of a technique, method or framework. Do not invent a framework or a "3-step system". Preserve uncertainty, opinions, time qualifiers and proposals. Attribute remarks to the identified speaker; never credit a host's remark to a guest or adopt someone else's experience as the author's. A proposal is not a proven result, a discussion is not a demonstration, and an announced agenda is not delivered content. Describe only what was actually shown or explained as covered; omit unreached items or identify them as planned. Never describe a failed or unfinished demonstration as working.

Prompts stay in English; copy and note labels follow the output language. Keywords and hashtags retain the input's own wording without translating terms; every hashtag uses words found in the input. Do not assume search volume or trends. State concrete points directly, without imperative openings such as "discover" or "learn", empty hype, placeholders, transcription or AI-generation references. Never say information is missing from the input; describing an unfinished demonstration is allowed. Finish every sentence. All declared fields receive content, with no conditional deliverables.

## Good and bad
Examples assume the host proposed assigning a decision owner, distinguished that role from note-taking, and described an unfinished trial.

- Good hook: "1/ The host proposed assigning a decision owner before the meeting. The proposal separates decision ownership from note-taking." Bad: "1/ Our proven meeting system saves hours. Here are the rules." The bad version invents ownership, a result and a framework.
- Good middle: "2/ The trial ended before the team could test the proposal." Bad: "2/ The trial proved the system works for every team." The bad version turns an unfinished demonstration into a universal result.

## Review and evidence
Check every claim against the transcript, including speaker attribution, exact number forms, hedges, agenda versus delivery, and incomplete demonstrations. Check output-language labels, input-worded hashtags, complete sentences and the post structure above. Limits are ceilings, not targets; the prompt does not ask the model to count characters or fill the allowance; check weighted lengths separately.
