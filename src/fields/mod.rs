//! Structured fields: the shared field set and the value types (ADR-0006).

pub mod prompt;
pub mod render;
pub mod validate;

use std::fmt;

use crate::chapters::{self, Chapter};

/// Bumped when the fields block, repair prompt, normalization or rendering changes; part of
/// the fingerprint of fields templates only. (The artifact's own `"version"` is separate, ADR-0007.)
pub const FIELDS_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Title,
    Body,
    Tags,
    Hashtags,
    ThumbnailText,
    /// Filled by minuta from `chapters.json`; the model never writes it.
    Chapters,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    List,
    Chapters,
}

impl Field {
    /// Every field, in the order of the spec table.
    pub const ALL: [Field; 6] = [
        Field::Title,
        Field::Body,
        Field::Tags,
        Field::Hashtags,
        Field::ThumbnailText,
        Field::Chapters,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Field::Title => "title",
            Field::Body => "body",
            Field::Tags => "tags",
            Field::Hashtags => "hashtags",
            Field::ThumbnailText => "thumbnail_text",
            Field::Chapters => "chapters",
        }
    }

    /// `None` for unknown names; the parser words that error.
    pub fn from_name(s: &str) -> Option<Field> {
        Field::ALL.into_iter().find(|f| f.name() == s)
    }

    pub fn kind(self) -> Kind {
        match self {
            Field::Title | Field::Body | Field::ThumbnailText => Kind::Text,
            Field::Tags | Field::Hashtags => Kind::List,
            Field::Chapters => Kind::Chapters,
        }
    }

    /// Whether minuta supplies the value instead of the model: such a field stays out of the
    /// fields block, the schema and answer validation. Only `chapters`.
    pub fn is_filled(self) -> bool {
        matches!(self, Field::Chapters)
    }

    /// The description given to the model. Never sent for `chapters` (it is filled, not asked).
    pub fn description(self) -> &'static str {
        match self {
            Field::Title => "A title.",
            Field::Body => "The main text.",
            Field::Tags => "Search keywords, without `#`.",
            Field::Hashtags => "Hashtags, written without the `#` sign.",
            Field::ThumbnailText => "A few words to show on a thumbnail image.",
            Field::Chapters => "Chapters, filled by minuta.",
        }
    }
}

/// Per-field limits: `max_chars` bounds the rendered length, `max` the number of list items.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Limit {
    pub max_chars: Option<usize>,
    pub max: Option<usize>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Format {
    #[default]
    Plain,
    Markdown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSpec {
    pub field: Field,
    pub limit: Limit,
}

/// The fields a template declares, in declared order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldsSpec {
    pub fields: Vec<FieldSpec>,
    pub format: Format,
}

impl FieldsSpec {
    /// Whether the template declares the filled `chapters` field.
    pub fn declares_chapters(&self) -> bool {
        self.fields.iter().any(|f| f.field == Field::Chapters)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Text(String),
    List(Vec<String>),
    Chapters(Vec<Chapter>),
}

impl Value {
    /// Whether this value is of the kind `field` holds. Internal invariant of `Values`: a
    /// `Chapters` value belongs to `Field::Chapters` and to no other field.
    pub(crate) fn fits(&self, field: Field) -> bool {
        match self {
            Value::Text(_) => field.kind() == Kind::Text,
            Value::List(_) => field.kind() == Kind::List,
            Value::Chapters(_) => field.kind() == Kind::Chapters,
        }
    }

