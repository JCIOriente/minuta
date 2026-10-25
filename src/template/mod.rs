//! Template files, frontmatter and variables.

pub mod discover;
pub mod graph;

use std::path::PathBuf;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::lang;
use crate::llm::BackendId;

/// Which layer a template came from; a later layer replaces an earlier one by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    BuiltIn,
    User,
    Project,
}

/// What a template reads: the transcript or the output of another template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    Transcript,
    Template(String),
}

#[derive(Debug, Clone)]
pub struct Template {
    pub name: String,
    pub layer: Layer,
    /// `None` for built-in templates.
    pub path: Option<PathBuf>,
    /// The whole file text, frontmatter included.
    pub raw: String,
    pub description: Option<String>,
    pub input: Input,
    pub llm: Option<BackendId>,
    pub lang: Option<String>,
    pub body: String,
}

/// Values for the template variables.
#[derive(Debug, Clone)]
pub struct Vars {
    pub output_lang: String,
    pub output_lang_code: String,
    pub speech_lang: String,
    pub job_name: String,
    pub duration: String,
}

pub const VARIABLES: [&str; 5] = [
    "output_lang",
    "output_lang_code",
    "speech_lang",
    "job_name",
    "duration",
];

const RESERVED: [&str; 8] = [
    "transcript",
    "chapters",
    "probe",
    "join",
    "audio",
    "transcribe",
    "outputs",
    "job",
];

const BUILTIN: [(&str, &str); 1] = [("summary", include_str!("builtin/summary.md"))];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Frontmatter {
    description: Option<String>,
    input: Option<String>,
    output: Option<String>,
    llm: Option<String>,
    lang: Option<String>,
}

/// True when `name` matches `^[a-z0-9][a-z0-9_-]*$`.
pub(crate) fn valid_stem(name: &str) -> bool {
    let mut chars = name.chars();
    let ok = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    chars.next().is_some_and(ok) && chars.all(|c| ok(c) || c == '_' || c == '-')
}

fn check_name(name: &str) -> Result<()> {
    if !valid_stem(name) {
        return Err(Error::Usage(format!(
            "invalid template name '{name}' (use lowercase letters, digits, '-' and '_', starting with a letter or digit)"
        )));
    }
    if RESERVED.contains(&name) {
        return Err(Error::Usage(format!(
            "template name '{name}' is reserved (reserved: {})",
            RESERVED.join(", ")
        )));
    }
    Ok(())
}

/// Split `text` into (frontmatter, body). `None` when it has no leading `---` line.
fn split_frontmatter(text: &str) -> Result<Option<(&str, &str)>> {
    let mut lines = text.split_inclusive('\n');
    let is_fence = |l: &str| l.trim_end_matches(['\n', '\r']) == "---";
    match lines.next() {
        Some(first) if is_fence(first) => {}
        _ => return Ok(None),
    }
    let mut offset = text.find('\n').map_or(text.len(), |i| i + 1);
    let start = offset;
    for line in lines {
        if is_fence(line) {
            return Ok(Some((&text[start..offset], &text[offset + line.len()..])));
        }
        offset += line.len();
    }
    Err(Error::Usage(
        "frontmatter is not closed (expected a closing '---' line)".into(),
    ))
}

/// Visit each `{{ name }}` in `body`: `f` receives the trimmed name and returns the
/// replacement. An unterminated `{{` is kept as text.
fn expand(body: &str, mut f: impl FnMut(&str) -> String) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(open) = rest.find("{{") {
        let Some(close) = rest[open + 2..].find("}}") else {
            break;
        };
        out.push_str(&rest[..open]);
        out.push_str(&f(rest[open + 2..open + 2 + close].trim()));
        rest = &rest[open + 2 + close + 2..];
    }
    out.push_str(rest);
    out
}

impl Template {
    /// Parse a template file. Every problem is a usage error naming the template.
    pub fn parse(name: &str, layer: Layer, path: Option<PathBuf>, text: &str) -> Result<Template> {
        let fail = |msg: String| Error::Usage(format!("template {name}: {msg}"));
        let place = path
            .as_ref()
            .map_or_else(|| "built-in".to_string(), |p| p.display().to_string());
        check_name(name).map_err(|e| match e {
            Error::Usage(m) => Error::Usage(format!("template {name}: {m}")),
            other => other,
        })?;

        let (fm, body) = match split_frontmatter(text).map_err(|e| match e {
            Error::Usage(m) => fail(format!("{place}: {m}")),
            other => other,
        })? {
            Some((yaml, body)) => {
                // A leading newline makes YAML line numbers equal file line numbers,
                // since the opening `---` is line 1.
                let yaml = format!("\n{yaml}");
                let fm: Option<Frontmatter> = serde_yaml_ng::from_str(&yaml)
                    .map_err(|e| fail(format!("{place}: invalid frontmatter: {e}")))?;
                (fm, body)
            }
            None => (None, text),
        };

        let mut tpl = Template {
            name: name.to_string(),
            layer,
            path,
            raw: text.to_string(),
            description: None,
            input: Input::Transcript,
            llm: None,
            lang: None,
            body: body.to_string(),
        };

        if let Some(fm) = fm {
            tpl.description = fm.description;
            match fm.output.as_deref() {
                None | Some("markdown") => {}
                Some("fields") => {
                    return Err(fail(
                        "'output: fields' arrives in a later version; only 'markdown' is supported"
                            .into(),
                    ))
                }
                Some(other) => {
                    return Err(fail(format!(
                        "unknown output '{other}' (only 'markdown' is supported)"
                    )))
                }
            }
            match fm.input.as_deref() {
                None | Some("transcript") => {}
                Some(dep) => {
                    check_name(dep).map_err(|e| fail(format!("invalid input: {e}")))?;
                    tpl.input = Input::Template(dep.to_string());
                }
            }
            if let Some(id) = fm.llm {
                tpl.llm = Some(
                    id.parse::<BackendId>()
                        .map_err(|e| fail(format!("llm: {e}")))?,
                );
            }
            if let Some(code) = fm.lang {
                lang::validate(&code).map_err(|e| fail(format!("lang: {e}")))?;
                tpl.lang = Some(code);
            }
        }

        let mut unknown = None;
        expand(&tpl.body, |v| {
            if !VARIABLES.contains(&v) && unknown.is_none() {
                unknown = Some(v.to_string());
            }
            String::new()
        });
        if let Some(v) = unknown {
            return Err(fail(format!(
                "unknown variable '{v}' (known: {})",
                VARIABLES.join(", ")
            )));
        }
        Ok(tpl)
    }

    /// Substitute the variables in the body.
    pub fn render(&self, vars: &Vars) -> String {
        expand(&self.body, |v| match v {
            "output_lang" => vars.output_lang.clone(),
            "output_lang_code" => vars.output_lang_code.clone(),
            "speech_lang" => vars.speech_lang.clone(),
            "job_name" => vars.job_name.clone(),
            "duration" => vars.duration.clone(),
            // Unreachable for parsed templates; keep the text rather than lose it.
            other => format!("{{{{{other}}}}}"),
        })
    }
}

/// The compiled-in templates.
pub(crate) fn builtins() -> Result<Vec<Template>> {
    BUILTIN
        .iter()
        .map(|(name, text)| Template::parse(name, Layer::BuiltIn, None, text))
        .collect()
}
