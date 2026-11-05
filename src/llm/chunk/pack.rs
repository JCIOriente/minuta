//! Splitting inputs into pieces that fit the context budget, and joining notes into parts.

use super::{input_budget, SystemFor};
use crate::error::Result;

/// Pack `units` into inputs that fit next to `system_for(i, n)` for every part i of n.
/// The budget is taken from the longest prompt, "part n of n"; n is raised until the
/// resulting count has no more digits than the n assumed.
pub(super) fn split_inputs(
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

pub(super) fn part_units(notes: &[String]) -> Vec<String> {
    notes
        .iter()
        .enumerate()
        .map(|(i, note)| format!("## Part {}\n\n{}", i + 1, note.trim()))
        .collect()
}

pub(super) fn join_notes(notes: &[String]) -> String {
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

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::error::Error;
    use crate::llm::chunk::test_support::{chunk_total, fits, map_calls, Recorder, Rng, LANG};
    use crate::llm::chunk::{generate, Request};

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
}