    /// The text a reader sees, and what `max_chars` measures.
    pub fn rendered(&self, field: Field) -> String {
        // A mismatched pair is a bug in the caller; the value decides what is printed, so
        // release builds render something sensible instead of panicking.
        debug_assert!(self.fits(field), "{self:?} does not fit field {field:?}");
        match self {
            Value::Text(text) => text.clone(),
            Value::List(items) if field == Field::Hashtags => items
                .iter()
                .map(|item| format!("#{item}"))
                .collect::<Vec<_>>()
                .join(" "),
            Value::List(items) => items.join(", "),
            Value::Chapters(chapters) => chapters::lines(chapters),
        }
    }
}

/// Validated values, in declared order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Values(pub Vec<(Field, Value)>);

impl Values {
    /// Inserts `Value::Chapters(chapters)` at the declared position of `chapters`; unchanged
    /// when the spec does not declare it.
    pub fn fill_chapters(mut self, spec: &FieldsSpec, chapters: Vec<Chapter>) -> Values {
        let Some(position) = spec.fields.iter().position(|f| f.field == Field::Chapters) else {
            return self;
        };
        // The values hold the answered fields only, so the position counts the fields before
        // `chapters` that the model answers.
        let index = spec.fields[..position]
            .iter()
            .filter(|f| !f.field.is_filled())
            .count();
        self.0.insert(
            index.min(self.0.len()),
            (Field::Chapters, Value::Chapters(chapters)),
        );
        self
    }
}

/// One problem with a model answer, worded for the model and for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation(pub String);

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_names_round_trip() {
        assert_eq!(Field::Title.name(), "title");
        assert_eq!(Field::Body.name(), "body");
        assert_eq!(Field::Tags.name(), "tags");
        assert_eq!(Field::Hashtags.name(), "hashtags");
        assert_eq!(Field::ThumbnailText.name(), "thumbnail_text");
        assert_eq!(Field::from_name("title"), Some(Field::Title));
        assert_eq!(Field::from_name("body"), Some(Field::Body));
        assert_eq!(Field::from_name("tags"), Some(Field::Tags));
        assert_eq!(Field::from_name("hashtags"), Some(Field::Hashtags));
        assert_eq!(
            Field::from_name("thumbnail_text"),
            Some(Field::ThumbnailText)
        );
    }

    #[test]
    fn field_descriptions() {
        assert_eq!(Field::Title.description(), "A title.");
        assert_eq!(Field::Body.description(), "The main text.");
        assert_eq!(Field::Tags.description(), "Search keywords, without `#`.");
        assert_eq!(
            Field::Hashtags.description(),
            "Hashtags, written without the `#` sign."
        );
        assert_eq!(
            Field::ThumbnailText.description(),
            "A few words to show on a thumbnail image."
        );
    }

    #[test]
    fn field_kinds() {
        assert_eq!(Field::Title.kind(), Kind::Text);
        assert_eq!(Field::Body.kind(), Kind::Text);
        assert_eq!(Field::ThumbnailText.kind(), Kind::Text);
        assert_eq!(Field::Tags.kind(), Kind::List);
        assert_eq!(Field::Hashtags.kind(), Kind::List);
    }

    #[test]
    fn chapters_from_name() {
        assert_eq!(Field::from_name("chapters"), Some(Field::Chapters));
        assert_eq!(Field::Chapters.name(), "chapters");
    }

    #[test]
    fn unknown_name_is_not_a_field() {
        assert_eq!(Field::from_name("hook"), None);
    }

    #[test]
    fn all_lists_chapters_last() {
        assert_eq!(
            Field::ALL,
            [
                Field::Title,
                Field::Body,
                Field::Tags,
                Field::Hashtags,
                Field::ThumbnailText,
                Field::Chapters,
            ]
        );
    }

    #[test]
    fn chapters_kind_and_description() {
        assert_eq!(Field::Chapters.kind(), Kind::Chapters);
        assert_eq!(Field::Chapters.description(), "Chapters, filled by minuta.");
    }

    #[test]
    fn chapters_is_filled() {
        assert!(Field::Chapters.is_filled());
    }

    #[test]
    fn title_not_filled() {
        assert!(!Field::Title.is_filled());
    }

    #[test]
    fn body_not_filled() {
        assert!(!Field::Body.is_filled());
    }

    #[test]
    fn tags_not_filled() {
        assert!(!Field::Tags.is_filled());
    }

    #[test]
    fn hashtags_not_filled() {
        assert!(!Field::Hashtags.is_filled());
    }

    #[test]
    fn thumbnail_text_not_filled() {
        assert!(!Field::ThumbnailText.is_filled());
    }

