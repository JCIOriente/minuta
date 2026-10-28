//! Prompt assembly, chunking and consolidation.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;

use crate::error::{Error, Result};
use crate::transcript::Transcript;

/// Version of the prompt wrappers below. It is part of every template step's fingerprint.
pub const PROMPT_VERSION: u32 = 1;

// Prompt wrapper wording (spec: "Prompt assembly, chunking and consolidation").
// Changing any of these requires bumping PROMPT_VERSION.
const LANGUAGE_PREFIX: &str = "Write the entire response in ";
const LANGUAGE_SUFFIX: &str = ", including headings and any placeholder text, regardless of \
                               the language of the input or of these instructions.";
const MAP_INTRO: &str = "You are preparing notes for this task:";
const MAP_TASK: &str = "Extract everything the task needs from this part, keeping timestamps. \
                        Do not write the final document.";
const CONSOLIDATION: &str =
    "The input is notes extracted from consecutive parts of a longer input.";

/// Minimum span of one transcript block, in seconds.
const BLOCK_SECONDS: f64 = 30.0;

const NOT_SHRINKING: &str = "notes do not shrink below the context budget";

/// The transcript as LLM input: segments merged into blocks spanning at least 30 s (the last
/// may be shorter), one `[hh:mm:ss] text` line per block, stamped with the block start.
pub fn transcript_text(t: &Transcript) -> String {
    let mut lines = Vec::new();
    let mut block: Option<(f64, String)> = None;
    for s in &t.segments {
        let (start, text) = block.get_or_insert_with(|| (s.start, String::new()));
        let piece = s.text.trim();
        if !piece.is_empty() {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(piece);
        }
        if s.end - *start >= BLOCK_SECONDS {
            lines.extend(block.take().and_then(block_line));
        }
    }
    lines.extend(block.and_then(block_line));
    lines.join("\n")
}

/// `[hh:mm:ss] text`, or None for a block without text.
fn block_line((start, text): (f64, String)) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    // Truncation to whole seconds is intended; negative starts clamp to zero.
    let secs = start.max(0.0) as u64;
    Some(format!(
        "[{:02}:{:02}:{:02}] {text}",
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    ))
}

/// Characters / 3, rounded up, over Unicode scalar values.
pub fn estimate_tokens(s: &str) -> usize {
    s.chars().count().div_ceil(3)
}

/// The sentence every system prompt ends with. `output_lang` is a language name ("Spanish").
pub fn language_instruction(output_lang: &str) -> String {
    format!("{LANGUAGE_PREFIX}{output_lang}{LANGUAGE_SUFFIX}")
}

