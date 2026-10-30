# minuta templates: what a strategist would deliver, per platform (late 2026)

Research date: 2026-10-30. All sources were read on that date. Written for a template writer who has not seen the conversation that produced it. The "owner decides" items below are settled in `docs/specs/2026-10-30-templates-strategy.md`, which wins where the two differ.

## 0. How to read this report

**Background.** minuta turns a recording into a transcript, a summary and then platform copy, through prompt templates in `templates/*.md` (catalogued in `templates/index.toml`), each with a brief in `docs/template-briefs/<name>.md`. The owner's complaint: "the template for youtube before was putting the chapters of the video and now it's just missing and the description is quite poor, only text, no hashtags, no tags, please be more strategic at the moment of refining and creating the templates and provide everything is needed for each social network". Already diagnosed: curated `templates/youtube.md` dropped the `chapters` field that `src/template/builtin/youtube.md` declares, and never declared `hashtags`.

**Evidence labels**
- **[O]** official platform source.
- **[O-snippet]** official page, seen only as a search-result excerpt because the page itself blocked fetching.
- **[S]** secondary reporting of a fact or a platform statement.
- **[P]** practice: what practitioners or vendors say. Not a platform fact. Vendor numbers vary and most are not reproducible.

**Brief-number verdicts:** CONFIRMED, STALE (current value given), UNVERIFIABLE (no primary source found; usually fine as a rule of thumb, but do not state it as a platform fact), INTERNAL (a minuta choice, not a platform number).

**Limits of this research.** Page reads went through a fetch tool that returns a model-made extract, so short quotes below are close to verbatim but re-check any quote before pasting it into docs. Blocked or empty: `help.x.com` (403), `reddit.com` and `redditinc.com`, `help.instagram.com` (empty page), TikTok Creator Academy and TikTok Support (JavaScript shell), the LinkedIn Engineering blog (400), MediaPost (403). Instagram and TikTok therefore have thinner official evidence than YouTube, Google and Apple. Nothing here has been tested on a real upload.

---

## 1. Top-level findings

1. **The damage is broader than YouTube.** Only `youtube` declares `thumbnail_text`. No template provides cover text, alt text, a pinned comment, preview text, a meta description or chapters. Today every template produces copy, not a publishing kit.
2. **`docs/template-briefs/README.md` line 20 is wrong and probably caused the YouTube regression.** It says "`chapters` is reserved for the template named `chapters`". ADR-0008 and `src/template/mod.rs` allow any fields template to declare the filled `chapters` field. Only `output: chapters` is exclusive to that template. `docs/template-briefs/youtube.md` then says "minuta's `chapters` template handles them", which reads as if nothing has to be declared. Fix both, or the regression can return.
3. **Rendered `<name>.md` has no labels** (`src/fields/render.rs`). `youtube.md` would be a title, a description, chapters, then `a, b, c`, then a line of thumbnail words, with nothing saying which is which. Labeled sections in `body` are not a free fix either: `body` is what the future YouTube publisher will send as the description (ADR-0006, ADR-0007), so a pinned comment inside `body` would be published in the description.
4. **Every declared field must be non-empty** (`src/fields/validate.rs`: `"<name>" is empty` is a violation). A declared field forces the model to answer. Never declare a field the recording may not support, such as links or guest bios. This is the main risk of richer deliverables for grounding.
5. **Stale or wrong platform facts in the briefs:**
   - X: "links in the first post reduce reach" (X leadership said in July 2026 there has been no penalty for over a year).
   - TikTok: `hashtags max 6` (a 5-hashtag in-app cap is widely reported since mid-2025; not officially documented).
   - YouTube: first hashtags are not simply "the first three"; YouTube now says up to three "considered most engaging".
   - Podcast: "simple Markdown render[s] everywhere" (Apple documents only `p ol ul li a`; `##` would paste literally).
   - LinkedIn: the 210 character fold (planning figure is about 140 on mobile).
   - Blog: FAQ rich results are gone (removed from Google Search in May 2026), so no one should add an FAQ for that reason.

### Facts about minuta used throughout (read from the repo)
- Allowed fields: `title`, `body`, `tags`, `hashtags`, `thumbnail_text`, and filled `chapters` (`src/fields/mod.rs`).
- Fields render in declared order, separated by a blank line, with no labels. `hashtags` renders `#a #b`; `tags` renders `a, b`.
- `chapters` rendering: `format: plain` gives `0:00 Intro`, `format: markdown` gives `- 0:00 Intro`. Stamps are `m:ss` below one hour, `h:mm:ss` above (`src/chapters/mod.rs`).
- Declaring `chapters` adds a graph dependency and one more LLM call, and fails on transcripts under 30 s (`src/generate.rs`). `limits:` on `chapters` is rejected; its limits live in `src/template/builtin/chapters.md` (`max: 20`, `max_chars: 1500`). A template cannot declare only `chapters`.
- `max_chars` measures the rendered field; `max` counts list items; violations trigger repair calls, default 2 (ADR-0006). There is no per-item limit.
- Adding a field is additive under ADR-0006, needs a Rust change and an ADR. A filled field needs its own ADR (ADR-0008).
- `input:` can be another template, which reads its `<name>.md`, so a chained template can add extras without Rust, at the cost of one more call and one more output file.
- Changing curated text needs evidence: `docs/conventions.md` line 33 (at least two recordings, a small local model and a stronger one, independent review). `index.toml` holds a sha256 for each template.

---

## 2. Decision on a shared mechanism (needed before per-template work)

