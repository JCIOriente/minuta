use crate::error::{Error, Result};
use crate::fields::prompt::repair_user;
use crate::fields::Violation;
use crate::llm::call::{call_json, Answer};
use crate::llm::chunk::{self, Event, Generated, Request};

use super::{Calls, TemplateRun};

/// What to try when a JSON answer still breaks its limits or rules after every repair call.
const REPAIRS_HINT: &str = "raise llm.repairs, relax the limit in a project template, \
                            or use a stronger backend with --llm";

/// A plain-text call through the call cache, as `chunk::generate_with` takes it.
type Complete<'a> = &'a (dyn Fn(&str, &str) -> Result<String> + Sync);

/// What the shared repair loop needs from a JSON output (fields or chapters).
pub(super) struct JsonOutput<'a, T> {
    /// Goes before the language instruction in the final system prompt.
    pub(super) block: String,
    /// The system prompt of every repair call.
    pub(super) repair_system: String,
    pub(super) schema: serde_json::Value,
    /// Parses and checks a cleaned answer.
    pub(super) accept: &'a dyn Fn(&str) -> std::result::Result<T, Vec<Violation>>,
    /// What a violation breaks, as the exhausted message words it: "limit" or "rule".
    pub(super) broken: &'static str,
}

impl TemplateRun<'_> {
    /// Generates a JSON output (fields or chapters): the final call asks for JSON with
    /// `output.schema`, and a rejected answer gets up to `repairs` repair calls, each built from
    /// the previous answer and its violations (ADR-0006). Map calls use `complete` as for
    /// Markdown.
    pub(super) fn generate_json<T>(
        &self,
        request: &Request,
        complete: Complete,
        calls: &Calls,
        output: &JsonOutput<T>,
        on_event: &mut dyn FnMut(Event),
    ) -> Result<(Generated, T)> {
        let accept = |text: &str| (output.accept)(text).map(|value| (value, text.to_owned()));
        let ask = |system: &str, user: &str| {
            call_json(
                self.backend,
                &calls.cache,
                calls.policy,
                system,
                user,
                &output.schema,
                self.forced,
                calls.interrupt,
                &accept,
            )
        };
        let mut accepted = None;
        let mut repair_calls = 0;
        let mut finish = |system: &str, user: &str, on_event: &mut dyn FnMut(Event)| {
            let mut answer = ask(system, user)?;
            loop {
                let (text, violations) = match answer {
                    Answer::Accepted((value, text)) => {
                        accepted = Some(value);
                        return Ok(text);
                    }
                    Answer::Rejected { text, violations } => (text, violations),
                };
                if repair_calls == self.repairs {
                    return Err(self.repairs_exhausted(&violations, output.broken));
                }
                repair_calls += 1;
                on_event(Event::Repair {
                    attempt: repair_calls as usize,
                    total: self.repairs as usize,
                });
                answer = ask(&output.repair_system, &repair_user(&text, &violations))?;
            }
        };
        let mut generated = chunk::generate_with(
            request,
            Some(&output.block),
            complete,
            &mut finish,
            on_event,
        )?;
        // `generate_with` counts the final call once; add the repair calls.
        generated.calls += repair_calls as usize;
        let value = accepted.expect("finish returned Ok only after accepting an answer");
        Ok((generated, value))
    }

    /// `broken` is what one violation breaks: "limit" or "rule".
    fn repairs_exhausted(&self, violations: &[Violation], broken: &str) -> Error {
        let count = violations.len();
        let broken = if count == 1 {
            broken.to_owned()
        } else {
            format!("{broken}s")
        };
        let repairs = if self.repairs == 1 {
            "repair"
        } else {
            "repairs"
        };
        let list: Vec<&str> = violations
            .iter()
            .map(|v| v.0.trim_end_matches('.'))
            .collect();
        self.error(format!(
            "the answer still breaks {count} {broken} after {} {repairs}: {}; {REPAIRS_HINT}",
            self.repairs,
            list.join("; ")
        ))
    }
}