pub struct Request<'a> {
    /// The rendered template body.
    pub body: &'a str,
    /// Output language name, as it appears in the language instruction.
    pub output_lang: &'a str,
    pub input: &'a str,
    pub context_tokens: usize,
    /// Maximum concurrent calls; 0 is treated as 1.
    pub concurrency: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Chunk {
        done: usize,
        total: usize,
    },
    Consolidating,
    /// `done` of the `total` groups of merge round `round` (1-based) have finished; reported
    /// with `done: 0` when the round starts.
    Merge {
        round: usize,
        done: usize,
        total: usize,
    },
    Repair {
        attempt: usize,
        total: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generated {
    pub text: String,
    pub calls: usize,
}

type Complete<'c> = &'c (dyn Fn(&str, &str) -> Result<String> + Sync);
/// Makes the final call: `finish(system, user, on_event)`.
type Finish<'f> = &'f mut dyn FnMut(&str, &str, &mut dyn FnMut(Event)) -> Result<String>;
/// The system prompt for part i of n.
type SystemFor<'f> = &'f (dyn Fn(usize, usize) -> String + Sync);

/// Turn `req.input` into one document with `complete(system, user)`: one call when it fits the
/// context budget, otherwise concurrent map calls over chunks and a final consolidation call.
/// `on_event` is only called on the calling thread. Template errors leave `name` empty for the
/// caller to fill in.
pub fn generate(
    req: &Request,
    complete: Complete,
    on_event: &mut dyn FnMut(Event),
) -> Result<Generated> {
    generate_with(
        req,
        None,
        complete,
        &mut |system: &str, user: &str, _: &mut dyn FnMut(Event)| complete(system, user),
        on_event,
    )
}

/// Like `generate`, but `final_block` (if any) goes before the language instruction in the
/// final system prompt (single call or consolidation), and the final call is made by
/// `finish(system, user, on_event)` instead of `complete`. Map calls are unchanged. Events
/// `finish` emits reach `on_event`, after the last `Consolidating` when chunked.
pub fn generate_with(
    req: &Request,
    final_block: Option<&str>,
    complete: Complete,
    finish: Finish,
    on_event: &mut dyn FnMut(Event),
) -> Result<Generated> {
    let context = req.context_tokens;
    let instruction = language_instruction(req.output_lang);
    let block = final_block.map(|b| format!("{b}\n\n")).unwrap_or_default();
    let system = format!("{}\n\n{block}{instruction}", req.body);
    if fits(&system, req.input, context) {
        let text = finish(&system, req.input, on_event)?;
        return Ok(Generated { text, calls: 1 });
    }

    let reduce_system = format!("{}\n\n{CONSOLIDATION}\n\n{block}{instruction}", req.body);
    // Fail before any map call when the consolidation prompt alone cannot fit. Without a
    // block it is shorter than every map prompt, so this adds no new failure.
    input_budget(&reduce_system, context)?;
    let map_system = |i: usize, n: usize| {
        format!(
            "{MAP_INTRO}\n\n{}\n\nThis is part {i} of {n}. {MAP_TASK}\n\n{instruction}",
            req.body
        )
    };
    let lines: Vec<&str> = req.input.lines().collect();
    let chunks = split_inputs(&lines, "\n", context, &map_system)?;
    let total = chunks.len();
    let mut calls = total;
    let mut notes = run_all(
        &chunks,
        &map_system,
        req.concurrency,
        complete,
        &mut |done| on_event(Event::Chunk { done, total }),
    )?;

    on_event(Event::Consolidating);
    let mut joined = join_notes(&notes);
    let mut round = 0;
    while !fits(&reduce_system, &joined, context) {
        round += 1;
        let units = part_units(&notes);
        let units: Vec<&str> = units.iter().map(String::as_str).collect();
        let groups = split_inputs(&units, "\n\n", context, &map_system)?;
        let total = groups.len();
        calls += total;
        on_event(Event::Merge {
            round,
            done: 0,
            total,
        });
        notes = run_all(
            &groups,
            &map_system,
            req.concurrency,
            complete,
            &mut |done| on_event(Event::Merge { round, done, total }),
        )?;
        let merged = join_notes(&notes);
        if estimate_tokens(&merged) >= estimate_tokens(&joined) {
            return Err(template_error(NOT_SHRINKING.to_string()));
        }
        joined = merged;
    }
    if round > 0 {
        // Back from the last round's count to the final consolidation call.
        on_event(Event::Consolidating);
    }
    let text = finish(&reduce_system, &joined, on_event)?;
    Ok(Generated {
        text,
        calls: calls + 1,
    })
}

fn template_error(message: String) -> Error {
    Error::Template {
        name: String::new(),
        message,
    }
}

/// True when a call fits: estimate(system) + estimate(user) + context/4 <= context.
fn fits(system: &str, user: &str, context: usize) -> bool {
    estimate_tokens(system)
        .saturating_add(estimate_tokens(user))
        .saturating_add(context / 4)
        <= context
}

/// The most characters a user message may have next to `system`.
fn input_budget(system: &str, context: usize) -> Result<usize> {
    let tokens = (context - context / 4).saturating_sub(estimate_tokens(system));
    if tokens == 0 {
        return Err(template_error(format!(
            "the prompt alone does not fit the context budget of {context} tokens"
        )));
    }
    Ok(tokens.saturating_mul(3))
}

/// Pack `units` into inputs that fit next to `system_for(i, n)` for every part i of n.
/// The budget is taken from the longest prompt, "part n of n"; n is raised until the
/// resulting count has no more digits than the n assumed.
fn split_inputs(
    units: &[&str],
    sep: &str,
    context: usize,
    system_for: SystemFor,
) -> Result<Vec<String>> {
    let digits = |n: usize| n.to_string().len();
    let mut n = 1;
    loop {
        let max_chars = input_budget(&system_for(n, n), context)?;
        let inputs = pack(units.iter().copied(), sep, max_chars);
        if digits(inputs.len()) <= digits(n) {
            return Ok(inputs);
        }
        n = inputs.len();
    }
}

fn part_units(notes: &[String]) -> Vec<String> {
    notes
        .iter()
        .enumerate()
        .map(|(i, note)| format!("## Part {}\n\n{}", i + 1, note.trim()))
        .collect()
}

fn join_notes(notes: &[String]) -> String {
    part_units(notes).join("\n\n")
}

/// Greedily join `units` with `sep` into pieces of at most `max_chars` characters, keeping
/// order. A unit over the limit is split at its lines, or else at whitespace, or else (one
/// overlong word) at the limit.
fn pack<'a>(units: impl IntoIterator<Item = &'a str>, sep: &str, max_chars: usize) -> Vec<String> {
    let mut packer = Packer {
        sep,
        sep_len: sep.chars().count(),
        max_chars,
        out: Vec::new(),
        current: None,
    };
    for unit in units {
        let len = unit.chars().count();
        if len <= max_chars {
            packer.push(unit, len);
            continue;
        }
        let pieces = if unit.contains('\n') {
            pack(unit.lines(), "\n", max_chars)
        } else {
            split_at_whitespace(unit, max_chars)
        };
        for piece in &pieces {
            packer.push(piece, piece.chars().count());
        }
    }
    packer.finish()
}

struct Packer<'s> {
    sep: &'s str,
    sep_len: usize,
    max_chars: usize,
    out: Vec<String>,
    /// The piece being filled and its length in characters.
    current: Option<(String, usize)>,
}

impl Packer<'_> {
    fn push(&mut self, unit: &str, len: usize) {
        let (sep_len, max_chars) = (self.sep_len, self.max_chars);
        match &mut self.current {
            Some((text, n)) if *n + sep_len + len <= max_chars => {
                text.push_str(self.sep);
                text.push_str(unit);
                *n += sep_len + len;
            }
            _ => {
                self.out.extend(self.current.take().map(|(text, _)| text));
                self.current = Some((unit.to_string(), len));
            }
        }
    }

    fn finish(mut self) -> Vec<String> {
        self.out.extend(self.current.take().map(|(text, _)| text));
        self.out
    }
}