Most missing pieces are the same kind of thing: "useful to the publisher, must not be pasted into the main text". Options:

| Option | How | Cost | Verdict |
|---|---|---|---|
| A. New text field `notes` | One optional-looking block rendered last, short labeled lines: `Pinned comment: ...`, `Alt titles: ...`, `Meta description: ...` | Rust + ADR. Additive. Must be asked of the model, so non-empty | **Recommended.** One call, one output, one concept for all templates |
| B. Chained template per platform (`youtube-extras`, `input: youtube`) | Markdown, no Rust | One more LLM call, one more file, catalog doubles | Good fallback if the owner refuses new fields |
| C. Labeled sections inside `body` | No Rust | Pollutes pasteable text and the publisher's `body` | Only for templates whose body is a document (Markdown outputs) |
| D. Separate named fields (`preview`, `meta_description`, `pinned_comment`, ...) | Rust + ADR for each | Grows the closed set. Machine-readable for a future publisher | Consider only `preview` (see below) |

Possible second field: **`preview`** (text), reused as newsletter preview text, blog meta description and podcast short summary. Worth it only if a publisher will consume it. Otherwise put it in `notes`.

Possible third field: **`posts`** (list with a per-item char limit, rendered one per paragraph), for `x-thread`, Threads and carousels. It is the only way to enforce 280 characters per post; today this is unenforceable and local models ignore lengths (spec `2026-10-25-fields-and-social-templates.md`). Needs Rust: a new limit kind and a new render rule.

**Labels.** Also decide whether `<name>.md` gains labels for non-body fields, such as `Tags: ...`, `Thumbnail text: ...`. It touches paste-ready rendering (ADR-0007), `render.rs` golden tests and `FIELDS_VERSION`. `<name>.json` is unaffected.

---

## 3. youtube

### 3.1 Complete deliverable
| Piece | Priority | Notes |
|---|---|---|
| Title | Must | Main phrase first; promise the video keeps |
| Description: first 1-2 lines | Must | Clear statement of the video with 1-2 main keywords [O] |
| Description: body | Must | What is covered, in the viewer's search vocabulary |
| Chapters | Must | Filled by minuta, placed after the body |
| Hashtags | Must | 3 (at most 5), in the description, not the title |
| Tags | Must, low value | The owner asked for them. Real value is mainly misspellings and variants [O] |
| Thumbnail text | Must | 2-5 words, complements the title |
| Comment prompt / pinned comment | Nice | A question tied to the video; to be pinned [O: pinning exists] |
| 2 alternative titles | Nice | Test & compare takes up to 3 [O] |
| Links block (subscribe, socials) | Nice, not the model's job | Set once in YouTube's default description [O]. Never generated |
| End-screen CTA line | Nice | [P] |

### 3.2 Verified rules
| Rule | Value | Source (freshness) |
|---|---|---|
| Title | 100 characters; no `<` or `>` | [O] https://developers.google.com/youtube/v3/docs/videos (updated 2026-10-15) |
| Description | **5000 bytes** (API), "5,000 characters" (Help); no `<` or `>` | same; https://support.google.com/youtube/answer/12948449 (undated) |
| Tags | 500 characters total; commas count; a tag with a space counts 2 more for implied quotes | same API page |
| Tag value | "minimal role" in discovery; most useful when a subject is commonly misspelled | [O] https://support.google.com/youtube/answer/146402 (undated) |
| Hashtags | Title or description. Up to three "considered most engaging" appear by the title. Over 60: all ignored. No spaces. Must relate to the content | [O] https://support.google.com/youtube/answer/6390658 (©2026, undated) |
| Chapters | At least 3, ascending, first at 00:00, each at least 10 s | [O] https://support.google.com/youtube/answer/9884579 (undated) |
| Description format | Bold/italic/strikethrough, `*`, `+`, `-` bullets; link playlists; credit collaborators | [O] answer/12948449 |
| Thumbnail | 16:9, up to 3840x2160, JPG or PNG; up to 50 MB desktop, 2 MB mobile | [O] https://support.google.com/youtube/answer/72431 (undated) |
| Test & compare | Up to 3 titles/thumbnails, winner by watch time, desktop Studio, advanced features, no Shorts | [O] https://support.google.com/youtube/answer/13861714 (undated) |
| Search ranking | Relevance (title, tags, description, content), engagement, quality | [O] https://support.google.com/youtube/answer/16090438 (undated) |
| Clickbait policy | Maliciously misleading titles, thumbnails, descriptions violate policy | [O] https://support.google.com/youtube/answer/2801973 (undated) |

### 3.3 Brief numbers
| Brief claim | Verdict |
|---|---|
| Title 100 | CONFIRMED |
| About 60 show | UNVERIFIABLE. [S] 50-70 depending on surface; about 50 is safer on mobile |
| Description 5000 | CONFIRMED, but bytes in the API. Multi-byte text (Spanish, emoji) counts more |
| 120-150 before "Show more" | UNVERIFIABLE. [S] 100-160. Put the point in the first 100 |
| Tags total 500 | CONFIRMED with the counting caveat. minuta measures `a, b` (2 per separator), API measures commas plus 2 per multi-word tag. 15 multi-word tags measure 16 more in the API than in minuta |
| "Tags matter less" | CONFIRMED, and YouTube says it more strongly |
| Thumbnail 2-5 words | UNVERIFIABLE, [P] |
| `body` 3450 | INTERNAL, too high once hashtags are added (below) |

