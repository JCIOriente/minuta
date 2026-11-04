use std::sync::Mutex;

use super::*;
use crate::error::Error;
use crate::transcript::Segment;

pub(super) const BODY: &str = "Write meeting minutes.";
pub(super) const LANG: &str = "Spanish";

pub(super) fn seg(start: f64, end: f64, text: &str) -> Segment {
    Segment {
        start,
        end,
        text: text.into(),
    }
}

pub(super) fn transcript(segments: Vec<Segment>) -> Transcript {
    Transcript {
        language: "en".into(),
        segments,
    }
}

pub(super) fn numbered_lines(n: usize) -> String {
    (0..n)
        .map(|i| format!("line {i:03}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn request<'a>(
    input: &'a str,
    context_tokens: usize,
    concurrency: usize,
) -> Request<'a> {
    Request {
        body: BODY,
        output_lang: LANG,
        input,
        context_tokens,
        concurrency,
    }
}

pub(super) fn fits(system: &str, user: &str, context: usize) -> bool {
    estimate_tokens(system) + estimate_tokens(user) + context / 4 <= context
}

/// The `i` in "This is part i of n." of a map system prompt.
pub(super) fn part_index(system: &str) -> usize {
    let rest = system.split("This is part ").nth(1).expect("map prompt");
    rest.split(' ').next().unwrap().parse().unwrap()
}

pub(super) fn is_final(system: &str) -> bool {
    system.contains(CONSOLIDATION)
}

/// Records every (system, user) pair and answers with `reply`.
pub(super) struct Recorder {
    calls: Mutex<Vec<(String, String)>>,
}

impl Recorder {
    pub(super) fn new() -> Self {
        Recorder {
            calls: Mutex::new(Vec::new()),
        }
    }
    pub(super) fn record(&self, system: &str, user: &str) {
        self.calls
            .lock()
            .unwrap()
            .push((system.to_string(), user.to_string()));
    }
    pub(super) fn calls(&self) -> Vec<(String, String)> {
        self.calls.lock().unwrap().clone()
    }
}

/// The map calls (the first `total` calls), sorted by part index.
pub(super) fn map_calls(calls: &[(String, String)], total: usize) -> Vec<(String, String)> {
    let mut maps = calls[..total].to_vec();
    maps.sort_by_key(|(s, _)| part_index(s));
    maps
}

pub(super) fn chunk_total(events: &[Event]) -> usize {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Chunk { total, .. } => Some(*total),
            Event::Consolidating | Event::Merge { .. } | Event::Repair { .. } => None,
        })
        .next()
        .unwrap_or(0)
}

/// xorshift64*: deterministic, good enough to vary test inputs.
pub(super) struct Rng(pub(super) u64);

impl Rng {
    pub(super) fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    pub(super) fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + (self.next() % (hi - lo) as u64) as usize
    }
    pub(super) fn words(&mut self, n: usize, max_len: usize) -> Vec<String> {
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

pub(super) fn boom() -> Error {
    Error::Llm {
        backend: "fake".into(),
        message: "boom".into(),
        transient: false,
    }
}

pub(super) const BLOCK: &str = "FIELDS: return a title and a body.";

/// Recorded (system, user) pairs.
pub(super) type Calls = Vec<(String, String)>;

/// Runs `generate_with` with `BLOCK`, a recording `complete` answering "n" and a `finish`
/// answering "final". Returns the output, the `complete` calls, the `finish` calls and
/// the events.
pub(super) fn run_with_block(req: &Request) -> (Generated, Calls, Calls, Vec<Event>) {
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