/// Split one line into pieces of at most `max_chars` (> 0) characters at whitespace, which is
/// dropped at the cut. A word longer than the limit is cut at the limit. One pass, O(len).
fn split_at_whitespace(line: &str, max_chars: usize) -> Vec<String> {
    let s = line.trim();
    let mut out = Vec::new();
    // (byte, char position) where the piece being filled starts; None while skipping the
    // whitespace after a cut.
    let mut start: Option<(usize, usize)> = None;
    // Byte where the current whitespace run began.
    let mut ws_run: Option<usize> = None;
    // The last whitespace run inside the piece: (its start byte, then byte and char position
    // of the next word).
    let mut cut: Option<(usize, usize, usize)> = None;
    for (pos, (i, c)) in s.char_indices().enumerate() {
        let Some((start_byte, start_pos)) = start else {
            if !c.is_whitespace() {
                start = Some((i, pos));
            }
            continue;
        };
        let full = pos - start_pos == max_chars;
        if c.is_whitespace() {
            if full {
                out.push(s[start_byte..i].trim_end().to_string());
                (start, ws_run, cut) = (None, None, None);
            } else {
                ws_run.get_or_insert(i);
            }
            continue;
        }
        if let Some(run) = ws_run.take() {
            cut = Some((run, i, pos));
        }
        if full {
            let (end, resume) = match cut.take() {
                Some((run, byte, p)) => (run, (byte, p)),
                None => (i, (i, pos)),
            };
            out.push(s[start_byte..end].to_string());
            start = Some(resume);
        }
    }
    if let Some((byte, _)) = start {
        out.push(s[byte..].to_string());
    }
    out
}