### 3.4 Strategy (as model instructions)
- Choose one main search phrase from the input. Put it at the start of the title and in the first sentence of the description. [O for placement of 1-2 keywords; P for "at the start"]
- The title makes the promise; the thumbnail text adds what the title does not say. Never restate the title. [P]
- Descriptions are unique per video, in words a viewer would type. Do not list keywords. [O]
- Do not put hashtags in the title. [P] Keep to 3: only up to three show by the title, and YouTube chooses them, so more adds little. Do not rely on order.
- Test & compare picks by watch time, not clicks, so the promise must hold. [O] Accuracy is a ranking interest as well as a grounding rule.
- Tags: exact main phrase, variants, common misspellings of names in the input. 5-10 is plenty. [P]

### 3.5 Gap
- No `chapters`, no `hashtags` (known).
- Prompt forbids timestamps but does not say chapters follow.
- No instruction to avoid `<` and `>`.
- Tags instruction demands 8-15, more than YouTube's own "minimal role" justifies, and invites near-duplicates from a small model.
- `Good thumbnail text: "HALF THE TIME"` in the brief conflicts with the template's "Never use all caps". Pick one; caps on thumbnail text are common [P].
- `body` ends in a comment invitation, which is fine, but no pinned-comment output and no alternative titles exist.

### 3.6 Delivery in minuta
```yaml
fields: [title, body, chapters, hashtags, tags, thumbnail_text]   # add notes if approved
limits:
  title: { max_chars: 100 }
  body: { max_chars: 3200 }
  hashtags: { max: 5, max_chars: 80 }
  tags: { max: 15, max_chars: 480 }
  thumbnail_text: { max_chars: 40 }
format: plain
```
- This order gives body, chapters and hashtags contiguously, so they paste as one description.
- Math: 3200 + 1500 (chapters) + 80 + separators = 4784 chars. At about 3% multi-byte that is about 4930 bytes, under 5000. The current 3450 would reach 5034 chars.
- `tags max_chars 480` absorbs the quote difference for 15 multi-word tags.
- Pinned comment, alternative titles: `notes` or chained `youtube-extras`. **Owner decides.** Not in `body`.
- A channel links block: keep out of templates; point the user to the default description.
- Fix the same field order in the built-in `src/template/builtin/youtube.md`, which also lacks `hashtags` (Rust build includes it).

---

## 4. instagram

### 4.1 Complete deliverable
| Piece | Priority | Notes |
|---|---|---|
| Caption with hook | Must | Understandable alone |
| 3-5 hashtags | Must | Labels, not reach |
| Cover text | Must for Reels | Few words; use `thumbnail_text` |
| Keyword sentence in caption | Must | Natural language, early |
| One CTA aimed at sends | Must | "Send this to..." : inference from Mosseri naming sends as the stronger signal for unconnected reach [S]; no test shows it beats a question |
| Alt text | Nice | Images only |
| Which moment to clip | Nice, high value | See cross-cutting rule 8 |
| On-screen hook line | Nice | Overlay on the first seconds [P] |
| Location, collaborators, audio | Skip | Not knowable from a recording |

### 4.2 Verified rules
| Rule | Value | Source (freshness) |
|---|---|---|
| Caption length | 2200 characters | [O] https://developers.facebook.com/documentation/instagram-platform/instagram-graph-api/reference/ig-user/media (latest dated change on the page 2025-07-09) |
| Hashtags | Instagram said it will "gradually update" the limit in a reel or post caption to five | [S quoting Instagram] https://www.socialmediatoday.com/news/instagrams-implementing-new-limits-on-hashtag-use/808309/ (2025-12-18). The Meta API page still says 30: treat 5 as the rule |
| Alt text | 1000 characters, images only, not Reels or Stories | [O] same Meta page |
| Carousel | Up to 20 items | [O] same Meta page |
| Hashtags and reach | Mosseri: hashtags "don't work" to raise reach; useful to say what a post is about | [S] socialmediatoday link above (2025) |
| Reels signals | "watch time, likes and sends"; sends slightly more important for unconnected reach | [S quoting Mosseri] https://keywordseverywhere.com/news/instagram-algorithm-updates/how-the-instagram-algorithm-works/ (Jan 2025) |
| New-creator metric order | Sends per reach, likes per reach, watch time | [S] https://kabo.sh/guides/instagram-ranking-2026 (reports a 2026-10-28 interview) |
| Originality | Listed as a ranking category in Sept 2026 | [S] same |
| Reels reduced for | Low resolution, watermarks, muted audio, borders, mostly text, already posted on Instagram | [O] https://about.instagram.com/blog/announcements/instagram-ranking-explained (2023-05-31; old) |
| Search | Keywords and hashtags must be in the caption, not comments | [S] https://www.socialmediatoday.com/news/instagram-outlines-how-its-search-algorithms-work-and-how-you-can-optimize/605596 (2021; old) |
| Web search | Public posts of professional accounts indexable since 2025-07-10 | [S, consistent] https://ppc.land/instagram-content-becomes-searchable-on-google-starting-july-10/ |

### 4.3 Brief numbers
| Brief claim | Verdict |
|---|---|
| Caption 2200 | CONFIRMED |
| 5 hashtags since Dec 2025 | CONFIRMED by Instagram's statement as reported; official API page lags |
| First ~125 chars before "more" | UNVERIFIABLE; widely repeated [P] |
| No clickable links in captions | UNVERIFIABLE here (Help Center unreadable); long-standing and safe to keep |
| `body` 1800, `hashtags` 100 | INTERNAL; both fit |

