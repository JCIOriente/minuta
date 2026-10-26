//! Template files, frontmatter and variables.

pub mod discover;
pub mod graph;

use std::path::PathBuf;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::fields::{Field, FieldSpec, FieldsSpec, Format, Kind, Limit};
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

/// What a template produces: free Markdown or a set of structured fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    Markdown,
    Fields(FieldsSpec),
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
    pub output: Output,
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

const BUILTIN: [(&str, &str); 4] = [
    ("summary", include_str!("builtin/summary.md")),
    ("youtube", include_str!("builtin/youtube.md")),
    ("linkedin", include_str!("builtin/linkedin.md")),
    ("instagram", include_str!("builtin/instagram.md")),
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Frontmatter {
    description: Option<String>,
    input: Option<String>,
    output: Option<String>,
    llm: Option<String>,
    lang: Option<String>,
    fields: Option<Vec<String>>,
    limits: Option<Limits>,
    format: Option<String>,
}

/// The `limits` map as written, keeping repeated keys so they can be rejected.
struct Limits(Vec<(String, LimitYaml)>);

impl<'de> Deserialize<'de> for Limits {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Limits;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a map from field name to limits")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Limits, A::Error> {
                let mut pairs = Vec::new();
                while let Some(pair) = map.next_entry()? {
                    pairs.push(pair);
                }
                Ok(Limits(pairs))
            }
        }
        d.deserialize_map(V)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LimitYaml {
    max_chars: Option<u64>,
    max: Option<u64>,
}

/// True when `name` matches `^[a-z0-9][a-z0-9_-]*$`.
pub(crate) fn valid_stem(name: &str) -> bool {
    let mut chars = name.chars();
    let ok = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    chars.next().is_some_and(ok) && chars.all(|c| ok(c) || c == '_' || c == '-')
}

pub(crate) fn invalid_name_message(name: &str) -> String {
    format!(
        "invalid template name '{name}' (use lowercase letters, digits, '-' and '_', starting with a letter or digit)"
    )
}

fn check_name(name: &str) -> Result<()> {
    if !valid_stem(name) {
        return Err(Error::Usage(invalid_name_message(name)));
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
    let is_fence = |l: &str| l.trim_end() == "---";
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

/// Build the fields spec from the frontmatter keys, checked against the shared field set.
fn fields_spec(
    names: Option<Vec<String>>,
    limits: Option<Limits>,
    format: Option<String>,
) -> std::result::Result<FieldsSpec, String> {
    let names = names.ok_or("'output: fields' requires 'fields'")?;
    if names.is_empty() {
        return Err("'fields' must not be empty".into());
    }
    let mut declared: Vec<Field> = Vec::new();
    for name in &names {
        let field = match Field::from_name(name) {
            Some(f) => f,
            None => {
                let known: Vec<&str> = Field::ALL.iter().map(|f| f.name()).collect();
                return Err(format!(
                    "unknown field {name:?} (known: {})",
                    known.join(", ")
                ));
            }
        };
        if declared.contains(&field) {
            return Err(format!("duplicate field {name:?}"));
        }
        declared.push(field);
    }
    if declared == [Field::Chapters] {
        return Err("'fields' needs at least one field besides 'chapters'".into());
    }
    let mut specs: Vec<FieldSpec> = declared
        .iter()
        .map(|&field| FieldSpec {
            field,
            limit: Limit::default(),
        })
        .collect();
    let mut seen: Vec<String> = Vec::new();
    for (key, raw) in limits.map_or_else(Vec::new, |l| l.0) {
        if seen.contains(&key) {
            return Err(format!("duplicate limit for {key:?}"));
        }
        seen.push(key.clone());
        if key == Field::Chapters.name() {
            return Err("the chapters field is limited in the chapters template".into());
        }
        let Some(spec) =
            Field::from_name(&key).and_then(|f| specs.iter_mut().find(|s| s.field == f))
        else {
            return Err(format!("limit for {key:?}, which is not in 'fields'"));
        };
        let positive = |what: &str, v: Option<u64>| match v {
            Some(0) => Err(format!("'{what}' of {key:?} must be at least 1")),
            other => Ok(other.map(|n| usize::try_from(n).unwrap_or(usize::MAX))),
        };
        spec.limit.max_chars = positive("max_chars", raw.max_chars)?;
        spec.limit.max = positive("max", raw.max)?;
        if spec.limit.max.is_some() && spec.field.kind() == Kind::Text {
            return Err(format!(
                "{key:?} is a text field; 'max' applies to lists only"
            ));
        }
    }
    let format = match format.as_deref() {
        None | Some("plain") => Format::Plain,
        Some("markdown") => Format::Markdown,
        Some(other) => return Err(format!("unknown format '{other}' (use plain or markdown)")),
    };
    Ok(FieldsSpec {
        fields: specs,
        format,
    })
}

impl Template {
    /// Parse a template file. Every problem is a usage error naming the template.
    pub fn parse(name: &str, layer: Layer, path: Option<PathBuf>, text: &str) -> Result<Template> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
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
            output: Output::Markdown,
            llm: None,
            lang: None,
            body: body.to_string(),
        };

        if let Some(fm) = fm {
            tpl.description = fm.description;
            let is_fields = match fm.output.as_deref() {
                None | Some("markdown") => false,
                Some("fields") => true,
                Some(other) => {
                    return Err(fail(format!(
                        "unknown output '{other}' (use 'markdown' or 'fields')"
                    )))
                }
            };
            if is_fields {
                tpl.output =
                    Output::Fields(fields_spec(fm.fields, fm.limits, fm.format).map_err(fail)?);
            } else {
                for (key, present) in [
                    ("fields", fm.fields.is_some()),
                    ("limits", fm.limits.is_some()),
                    ("format", fm.format.is_some()),
                ] {
                    if present {
                        return Err(fail(format!("'{key}' requires 'output: fields'")));
                    }
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