/// Run a call for every input, part i of n with system prompt `system_for(i, n)` (built by
/// the worker just before its call), at most `concurrency` at a time, and return the outputs
/// in order. `on_done(count)` runs on the calling thread after each successful call. The first
/// error stops new calls from starting and is returned once the in-flight calls end.
fn run_all(
    inputs: &[String],
    system_for: SystemFor,
    concurrency: usize,
    complete: Complete,
    on_done: &mut dyn FnMut(usize),
) -> Result<Vec<String>> {
    let total = inputs.len();
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let mut outputs: Vec<Option<String>> = vec![None; total];
    let mut first_error = None;
    thread::scope(|scope| {
        let (tx, rx) = mpsc::channel();
        for _ in 0..concurrency.clamp(1, total.max(1)) {
            let (tx, next, stop) = (tx.clone(), &next, &stop);
            scope.spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(user) = inputs.get(i) else {
                        break;
                    };
                    let result = complete(&system_for(i + 1, total), user);
                    if result.is_err() {
                        stop.store(true, Ordering::SeqCst);
                    }
                    if tx.send((i, result)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);
        let mut done = 0;
        for (i, result) in rx {
            match result {
                Ok(text) => {
                    outputs[i] = Some(text);
                    done += 1;
                    on_done(done);
                }
                Err(e) => {
                    first_error.get_or_insert(e);
                }
            }
        }
    });
    if let Some(e) = first_error {
        return Err(e);
    }
    Ok(outputs
        .into_iter()
        .map(|o| o.expect("every call reported a result when none failed"))
        .collect())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Condvar, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::error::Error;
    use crate::transcript::Segment;

    const BODY: &str = "Write meeting minutes.";
    const LANG: &str = "Spanish";

    fn seg(start: f64, end: f64, text: &str) -> Segment {
        Segment {
            start,
            end,
            text: text.into(),
        }
    }

    fn transcript(segments: Vec<Segment>) -> Transcript {
        Transcript {
            language: "en".into(),
            segments,
        }
    }

    fn numbered_lines(n: usize) -> String {
        (0..n)
            .map(|i| format!("line {i:03}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    const DEADLINE: Duration = Duration::from_secs(10);

    /// A counter threads can raise and wait on, failing instead of hanging after DEADLINE.
    struct Gate {
        count: Mutex<usize>,
        changed: Condvar,
    }

    impl Gate {
        fn new() -> Self {
            Gate {
                count: Mutex::new(0),
                changed: Condvar::new(),
            }
        }
        fn raise(&self) {
            *self.count.lock().unwrap() += 1;
            self.changed.notify_all();
        }
        /// Block until the count reaches `n`; panic with `failure` after DEADLINE.
        fn wait_for(&self, n: usize, failure: &str) {
            let guard = self.count.lock().unwrap();
            let (_count, timeout) = self
                .changed
                .wait_timeout_while(guard, DEADLINE, |count| *count < n)
                .unwrap();
            assert!(!timeout.timed_out(), "{failure}");
        }
        /// Block until the count exceeds `n` or `timeout` passes, whichever comes first.
        fn wait_above(&self, n: usize, timeout: Duration) {
            let guard = self.count.lock().unwrap();
            let (_count, _timeout) = self
                .changed
                .wait_timeout_while(guard, timeout, |count| *count <= n)
                .unwrap();
        }
    }

    fn request<'a>(input: &'a str, context_tokens: usize, concurrency: usize) -> Request<'a> {
        Request {
            body: BODY,
            output_lang: LANG,
            input,
            context_tokens,
            concurrency,
        }
    }

    fn fits(system: &str, user: &str, context: usize) -> bool {
        estimate_tokens(system) + estimate_tokens(user) + context / 4 <= context
    }

    /// The `i` in "This is part i of n." of a map system prompt.
    fn part_index(system: &str) -> usize {
        let rest = system.split("This is part ").nth(1).expect("map prompt");
        rest.split(' ').next().unwrap().parse().unwrap()
    }

    fn is_final(system: &str) -> bool {
        system.contains(CONSOLIDATION)
    }

    /// Records every (system, user) pair and answers with `reply`.
    struct Recorder {
        calls: Mutex<Vec<(String, String)>>,
    }

    impl Recorder {
        fn new() -> Self {
            Recorder {
                calls: Mutex::new(Vec::new()),
            }
        }
        fn record(&self, system: &str, user: &str) {
            self.calls
                .lock()
                .unwrap()
                .push((system.to_string(), user.to_string()));
        }
        fn calls(&self) -> Vec<(String, String)> {
            self.calls.lock().unwrap().clone()
        }
    }

    /// The map calls (the first `total` calls), sorted by part index.
    fn map_calls(calls: &[(String, String)], total: usize) -> Vec<(String, String)> {
        let mut maps = calls[..total].to_vec();
        maps.sort_by_key(|(s, _)| part_index(s));
        maps
    }

    fn chunk_total(events: &[Event]) -> usize {
        events
            .iter()
            .filter_map(|e| match e {
                Event::Chunk { total, .. } => Some(*total),
                Event::Consolidating | Event::Merge { .. } | Event::Repair { .. } => None,
            })
            .next()
            .unwrap_or(0)
    }

    #[test]
    fn blocks_span_30_seconds() {
        let t = transcript(vec![
            seg(0.0, 10.0, " a "),
            seg(10.0, 20.0, "b"),
            seg(20.0, 31.0, "c"),
            seg(31.0, 40.0, "d"),
            seg(40.0, 61.0, "e"),
            seg(61.0, 65.0, "f"),
        ]);
        assert_eq!(
            transcript_text(&t),
            "[00:00:00] a b c\n[00:00:31] d e\n[00:01:01] f"
        );
    }

    #[test]
    fn block_timestamp_format_over_one_hour() {
        let t = transcript(vec![seg(3725.7, 3760.0, "x")]);
        assert_eq!(transcript_text(&t), "[01:02:05] x");
    }

    #[test]
    fn empty_transcript_empty_text() {
        assert_eq!(transcript_text(&transcript(vec![])), "");
    }

    #[test]
    fn estimate_rounds_up() {
        let cases = [
            ("", 0),
            ("a", 1),
            ("abc", 1),
            ("abcd", 2),
            ("abcdef", 2),
            ("ñññ", 1),
            ("ñññn", 2),
        ];
        for (s, want) in cases {
            assert_eq!(estimate_tokens(s), want, "{s:?}");
        }
    }

    #[test]
    fn language_instruction_last() {
        let instruction = language_instruction(LANG);
        assert_eq!(
            instruction,
            "Write the entire response in Spanish, including headings and any placeholder \
             text, regardless of the language of the input or of these instructions."
        );
        let rec = Recorder::new();
        let complete = |s: &str, u: &str| {
            rec.record(s, u);
            Ok("out".to_string())
        };
        generate(&request("hello", 1000, 1), &complete, &mut |_| {}).unwrap();
        assert_eq!(rec.calls()[0].0, format!("{BODY}\n\n{instruction}"));
    }

    #[test]
    fn fits_makes_one_call() {
        let input = "[00:00:00] hello there";
        let rec = Recorder::new();
        let complete = |s: &str, u: &str| {
            rec.record(s, u);
            Ok("the minutes".to_string())
        };
        let mut events = Vec::new();
        let out = generate(&request(input, 1000, 4), &complete, &mut |e| events.push(e)).unwrap();
        assert_eq!(
            out,
            Generated {
                text: "the minutes".into(),
                calls: 1
            }
        );
        let calls = rec.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].0,
            format!("{BODY}\n\n{}", language_instruction(LANG))
        );
        assert_eq!(calls[0].1, input);
        assert!(events.is_empty(), "{events:?}");
    }

    #[test]
    fn system_alone_too_large_fails() {
        let body = "word ".repeat(400);
        let req = Request {
            body: &body,
            output_lang: LANG,
            input: "hello",
            context_tokens: 200,
            concurrency: 1,
        };
        let err = generate(&req, &|_: &str, _: &str| Ok("x".into()), &mut |_| {}).unwrap_err();
        assert!(
            matches!(err, Error::Template { ref name, .. } if name.is_empty()),
            "{err}"
        );
    }

    /// xorshift64*: deterministic, good enough to vary test inputs.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
        }
        fn range(&mut self, lo: usize, hi: usize) -> usize {
            lo + (self.next() % (hi - lo) as u64) as usize
        }
        fn words(&mut self, n: usize, max_len: usize) -> Vec<String> {
            (0..n)
                .map(|_| {
                    let len = self.range(1, max_len + 1);
                    (0..len)
                        .map(|_| {
                            if self.range(0, 10) == 0 {
                                'é'
                            } else {
                                (b'a' + self.range(0, 26) as u8) as char
                            }
                        })
                        .collect()
                })
                .collect()
        }
    }

    #[test]
    fn chunks_never_exceed_budget() {
        let mut ok = 0;
        for case in 0..300u64 {
            let mut rng = Rng(case.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
            let context = rng.range(60, 3000);
            let body_words = rng.range(0, 60);
            let body = rng.words(body_words, 12).join(" ");
            let line_count = rng.range(1, 80);
            let lines: Vec<String> = (0..line_count)
                .map(|_| {
                    let n = rng.range(0, 30);
                    let max_len = if rng.range(0, 8) == 0 { 400 } else { 15 };
                    rng.words(n, max_len).join(" ")
                })
                .collect();
            let input = lines.join("\n");
            let concurrency = rng.range(1, 5);
            let req = Request {
                body: &body,
                output_lang: LANG,
                input: &input,
                context_tokens: context,
                concurrency,
            };
            let rec = Recorder::new();
            let complete = |s: &str, u: &str| {
                rec.record(s, u);
                Ok("note".to_string())
            };
            let mut events = Vec::new();
            let result = generate(&req, &complete, &mut |e| events.push(e));
            let calls = rec.calls();
            for (s, u) in &calls {
                assert!(fits(s, u, context), "case {case}: call over budget");
            }
            match result {
                Ok(out) => {
                    ok += 1;
                    assert_eq!(out.calls, calls.len(), "case {case}");
                    let total = chunk_total(&events);
                    if total > 0 {
                        // Overlong words are cut, so compare text without whitespace.
                        let got: String = map_calls(&calls, total)
                            .iter()
                            .flat_map(|(_, u)| u.chars().filter(|c| !c.is_whitespace()))
                            .collect();
                        let want: String = input.chars().filter(|c| !c.is_whitespace()).collect();
                        assert_eq!(got, want, "case {case}: text lost or reordered");
                    }
                }
                Err(Error::Template { name, .. }) => assert!(name.is_empty(), "case {case}"),
                Err(e) => panic!("case {case}: unexpected error {e}"),
            }
        }
        assert!(ok > 150, "only {ok} cases succeeded");
    }

    #[test]
    fn overlong_line_split_at_whitespace() {
        let cases: [(&str, usize, Vec<&str>); 5] = [
            ("aaaa bbbb cccc", 9, vec!["aaaa bbbb", "cccc"]),
            ("aaaa  bbbb", 4, vec!["aaaa", "bbbb"]),
            ("abcdefghij", 4, vec!["abcd", "efgh", "ij"]),
            ("a bbbbbb", 4, vec!["a", "bbbb", "bb"]),
            ("ééé ñññ", 3, vec!["ééé", "ñññ"]),
        ];
        for (line, max, want) in cases {
            assert_eq!(pack([line], "\n", max), want, "{line:?} max {max}");
        }
    }

    #[test]
    fn huge_single_line_splits_quickly() {
        const WORDS: &str = "abcdefg hij klmnopqrs tu "; // 25 bytes
        let small = WORDS.repeat(5_000); // 125 KB, one line
        let large = WORDS.repeat(320_000); // 8 MB, one line, 64x the small one
        const LIMIT: Duration = Duration::from_secs(30);
        let started = Instant::now();
        // One deadline for the whole measurement: a quadratic split fails instead of hanging.
        // The worker is left running on timeout; the process ends when the test binary does.
        let run = |line: &str| {
            let (tx, rx) = mpsc::channel();
            let owned = line.to_string();
            thread::spawn(move || {
                let began = Instant::now();
                let pieces = pack([owned.as_str()], "\n", 30);
                let _ = tx.send((began.elapsed(), pieces));
            });
            let left = LIMIT.saturating_sub(started.elapsed());
            rx.recv_timeout(left).unwrap_or_else(|e| match e {
                mpsc::RecvTimeoutError::Timeout => panic!(
                    "pack did not finish within {LIMIT:?} on a {}-byte line; \
                     split_at_whitespace is likely not linear",
                    line.len()
                ),
                mpsc::RecvTimeoutError::Disconnected => panic!(
                    "pack panicked on a {}-byte line; see the worker's panic above",
                    line.len()
                ),
            })
        };
        // Best of three runs for the small line, floored at 1 ms so a tiny timing does not make
        // a noisy ratio. The large line runs once.
        let t_small = (0..3)
            .map(|_| run(&small).0)
            .min()
            .unwrap()
            .max(Duration::from_millis(1));
        let (t_large, pieces) = run(&large);
        // 64x the input: linear growth gives a ratio near 64, quadratic near 4096. The limit of
        // 1024 leaves over 10x headroom for a loaded machine and stays well below quadratic.
        let ratio = t_large.as_secs_f64() / t_small.as_secs_f64();
        assert!(
            ratio < 1024.0,
            "64x input took {ratio:.1}x as long ({t_small:?} -> {t_large:?})"
        );
        assert!(pieces.iter().all(|p| p.chars().count() <= 30));
        let got: String = pieces.concat().split_whitespace().collect();
        let want: String = large.split_whitespace().collect();
        assert_eq!(got, want);
    }

    #[test]
    fn chunk_order_kept() {
        let input = numbered_lines(200);
        let rec = Recorder::new();
        let complete = |s: &str, u: &str| {
            rec.record(s, u);
            Ok("n".to_string())
        };
        let mut events = Vec::new();
        generate(&request(&input, 300, 3), &complete, &mut |e| events.push(e)).unwrap();
        let total = chunk_total(&events);
        assert!(total > 2, "expected several chunks, got {total}");
        let maps = map_calls(&rec.calls(), total);
        for (i, (s, _)) in maps.iter().enumerate() {
            assert!(
                s.contains(&format!("This is part {} of {total}.", i + 1)),
                "{s}"
            );
        }
        let joined: Vec<&str> = maps.iter().map(|(_, u)| u.as_str()).collect();
        assert_eq!(joined.join("\n"), input);
    }

    #[test]
    fn concurrency_limit_respected() {
        const CONCURRENCY: usize = 3;
        let input = numbered_lines(200);
        let started = AtomicUsize::new(0);
        let current = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        // The first CONCURRENCY calls wait for each other, so they overlap for certain. They
        // then hold on for up to 100 ms more, giving a worker beyond the limit time to start
        // a call and raise the peak; for correct code that wait simply times out.
        let arrived = Gate::new();
        let complete = |s: &str, _: &str| {
            if !is_final(s) {
                let now = current.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                arrived.raise();
                if started.fetch_add(1, Ordering::SeqCst) < CONCURRENCY {
                    arrived.wait_for(CONCURRENCY, "fewer than 3 calls overlapped");
                    arrived.wait_above(CONCURRENCY, Duration::from_millis(100));
                }
                current.fetch_sub(1, Ordering::SeqCst);
            }
            Ok("n".to_string())
        };
        let mut events = Vec::new();
        generate(&request(&input, 300, CONCURRENCY), &complete, &mut |e| {
            events.push(e)
        })
        .unwrap();
        assert!(chunk_total(&events) > CONCURRENCY);
        assert_eq!(peak.load(Ordering::SeqCst), CONCURRENCY);
    }

    #[test]
    fn fit_boundary_is_inclusive() {
        let context = 1000;
        let system = format!("{BODY}\n\n{}", language_instruction(LANG));
        let room = context - context / 4 - estimate_tokens(&system);
        let exact = "x".repeat(room * 3);
        assert!(fits(&system, &exact, context));
        let cases = [(exact.clone(), true), (format!("{exact}x"), false)];
        for (input, one_call) in cases {
            let count = AtomicUsize::new(0);
            let complete = |_: &str, _: &str| {
                count.fetch_add(1, Ordering::SeqCst);
                Ok("n".to_string())
            };
            let out = generate(&request(&input, context, 1), &complete, &mut |_| {}).unwrap();
            assert_eq!(out.calls == 1, one_call, "input of {} chars", input.len());
        }
    }

    #[test]
    fn reduce_joins_parts_in_order() {
        let input = numbered_lines(200);
        let rec = Recorder::new();
        let complete = |s: &str, u: &str| {
            rec.record(s, u);
            if is_final(s) {
                Ok("final".to_string())
            } else {
                Ok(format!("notes on {}", u.lines().next().unwrap()))
            }
        };
        let mut events = Vec::new();
        let out = generate(&request(&input, 300, 3), &complete, &mut |e| events.push(e)).unwrap();
        let total = chunk_total(&events);
        let calls = rec.calls();
        assert_eq!(
            out,
            Generated {
                text: "final".into(),
                calls: total + 1
            }
        );
        let instruction = language_instruction(LANG);
        let maps = map_calls(&calls, total);
        for (s, _) in &maps {
            assert!(s.ends_with(&format!("\n\n{instruction}")), "{s}");
        }
        let want_user = maps
            .iter()
            .enumerate()
            .map(|(i, (_, u))| {
                format!(
                    "## Part {}\n\nnotes on {}",
                    i + 1,
                    u.lines().next().unwrap()
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let (final_system, final_user) = calls.last().unwrap();
        assert_eq!(final_user, &want_user);
        assert_eq!(
            final_system,
            &format!("{BODY}\n\n{CONSOLIDATION}\n\n{instruction}")
        );
    }

    #[test]
    fn multi_round_reduce() {
        let input = numbered_lines(400);
        let rec = Recorder::new();
        let complete = |s: &str, u: &str| {
            rec.record(s, u);
            if is_final(s) {
                Ok("final".to_string())
            } else if u.starts_with("## Part") {
                // Shrinks, but not enough to fit after one round.
                Ok("y".repeat(u.chars().count() * 3 / 4))
            } else {
                Ok("x".repeat(200))
            }
        };
        // Concurrency 1 keeps the recorded calls in order, so each round starts with part 1.
        let out = generate(&request(&input, 400, 1), &complete, &mut |_| {}).unwrap();
        let calls = rec.calls();
        let rounds = calls
            .iter()
            .filter(|(s, u)| u.starts_with("## Part") && s.contains("This is part 1 of "))
            .count();
        assert!(
            rounds >= 2,
            "expected at least two merge rounds, got {rounds}"
        );
        assert_eq!(out.text, "final");
        assert_eq!(out.calls, calls.len());
        for (s, u) in &calls {
            assert!(fits(s, u, 400), "call over budget");
        }
    }

    #[test]
    fn merge_rounds_report_progress() {
        let input = numbered_lines(400);
        // 7 map notes need a first merge round of 4 groups, whose notes still do not fit, then
        // a second round of 2 groups, whose short notes fit the consolidation call. Each round
        // reports its start (0 done), then every finished group.
        let complete = |s: &str, u: &str| {
            if is_final(s) {
                Ok("final".to_string())
            } else if u.contains('y') {
                Ok("z".to_string())
            } else if u.starts_with("## Part") {
                Ok("y".repeat(200))
            } else {
                Ok("x".repeat(200))
            }
        };
        let mut events = Vec::new();
        let out = generate(&request(&input, 400, 3), &complete, &mut |e| events.push(e)).unwrap();
        let chunk = |done| Event::Chunk { done, total: 7 };
        let merge = |round, done, total| Event::Merge { round, done, total };
        let want = vec![
            chunk(1),
            chunk(2),
            chunk(3),
            chunk(4),
            chunk(5),
            chunk(6),
            chunk(7),
            Event::Consolidating,
            merge(1, 0, 4),
            merge(1, 1, 4),
            merge(1, 2, 4),
            merge(1, 3, 4),
            merge(1, 4, 4),
            merge(2, 0, 2),
            merge(2, 1, 2),
            merge(2, 2, 2),
            // The final consolidation call.
            Event::Consolidating,
        ];
        assert_eq!(events, want);
        assert_eq!(
            out,
            Generated {
                text: "final".into(),
                calls: 7 + 4 + 2 + 1
            }
        );
    }

    #[test]
    fn non_shrinking_notes_fail() {
        let input = numbered_lines(200);
        let complete = |_: &str, u: &str| {
            if u.starts_with("## Part") {
                Ok(u.to_string())
            } else {
                Ok("x".repeat(200))
            }
        };
        let err = generate(&request(&input, 400, 2), &complete, &mut |_| {}).unwrap_err();
        assert!(
            matches!(
                err,
                Error::Template { ref name, ref message }
                    if name.is_empty() && message == "notes do not shrink below the context budget"
            ),
            "{err}"
        );
    }

    fn boom() -> Error {
        Error::Llm {
            backend: "fake".into(),
            message: "boom".into(),
            transient: false,
        }
    }

    #[test]
    fn first_error_stops_scheduling() {
        let input = numbered_lines(200);
        let count = AtomicUsize::new(0);
        let complete = |_: &str, _: &str| {
            if count.fetch_add(1, Ordering::SeqCst) == 1 {
                Err(boom())
            } else {
                Ok("n".to_string())
            }
        };
        let err = generate(&request(&input, 300, 1), &complete, &mut |_| {}).unwrap_err();
        assert!(matches!(err, Error::Llm { ref message, .. } if message == "boom"));
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn first_error_waits_for_in_flight_calls() {
        let input = numbered_lines(200);
        let started = AtomicUsize::new(0);
        let finished = AtomicUsize::new(0);
        // Chunk 0 fails only once chunk 1 is in flight; chunk 1 ends only after chunk 0 failed,
        // so generate must wait for it. No timing decides the outcome.
        let second_started = Gate::new();
        let failed = Gate::new();
        let complete = |_: &str, u: &str| {
            started.fetch_add(1, Ordering::SeqCst);
            let result = if u.starts_with("line 000") {
                second_started.wait_for(1, "the second call never started");
                failed.raise();
                Err(boom())
            } else {
                second_started.raise();
                failed.wait_for(1, "the first call never failed");
                Ok("n".to_string())
            };
            finished.fetch_add(1, Ordering::SeqCst);
            result
        };
        let err = generate(&request(&input, 300, 2), &complete, &mut |_| {}).unwrap_err();
        assert!(matches!(err, Error::Llm { .. }), "{err}");
        let started = started.load(Ordering::SeqCst);
        assert!(started >= 2, "only {started} calls started");
        assert_eq!(finished.load(Ordering::SeqCst), started);
    }

    #[test]
    fn events_in_order() {
        let input = numbered_lines(200);
        let caller = thread::current().id();
        let complete = |_: &str, _: &str| Ok("n".to_string());
        let mut events = Vec::new();
        generate(&request(&input, 300, 3), &complete, &mut |e| {
            assert_eq!(thread::current().id(), caller);
            events.push(e);
        })
        .unwrap();
        let total = chunk_total(&events);
        let mut want: Vec<Event> = (1..=total)
            .map(|done| Event::Chunk { done, total })
            .collect();
        want.push(Event::Consolidating);
        assert_eq!(events, want);
    }

    const BLOCK: &str = "FIELDS: return a title and a body.";

    /// Recorded (system, user) pairs.
    type Calls = Vec<(String, String)>;

    /// Runs `generate_with` with `BLOCK`, a recording `complete` answering "n" and a `finish`
    /// answering "final". Returns the output, the `complete` calls, the `finish` calls and
    /// the events.
    fn run_with_block(req: &Request) -> (Generated, Calls, Calls, Vec<Event>) {
        let rec = Recorder::new();
        let complete = |s: &str, u: &str| {
            rec.record(s, u);
            Ok("n".to_string())
        };
        let mut finals = Vec::new();
        let mut finish = |s: &str, u: &str, _: &mut dyn FnMut(Event)| {
            finals.push((s.to_string(), u.to_string()));
            Ok("final".to_string())
        };
        let mut events = Vec::new();
        let out = generate_with(req, Some(BLOCK), &complete, &mut finish, &mut |e| {
            events.push(e)
        })
        .unwrap();
        (out, rec.calls(), finals, events)
    }

    #[test]
    fn single_call_final_block_before_language() {
        let (_, _, finals, _) = run_with_block(&request("hello", 1000, 1));
        assert_eq!(
            finals[0].0,
            format!("{BODY}\n\n{BLOCK}\n\n{}", language_instruction(LANG))
        );
    }

    #[test]
    fn finish_used_for_single_call() {
        let (out, calls, finals, _) = run_with_block(&request("hello", 1000, 1));
        assert_eq!(
            out,
            Generated {
                text: "final".into(),
                calls: 1
            }
        );
        assert!(calls.is_empty(), "complete was called: {calls:?}");
        assert_eq!(finals.len(), 1);
        assert_eq!(finals[0].1, "hello");
    }

    #[test]
    fn map_prompts_have_no_final_block() {
        let input = numbered_lines(200);
        let (_, calls, _, events) = run_with_block(&request(&input, 300, 3));
        assert!(chunk_total(&events) > 1);
        for (s, _) in &calls {
            assert!(!s.contains(BLOCK), "map prompt has the final block: {s}");
        }
    }

    #[test]
    fn consolidation_has_final_block() {
        let input = numbered_lines(200);
        let (out, calls, finals, events) = run_with_block(&request(&input, 300, 3));
        let total = chunk_total(&events);
        assert_eq!(
            out,
            Generated {
                text: "final".into(),
                calls: total + 1
            }
        );
        assert_eq!(calls.len(), total);
        assert!(calls.iter().all(|(s, _)| !is_final(s)), "{calls:?}");
        assert_eq!(finals.len(), 1);
        let (system, user) = &finals[0];
        assert_eq!(
            system,
            &format!(
                "{BODY}\n\n{CONSOLIDATION}\n\n{BLOCK}\n\n{}",
                language_instruction(LANG)
            )
        );
        assert_eq!(user, &join_notes(&vec!["n".to_string(); total]));
    }

    #[test]
    fn fit_boundary_counts_final_block() {
        let context = 1000;
        let plain = format!("{BODY}\n\n{}", language_instruction(LANG));
        let with_block = format!("{BODY}\n\n{BLOCK}\n\n{}", language_instruction(LANG));
        let room = |system: &str| (context - context / 4 - estimate_tokens(system)) * 3;
        let exact = "x".repeat(room(&with_block));
        let fits_only_without = "x".repeat(room(&plain));
        assert!(fits(&plain, &fits_only_without, context));
        assert!(!fits(&with_block, &fits_only_without, context));
        let cases = [
            (exact.clone(), true),
            (format!("{exact}x"), false),
            (fits_only_without, false),
        ];
        for (input, one_call) in cases {
            let (out, _, _, _) = run_with_block(&request(&input, context, 1));
            assert_eq!(out.calls == 1, one_call, "input of {} chars", input.len());
        }
    }

    #[test]
    fn final_with_block_never_exceeds_budget() {
        let mut ok = 0;
        let mut chunked = 0;
        for case in 0..300u64 {
            let mut rng = Rng(case.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
            let context = rng.range(60, 3000);
            let body_words = rng.range(0, 60);
            let body = rng.words(body_words, 12).join(" ");
            let block_words = rng.range(1, 40);
            // Upper case keeps the block from occurring by chance in the body or input.
            let block = format!("FIELDS: {}", rng.words(block_words, 12).join(" "));
            let line_count = rng.range(1, 80);
            let lines: Vec<String> = (0..line_count)
                .map(|_| {
                    let n = rng.range(0, 30);
                    let max_len = if rng.range(0, 8) == 0 { 400 } else { 15 };
                    rng.words(n, max_len).join(" ")
                })
                .collect();
            let input = lines.join("\n");
            let concurrency = rng.range(1, 5);
            let req = Request {
                body: &body,
                output_lang: LANG,
                input: &input,
                context_tokens: context,
                concurrency,
            };
            let rec = Recorder::new();
            let complete = |s: &str, u: &str| {
                rec.record(s, u);
                Ok("note".to_string())
            };
            let mut finals = Vec::new();
            let mut finish = |s: &str, u: &str, _: &mut dyn FnMut(Event)| {
                finals.push((s.to_string(), u.to_string()));
                Ok("final".to_string())
            };
            let result = generate_with(&req, Some(&block), &complete, &mut finish, &mut |_| {});
            let calls = rec.calls();
            for (s, u) in &calls {
                assert!(fits(s, u, context), "case {case}: map call over budget");
                assert!(!s.contains(&block), "case {case}: map call has the block");
            }
            for (s, u) in &finals {
                assert!(fits(s, u, context), "case {case}: final call over budget");
                assert!(
                    s.contains(&block),
                    "case {case}: final call lacks the block"
                );
            }
            match result {
                Ok(out) => {
                    ok += 1;
                    if !calls.is_empty() {
                        chunked += 1;
                    }
                    assert_eq!(finals.len(), 1, "case {case}");
                    assert_eq!(out.calls, calls.len() + 1, "case {case}");
                }
                Err(Error::Template { name, .. }) => assert!(name.is_empty(), "case {case}"),
                Err(e) => panic!("case {case}: unexpected error {e}"),
            }
        }
        assert!(ok > 150, "only {ok} cases succeeded");
        assert!(chunked > 50, "only {chunked} cases were chunked");
    }

    #[test]
    fn oversized_final_block_fails_before_map_calls() {
        let input = numbered_lines(200);
        let block = "word ".repeat(200);
        let count = AtomicUsize::new(0);
        let complete = |_: &str, _: &str| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok("n".to_string())
        };
        let mut finish = |_: &str, _: &str, _: &mut dyn FnMut(Event)| Ok("final".to_string());
        let err = generate_with(
            &request(&input, 300, 1),
            Some(&block),
            &complete,
            &mut finish,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(
            matches!(
                err,
                Error::Template { ref name, ref message }
                    if name.is_empty()
                        && message == "the prompt alone does not fit the context budget of 300 tokens"
            ),
            "{err}"
        );
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn finish_events_forwarded() {
        let repair = Event::Repair {
            attempt: 1,
            total: 2,
        };
        let chunked = numbered_lines(200);
        let cases = [("hello".to_string(), false), (chunked, true)];
        for (input, is_chunked) in cases {
            let complete = |_: &str, _: &str| Ok("n".to_string());
            let mut finish = |_: &str, _: &str, on_event: &mut dyn FnMut(Event)| {
                on_event(repair);
                Ok("final".to_string())
            };
            let mut events = Vec::new();
            generate_with(
                &request(&input, 300, 3),
                Some(BLOCK),
                &complete,
                &mut finish,
                &mut |e| events.push(e),
            )
            .unwrap();
            let total = chunk_total(&events);
            let mut want: Vec<Event> = (1..=total)
                .map(|done| Event::Chunk { done, total })
                .collect();
            if is_chunked {
                want.push(Event::Consolidating);
            }
            want.push(repair);
            assert_eq!(events, want, "chunked: {is_chunked}");
        }
    }
}