### 4.4 Strategy
- Write for a send: name a specific person the post would help. CTA order: send, then save, then question. [S/P]
- First line: the subject in plain searchable words plus the hook. Not a teaser. [P]
- Hashtags describe the topic. 3-5, never reach tags. [S]
- Warn in the output, not the prompt, that recycled clips with watermarks are reduced. [O, 2023]

### 4.5 Gap
- Template says "feed or Reels" but provides no cover text.
- CTA treats save, share and question as equal; current evidence ranks sends first.
- Nothing about keywords.
- Nothing states what is posted (Reel or still). The caption assumes the reader has a video.

### 4.6 Delivery
`fields: [body, hashtags, thumbnail_text]`, `thumbnail_text` max 40, `format: plain`. Cover text fits the existing field description ("A few words to show on a thumbnail image"). Alt text and clip moment go in `notes`. No new field needed for the core. `chapters`: no.

---

## 5. tiktok

### 5.1 Complete deliverable
| Piece | Priority | Notes |
|---|---|---|
| Caption: hook plus keyword line | Must | Natural search phrase |
| Comment prompt | Must | Specific question |
| 3-5 hashtags | Must | Topic, not `fyp` |
| Cover text | Must | `thumbnail_text` |
| On-screen hook text | Nice | Overlay for the first seconds [P] |
| Clip moment | Nice, high value | Which part of a long recording |
| Sound, trend | Skip | Needs live trend data; would invite invention |

### 5.2 Verified rules
| Rule | Value | Source (freshness) |
|---|---|---|
| Caption (API) | 2200 UTF-16 runes for video Direct Post | [O] https://developers.tiktok.com/doc/content-posting-api-reference-direct-post (updated 2026-10-27) |
| Caption (app) | 4000 claimed | [S, unconfirmed] https://typecount.com/blog/tiktok-caption-character-limit; conflicting guides |
| Photo posts | Title 90, description 4000 runes, up to 35 images | [O] https://developers.tiktok.com/doc/content-posting-api-reference-photo-post (updated 2026-10-27) |
| Hashtag cap | "Maximum 5 hashtags" warning reported since about Jul-Aug 2025. No official documentation; inconsistent enforcement; some report 7 | [S] https://www.seoteric.com/tiktok-limits-hashtags-to-five-per-post-what-it-means-for-creators-and-marketers/, https://www.pod2reels.com/tools/tiktok-hashtag-limit |
| Hashtag parsing | Recognized when separated by spaces or line breaks | [O] Direct Post reference |
| Recommendation inputs | Interactions, video information (captions, sounds, hashtags), device settings (lowest weight); finishing a longer video is a strong signal | [O] https://newsroom.tiktok.com/en-us/how-tiktok-recommends-videos-for-you (2020-06-18; old) |
| Follower count | Not a direct factor | [O-snippet] same |
| Creator Search Insights | Shows topics people search for | [S] https://www.socialmediatoday.com/news/tiktok-adds-insights-better-inform-content-strategies/710242/ |
| Search keywords | Keyword aligned across caption, on-screen text and speech | [P] several vendor guides; no official source found |

### 5.3 Brief numbers
| Brief claim | Verdict |
|---|---|
| "allows up to 2200" | CONFIRMED for the API; possibly STALE for the app (4000 unverified). Harmless given our ceiling |
| Only first line or two show | UNVERIFIABLE |
| `hashtags max 6` | **STALE in practice: use 5** |
| `body` 300 | INTERNAL, too tight |

### 5.4 Strategy
- Search is a real entry point. Use one plain phrase a viewer would type, in the first caption line. [P]
- Caption adds context, does not narrate. Keep. [P]
- Ask a question viewers can answer from experience. Keep.
- 3-4 specific hashtags. Staying at or under 5 is safe whatever the real cap is.

### 5.5 Gap
- Search is absent from the template.
- 6 hashtags can be rejected at upload.
- 300 chars forces cuts, and a caption cut mid-sentence is a logged failure (`docs/conventions.md`). Raising the ceiling costs nothing.
- No cover text.

### 5.6 Delivery
`fields: [body, hashtags, thumbnail_text]`, limits `body 500`, `hashtags { max: 5, max_chars: 100 }`, `thumbnail_text 40`. Clip moment in `notes`. No `chapters`. Note the template's input is `summary`, which may have lost timestamps, so a clip suggestion needs `input: transcript` or a summary that keeps stamps. **Owner decides** (rule 8).

---

## 6. linkedin

### 6.1 Complete deliverable
| Piece | Priority |
|---|---|
| Post body with a hook inside the fold | Must |
| 1-3 hashtags | Nice (label only) |
| Closing question | Must |
| First comment text (with link if the user has one) | Nice |
| Carousel/document outline | Nice; better as a separate template later |

### 6.2 Verified rules
| Rule | Value | Source (freshness) |
|---|---|---|
| Post | 3000 characters | [O] https://www.linkedin.com/help/linkedin/answer/a528176 (undated) |
| Automation | Excessive posting or automation can limit visibility | [O] same |
| "See more" fold | About 140 mobile, about 210 desktop; depends on lines and media; no official number | [S] 2026 guides in search results, for example https://linkedinpreview.com/blog/linkedin-hook-preview, https://contentin.io/blog/linkedin-post-character-limit/ |
| Hashtag following | Reported removed late 2024; hashtags act as topic labels | [S] https://taplio.com/blog/linkedin-hashtags |
| Links | Vendors report 18-60% less reach; LinkedIn has not confirmed | [S] https://upgrowth.in/linkedin-algorithm-2026-360brew-update/ |
| Dwell time | Described as confirmed by LinkedIn since 2020 | [S] same; primary engineering post could not be read |
| Comments, early velocity, carousels | Repeated across vendors | [P] https://www.socialpilot.co/blog/linkedin-algorithm |
| AI citations | Mid-length posts (50-299 words) are the largest share among cited feed posts; articles do better overall | [S] https://www.semrush.com/blog/linkedin-ai-visibility-study/ |

