//! Prompt assembly, chunking and consolidation.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;

use crate::error::{Error, Result};
use crate::transcript::Transcript;

mod pack;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests_map;
#[cfg(test)]
mod tests_reduce;
#[cfg(test)]
mod tests_text;

use pack::{join_notes, part_units, split_inputs};

/// Version of the prompt wrappers below. It is part of every template step's fingerprint.
pub(crate) const PROMPT_VERSION: u32 = 1;

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
pub(crate) fn transcript_text(t: &Transcript) -> String {
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
pub(crate) fn estimate_tokens(s: &str) -> usize {
    s.chars().count().div_ceil(3)
}

/// The sentence every system prompt ends with. `output_lang` is a language name ("Spanish").
pub(crate) fn language_instruction(output_lang: &str) -> String {
    format!("{LANGUAGE_PREFIX}{output_lang}{LANGUAGE_SUFFIX}")
}

pub(crate) struct Request<'a> {
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
pub(crate) enum Event {
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
pub(crate) struct Generated {
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
pub(crate) fn generate(
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
pub(crate) fn generate_with(
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