    fn chapter(start: u64, title: &str) -> Chapter {
        Chapter {
            start,
            title: title.into(),
        }
    }

    fn chapters() -> Vec<Chapter> {
        vec![chapter(0, "Intro"), chapter(252, "Ownership")]
    }

    #[test]
    fn rendered_chapters_are_lines() {
        assert_eq!(
            Value::Chapters(chapters()).rendered(Field::Chapters),
            "0:00 Intro\n4:12 Ownership"
        );
    }

    fn spec_of(fields: &[Field]) -> FieldsSpec {
        FieldsSpec {
            fields: fields
                .iter()
                .map(|&field| FieldSpec {
                    field,
                    limit: Limit::default(),
                })
                .collect(),
            format: Format::Plain,
        }
    }

    #[test]
    fn declares_chapters_when_listed() {
        assert!(spec_of(&[Field::Body, Field::Chapters]).declares_chapters());
    }

    #[test]
    fn does_not_declare_chapters_when_absent() {
        assert!(!spec_of(&[Field::Body, Field::Tags]).declares_chapters());
    }

    fn body_and_tags() -> Values {
        Values(vec![
            (Field::Body, Value::Text("b".into())),
            (Field::Tags, Value::List(vec!["t".into()])),
        ])
    }

    #[test]
    fn fill_chapters_first() {
        let spec = spec_of(&[Field::Chapters, Field::Body, Field::Tags]);
        assert_eq!(
            body_and_tags().fill_chapters(&spec, chapters()),
            Values(vec![
                (Field::Chapters, Value::Chapters(chapters())),
                (Field::Body, Value::Text("b".into())),
                (Field::Tags, Value::List(vec!["t".into()])),
            ])
        );
    }

    #[test]
    fn fill_chapters_middle() {
        let spec = spec_of(&[Field::Body, Field::Chapters, Field::Tags]);
        assert_eq!(
            body_and_tags().fill_chapters(&spec, chapters()),
            Values(vec![
                (Field::Body, Value::Text("b".into())),
                (Field::Chapters, Value::Chapters(chapters())),
                (Field::Tags, Value::List(vec!["t".into()])),
            ])
        );
    }

    #[test]
    fn fill_chapters_last() {
        let spec = spec_of(&[Field::Body, Field::Tags, Field::Chapters]);
        assert_eq!(
            body_and_tags().fill_chapters(&spec, chapters()),
            Values(vec![
                (Field::Body, Value::Text("b".into())),
                (Field::Tags, Value::List(vec!["t".into()])),
                (Field::Chapters, Value::Chapters(chapters())),
            ])
        );
    }

    #[test]
    fn fill_chapters_undeclared() {
        let spec = spec_of(&[Field::Body, Field::Tags]);
        assert_eq!(
            body_and_tags().fill_chapters(&spec, chapters()),
            body_and_tags()
        );
    }

    #[test]
    fn rendered_text_is_unchanged() {
        assert_eq!(
            Value::Text("Hi there".into()).rendered(Field::Title),
            "Hi there"
        );
    }

    #[test]
    fn rendered_tags_comma_separated() {
        let tags = |items: &[&str]| Value::List(items.iter().map(|s| s.to_string()).collect());
        assert_eq!(tags(&["a", "b"]).rendered(Field::Tags), "a, b");
        assert_eq!(tags(&["a"]).rendered(Field::Tags), "a");
        assert_eq!(tags(&[]).rendered(Field::Tags), "");
    }

    #[test]
    fn rendered_hashtags_with_hash() {
        let tags = |items: &[&str]| Value::List(items.iter().map(|s| s.to_string()).collect());
        assert_eq!(tags(&["a", "b"]).rendered(Field::Hashtags), "#a #b");
        assert_eq!(tags(&["a"]).rendered(Field::Hashtags), "#a");
        assert_eq!(tags(&[]).rendered(Field::Hashtags), "");
    }

    #[test]
    fn violation_displays_its_message() {
        assert_eq!(Violation("too long".into()).to_string(), "too long");
    }
}