### 6.3 Brief numbers
| Brief claim | Verdict |
|---|---|
| 3000 | CONFIRMED |
| About 210 before "see more" | STALE as a single number: plan for about 140 |
| 3-5 hashtags "are enough" | UNVERIFIABLE; 1-3 is the safer practice |
| "Links in the body reduce reach" | UNVERIFIABLE; [P], keep as guidance, not fact |
| `body` 2800 | INTERNAL. 2800 + 2 + 120 = 2922 fits |

### 6.4 Strategy
- The complete hook fits within 140 characters. Avoid an early blank line, since breaks move the fold. [S]
- 150-300 words is usually enough. The 2800 limit is a ceiling, not a target. [P]
- One lesson, one concrete example, one real question. The template already does this well.
- Do not state in copy that LinkedIn penalizes links. Omit links because none exist.

### 6.5 Gap
Fold number, hashtag count, and no target length (local models tend to fill). Strongest template of the set otherwise.

### 6.6 Delivery
`fields: [body, hashtags]`, `hashtags { max: 3, max_chars: 80 }`. Since `hashtags` cannot be empty, this means at least one. **Owner decides** between 1-3 and dropping the field. Given the complaint, keep it. First comment in `notes`.

---

## 7. x-thread

### 7.1 Complete deliverable
| Piece | Priority |
|---|---|
| Hook post, standalone | Must |
| 3-8 body posts, one idea each | Must |
| Closing post with takeaway and one ask | Must |
| Media suggestion for the hook | Nice |
| 0-1 hashtag | Nice |
| Standalone single-post version of the hook | Nice |

### 7.2 Verified rules
| Rule | Value | Source (freshness) |
|---|---|---|
| Post | 280 weighted characters | [O] https://docs.x.com/fundamentals/counting-characters (undated) |
| Weights | Emoji 2, any URL 23, CJK 2, Latin 1 | [O] same |
| Hashtags | "no more than 2 hashtags per post" recommended; numeric-only tags do not link | [O-snippet] https://help.x.com/en/using-x/how-to-use-hashtags |
| Link penalty | Bier: "you do not need to put the links in replies anymore." Musk: "We haven't for over a year." | [S] https://www.freepressjournal.in/tech/x-product-head-nikita-bier-confirms-link-penalty-removed-over-a-year-ago-tells-mark-zuckerberg-he-can-post-them-directly (2026-07-29) |
| Algorithm code | Published 2026-01-20, repo `xai-org/x-algorithm`; predicts engagement actions | [S] https://ppc.land/how-xs-algorithm-silently-kills-your-links-without-explicitly-penalizing-them/ (2026-01-21). Its "links are demoted" claim is inference, not code |
| Mutual follows | Added to ranking July 2026 | [S] https://opentweet.io/x-algorithm-changes |
| Long posts | Premium tiers: sources conflict (4000 vs 25000) | [S] unresolved |
| Thread length limit | Not found | unresolved |

### 7.3 Brief numbers
| Brief claim | Verdict |
|---|---|
| 280 counting links and spaces | CONFIRMED but incomplete: weights above |
| Aim for 220-260 | INTERNAL/[P] |
| 5-10 posts | UNVERIFIABLE [P]. Template says 4-10, so brief and template disagree |
| **Links in the first post reduce reach** | **STALE.** Contradicted by X leadership (July 2026) |
| 0-1 hashtag | CONFIRMED as within X's guidance (stricter) |

### 7.4 Strategy
- Hook post stands alone. [P]
- Replies are a predicted action, so the closing question should be easy to answer from experience. [S/P]
- No evidence was found that threads beat long posts or the reverse. Do not claim it.

### 7.5 Gap
- 280 is prompt-only, and a model cannot count. Highest structural gap of any template.
- Stale link sentence ("because they reduce reach").
- Emoji/URL weights absent.

### 7.6 Delivery
Today: stay Markdown, target at most 240 per post so overflow is rare, and state weights. Better: new `posts` list field with a per-item limit (Rust + ADR). **Owner decides.** `chapters`: no.

---

## 8. newsletter

### 8.1 Complete deliverable
| Piece | Priority |
|---|---|
| Subject line | Must |
| Preview text, separate from the first sentence | Must |
| Body | Must |
| One CTA | Must, with no invented link |
| Alternative subject for A/B | Nice |
| Sign-off | Nice |
| Unsubscribe, address | Handled by the sending service, not the template |

### 8.2 Verified rules
| Rule | Value | Source (freshness) |
|---|---|---|
| Subject | At most 9 words and 60 characters; at most 1 emoji (research guidance) | [S, sending-service guidance] https://mailchimp.com/help/best-practices-for-email-subject-lines/ |
| Preview text | Keep 30-80 characters; appears beside the subject; if absent, opening text fills it | [S] https://mailchimp.com/resources/email-preheader/ |
| Gmail sender rules | Accurate, non-deceptive headers and content; no fake `Re:`/`Fwd:`; one-click unsubscribe above 5000/day; spam rate below 0.3% | [O] https://support.google.com/a/answer/81126 (undated) |
| Clipping | About 102 KB, from sending-service docs, not Gmail | [S] https://www.palisade.email/learning/email-questions/why-did-my-email-get-clipped-in-gmail. Irrelevant at 4000 characters |
| AI summaries | Gmail shows summaries on long threads; effect on preview text is unmeasured; Apple preview replacement is a vendor claim | [S] https://www.eweek.com/news/google-gmail-ai-summaries-global-rollout/, https://www.mailpro.com/blog/ai-inbox-problem-email-marketing |

### 8.3 Brief numbers
| Brief claim | Verdict |
|---|---|
| Over ~50 cut on phones | UNVERIFIABLE [P]; keep the point first |
| `title` 60 | CONFIRMED against Mailchimp guidance (not a hard limit) |
| First sentence shows as preview | CONFIRMED, but only as fallback; a written preview is better |
| Plain and light Markdown survive everywhere | UNVERIFIABLE [P] |
| `body` 4000 | INTERNAL |

### 8.4 Strategy
- Subject states the point; preview adds what the subject lacks. [P, consistent with Mailchimp]
- Treat open rates as noisy; write for the reply or the click. [P]
- Keep the key point in the first lines, since summaries may be all that is read. [P]

### 8.5 Gap
Preview text is the single clearest missing piece. The template's single "CTA" cannot include a link, so "one action" usually degrades to a question. Acceptable, but state it.

### 8.6 Delivery
`fields: [title, body]` plus `preview` or `notes`. Fallback without Rust: first line of `body` as `Preview: ...`, which the user must delete, so prefer the field. **Owner decides.** `format: markdown` stays. `chapters`: no.

---

## 9. blog-post

### 9.1 Complete deliverable
| Piece | Priority |
|---|---|
| Title (H1) | Must |
| Article | Must |
| Meta description | Must |
| URL slug | Must, trivial |
| Byline, date | Human adds; template must not invent |
| Image alt text / image idea | Nice |
| Internal links | Human adds; the model cannot know them |
| Tags (site taxonomy) | Nice |
| Video embed note, link to the recording | Nice; supports first-hand evidence [P] |
| AI-use disclosure | Owner decision |

### 9.2 Verified rules
| Rule | Value | Source (freshness) |
|---|---|---|
| Title | No limit; truncated to device; descriptive, concise, no stuffing | [O] https://developers.google.com/search/docs/appearance/title-link (2025-12-10) |
| Meta description | No limit; truncated as needed; unique, summary, no keyword lists; snippets mostly come from page content | [O] https://developers.google.com/search/docs/appearance/snippet (2026-04-20) |
| Meta keywords | Not used by Google | [O] https://developers.google.com/search/docs/fundamentals/seo-starter-guide (2025-12-10) |
| Heading order, length, URL keywords | "Don't focus on" list | [O] same |
| Alt text | Short, descriptive | [O] same |
| Helpful content | Clear byline where expected; no preferred word count; writing to a length is a warning sign; disclose automation where readers would ask | [O] https://developers.google.com/search/docs/fundamentals/creating-helpful-content (2026-10-05) |
| AI Overviews/AI Mode | No special optimization or markup | [O] https://developers.google.com/search/docs/appearance/ai-features (2025-12-10) |
| FAQ rich result | Gone: "will no longer appear in Google Search starting May 7, 2026"; docs removed 2026-06-15 | [O] https://developers.google.com/search/docs/appearance/structured-data/faqpage and changelog |
| Article markup | `headline`, `author`, `datePublished`, `dateModified`, `image`; no required props | [O] https://developers.google.com/search/docs/appearance/structured-data/article (2026-09-08) |
| Scaled content abuse | Mass-produced pages with no added value | [O] https://developers.google.com/search/docs/essentials/spam-policies (2026-08-28) |

### 9.3 Brief numbers
| Brief claim | Verdict |
|---|---|
| Good title is 50-60 chars | UNVERIFIABLE as a rule; Google states no limit. Keep as [P] |
| Subheadings every 150-250 words | UNVERIFIABLE [P]; Google says no ideal number of headings. Cut from the prompt, since it adds counting |
| Tags "topical keywords" | Misleading if read as SEO: Google ignores them |

### 9.4 Strategy
- Answer first, then evidence; the speaker's first-hand details are the differentiator. [O in spirit: people-first, experience]
- Headings state ideas. [P]
- Do not add FAQ sections for rich results. Do not chase a word count.

### 9.5 Gap
No meta description. Tags given more weight than they deserve. Word-interval rule makes local models count. The template's anti-invention text is strong; keep it.

### 9.6 Delivery
`fields: [title, body, tags]` plus `preview` (meta description) or `notes` (meta description, slug, alt text). Do not put a meta description at the top of the article. `tags max 5`. Stays `format: markdown`. `chapters`: not useful in a text article, unless the post embeds the video.

---

## 10. podcast-notes

### 10.1 Complete deliverable
| Piece | Priority |
|---|---|
| Episode title | Must |
| Hook, 2-3 sentences | Must |
| Topic bullets | Must |
| Chapters | Must |
| Guests and mentions | Conditional on the input |
| Key takeaways | Nice |
| Tags | Nice; host/site only |
| Follow CTA, sponsor, transcript link | Human or host |

### 10.2 Verified rules
| Rule | Value | Source (freshness) |
|---|---|---|
| Title | Brief, focused; no episode number, show name or date; unique | [O] https://podcasters.apple.com/support/867-featured-present-your-podcast, https://podcasters.apple.com/support/1993-optimize-your-request-for-promotion, https://help.apple.com/itc/podcasts_connect/en.lproj/itcb54353390.html |
| Episode description | **Apple conflict:** RSS guide says 10,000 characters; subscriptions page says 4000 | [O] RSS guide above; https://podcasters.apple.com/support/1236-subscription-best-practices |
| HTML | `p ol ul li a` in CDATA | [O] RSS guide |
| Keywords | In the description; no dedicated keyword tag in Apple's guide; keyword lists in a show title can get a show removed | [O] same; 867 page |
| Metadata accuracy | Must represent the content | [O-snippet] https://podcasters.apple.com/support/891-content-and-subscription-guidelines |
| Spotify chapters | Timestamp and title per line; formats `(MM:SS)`, `MM:SS`, `(HH:MM:SS)`, `HH:MM:SS`; first at 00:00; at least 3; titles under 40 chars, no emoji, no numbering; merge chapters under 30 s | [O] https://support.spotify.com/us/creators/article/creating-and-managing-chapters/ |
| Spotify formatting | Rich text and HTML toggle; `https` links only for RSS | [O-snippet] https://support.spotify.com/us/creators/article/formatting-your-show-notes/ |
| Apple chapters | Auto chapters when none exist, iOS 26.2, not under 10 minutes; read from episode notes | [S] https://podnews.net/article/apple-chapters-faq (updated 2026-04-21) |
| Line breaks | Spotify honors plain breaks, Apple did not | [S] https://podnews.net/article/html-episode-notes-in-podcast-rss (2021; old) |

### 10.3 Brief numbers
| Brief claim | Verdict |
|---|---|
| First 2-3 lines shown | UNVERIFIABLE |
| **"simple Markdown render everywhere"** | **STALE/wrong.** No source supports it; Apple lists HTML tags only |
| `title` 100 | INTERNAL. No published cap found |
| `body` 4000 | INTERNAL, consistent with the stricter Apple figure |
| `tags` | INTERNAL. No Apple target for them |

### 10.4 Strategy
- Lead the title with the guest or topic, not a number. [O]
- Work searchable words into the description. [O]
- Chapters help skimming and are read by Apple, Spotify and YouTube from the notes. [O/S]

### 10.5 Gap
- No chapters.
- Title guidance omits number/show-name/date bans.
- `##` headings do not render.
- The "one surprise" rule is fine.

### 10.6 Delivery: `chapters` is clearly useful here
```yaml
fields: [title, body, chapters, tags]
limits:
  title: { max_chars: 100 }
  body: { max_chars: 2400 }   # 2400 + 2 + 1500 = 3902, fits the stricter 4000
  tags: { max: 8, max_chars: 200 }
format: plain
```
- `format: plain` is needed: `format: markdown` renders `- 0:00 Intro`, and whether Spotify or Apple parse the `- ` prefix is unknown. Plain renders `0:00 Intro`.
- Body uses labels (`In this episode:`) and `-` bullets, not `##`.
- **Risk to test before shipping:** Spotify documents `MM:SS` and `00:00`. minuta writes `0:00`. If Spotify rejects it, `stamp` needs a format option (Rust).
- The `chapters` prompt says "for the YouTube description" and allows 10 s chapters, while Spotify advises 30 s. Rewording the built-in is a small change; the 30 s rule would need a limit setting.
- **Owner decides:** plain output, and whether to accept one more LLM call for chapters.

---

## 11. meeting-minutes

### 11.1 Complete deliverable
| Piece | Priority |
|---|---|
| Title, duration, attendees | Must |
| Date, time, place | Must, if stated; minuta has no date variable |
| Summary | Must |
| Decisions, with who approved and any vote | Must |
| Action items: owner, task, due | Must |
| Open questions | Must |
| Deferred items (parking lot) | Nice |
| Next meeting, if stated | Nice |
| Absentees | Only if the transcript says so |
| Draft marker for human review | Must for AI-made minutes [P] |

### 11.2 Sources
- "Minutes are a record of what was done at a meeting, not a record of what was said." Summarizing debate is improper for formal bodies. [O for RONR] https://www.robertsrules.com/frequently-asked-questions/ (undated)
- Date/time/place, attendees and absentees, decisions, owners with deadlines, next meeting. [P] https://www.wildapricot.com/articles/how-to-write-meeting-minutes, https://slite.com/en/learn/meeting-minutes
- Verify AI-drafted wording, owner and due date before sharing. [P] seen in search results for action-item tracking guides (for example https://www.read.ai/it/articles/action-item-tracking-best-practices-tools, https://www.remio.ai/post/action-items-tracker-template); the exact page for this line was not confirmed

### 11.3 Brief numbers
None are platform limits. "One page for one hour" is INTERNAL.

### 11.4 Strategy
Decisions and actions lead. Discussion is short, and only what is needed to understand them. Proposals stay distinct from agreements (already in the template).

### 11.5 Gap
- No date line, no next meeting, no deferred items, no review note.
- Discussion has equal billing, though RONR treats debate summaries as improper in formal minutes.
- The template lacks a timestamp in its action format although the brief's Good example has one.

### 11.6 Delivery
Stay Markdown. Add `Date:` only when the transcript states it. Move `## Decisions` and `## Action items` above `## Discussion`. **Owner decides:** business style with short discussion (recommended) or formal action-only. `chapters`: not applicable to Markdown output and not needed.

---

## 12. action-items

### 12.1 Complete deliverable
| Piece | Priority |
|---|---|
| Verb-led task, specific deliverable | Must |
| One owner | Must |
| Due date as spoken | Must when stated |
| Source timestamp | Must |
| Blocker or dependency | Nice, only when stated |
| Priority | Nice, only when stated; never inferred |
| Status | Skip; everything is new |
| Unclear list | Must |

### 12.2 Sources
Verb-led description, one owner, deadline, priority, status, context. [P, reputable] https://asana.com/templates/action-items.md. Shared ownership is the most common failure; record the source. [P] same tracking guides as in 11.2, exact page not confirmed

### 12.3 Brief numbers
None are platform limits.

### 12.4 Strategy
One verb, one owner, one result. Keep what was said; never upgrade a wish.

### 12.5 Gap
- Blockers absent.
- "Count the items, then choose the layout" needs a reliable count from a small model. A flat list sorted by owner is simpler; grouping then becomes unnecessary.
- `Always put the due date in parentheses` conflicts with `omit the due-date parentheses` when none is stated, though context resolves it.

### 12.6 Delivery
Stay Markdown; this is the strongest of the work templates. Add one optional clause: `(blocked by: ...)` only when stated. **Owner decides:** drop the grouping threshold. `chapters`: no.

---

## 13. New templates

| Candidate | Verdict | Reason |
|---|---|---|
| `youtube-shorts` | **Add** | A recording's best clip needs title and hashtags but must avoid `chapters`, which fails under 30 s and is meaningless in a short. [O] Shorts up to 3 minutes, title 100: https://support.google.com/youtube/answer/10059070 |
| `instagram-carousel` | **Add** | [S] carousels lead on saves and engagement while Reels lead on reach; 20 items [O]. Works from text alone, with no video edit: https://metricool.com/instagram-research-study/ |
| `threads` | **Add later** | Cheap variation: 500 chars, one topic tag, 5 links, emoji counted by UTF-8 bytes [O]: https://developers.facebook.com/documentation/threads/posts.md. Long version via 10,000-char text attachment [O]: https://developers.facebook.com/documentation/threads/create-posts/text-attachments.md |
| Facebook | Skip | No verified current limits or hashtag guidance found; caption reuse from Instagram is adequate [P] |
| Reddit | Skip | Community rules override site norms; 9:1 is a guideline [S] https://redship.io/blog/reddit-self-promotion-rules; official pages could not be read; a generic generated post risks removal |

---

## 14. Cross-cutting rules for all templates

1. **Grounding stays.** Keep the existing "Use only facts..." blocks. Richer outputs add surfaces for invention, not exceptions.
2. **Sort each piece into model-derivable or human-supplied.** Hooks, hashtags, tags, cover text, alt titles, meta description: derivable. Links, dates, bylines, sponsors, guest bios, location, sound: human. Never ask the model for the second group.
3. **No declared field without guaranteed content** (non-empty rule). Conditional content belongs in `body` sections that may be omitted, or in `notes`.
4. **Keywords come from the input.** The model has no search data. "Words a viewer would type" means plain wording of the topic, not claims of popularity or volume. No trending tags.
5. **Limits belong in `limits:`, not in counting instructions.** Local models cannot count. Where a limit is not enforceable, target well below it. Remove "every N words" rules.
6. **Limits are ceilings, stated as such.** Filling to the limit is a logged failure mode; so is cutting. Prefer a generous ceiling and a low target.
7. **Fixed names survive.** Keep the bans on computed numbers, invented frameworks ("3-step system"), unattributed remarks and mid-sentence endings; these are the recorded failures in `docs/conventions.md`.
8. **Short-form needs a clip.** A long recording is not a Reel. The strategist's first output is which moment to cut. Only the transcript has reliable times, so it needs `input: transcript`, or accept that a summary may not have them. Never estimate a time. ADR-0009 mentions a future edit plan; this could feed it.
9. **Strategy as placement rules,** not adjectives: "main phrase in the first sentence", not "be engaging".
10. **State reasons as guidance, not as platform claims,** and avoid reach claims in prompts, because they go stale (the X sentence shows why).
11. **Brief hygiene.** Replace the blanket "as the authors knew them" sentence in the README with a source and date per number. Fix README line 20. Keep brief and template in sync (the x-thread post count differs).
12. **Prompts are already long.** Adding pieces should replace text, not append. Each new required piece raises repair-call risk on small models.
13. **Process.** Any curated change needs the evidence run in `docs/conventions.md` line 33 and a new sha256 in `templates/index.toml`.

---

## 15. Decisions for the owner

1. **Extras mechanism:** new `notes` field (recommended), chained `-extras` templates, or drop those pieces.
2. **Preview:** separate `preview` field, or part of `notes`.
3. **Labels in `<name>.md`:** add them for non-body fields?
4. **`posts` list field** with per-item limit for `x-thread`, Threads and carousels, or stay Markdown with a low target.
5. **podcast-notes:** switch to `format: plain` and add `chapters`, accepting one more call; test `0:00` on Spotify first.
6. **LinkedIn hashtags:** keep at 1-3 (recommended) or remove.
7. **TikTok:** `hashtags max 5` and `body 500`.
8. **Clip suggestion for short-form:** move input to `transcript`, or accept suggestions only when the summary has stamps.
9. **Minutes style:** short-discussion business minutes (recommended) or formal action-only.
10. **action-items:** drop the 5-item grouping rule.
11. **New templates:** `youtube-shorts` and `instagram-carousel` now, `threads` later, skip Facebook and Reddit.
12. **Human-supplied items:** may `notes` list "add before publishing" reminders (recommended), given the template rule against placeholders?
13. **AI disclosure line** in blog posts: Google suggests it where readers would ask.
14. **YouTube channel boilerplate:** leave to YouTube's default description (recommended) or add a config value (Rust).
