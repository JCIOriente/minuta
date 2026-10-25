//! Structured fields: the shared field set and the value types (ADR-0006).

pub mod prompt;
pub mod render;
pub mod validate;

use std::fmt;

/// Version of the JSON artifact layout (ADR-0007).
pub const FIELDS_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Title,
    Body,
    Tags,
    Hashtags,
    ThumbnailText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    List,
}

impl Field {
    /// Every field, in the order of the spec table.
    pub const ALL: [Field; 5] = [
        Field::Title,
        Field::Body,
        Field::Tags,
        Field::Hashtags,
        Field::ThumbnailText,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Field::Title => "title",
            Field::Body => "body",
            Field::Tags => "tags",
            Field::Hashtags => "hashtags",
            Field::ThumbnailText => "thumbnail_text",
        }
    }

    /// `None` for unknown names, including the reserved `chapters`; the parser words that error.
    pub fn from_name(s: &str) -> Option<Field> {
        Field::ALL.into_iter().find(|f| f.name() == s)
    }

    pub fn kind(self) -> Kind {
        match self {
            Field::Title | Field::Body | Field::ThumbnailText => Kind::Text,
            Field::Tags | Field::Hashtags => Kind::List,
        }
    }

    /// The description given to the model.
    pub fn description(self) -> &'static str {
        match self {
            Field::Title => "A title.",
            Field::Body => "The main text.",
            Field::Tags => "Search keywords, without `#`.",
            Field::Hashtags => "Hashtags, written without the `#` sign.",
            Field::ThumbnailText => "A few words to show on a thumbnail image.",
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Text(String),
    List(Vec<String>),
}

impl Value {
    /// The text a reader sees, and what `max_chars` measures.
    pub fn rendered(&self, field: Field) -> String {
        match self {
            Value::Text(text) => text.clone(),
            Value::List(items) if field == Field::Hashtags => items
                .iter()
                .map(|item| format!("#{item}"))
                .collect::<Vec<_>>()
                .join(" "),
            Value::List(items) => items.join(", "),
        }
    }
}

/// Validated values, in declared order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Values(pub Vec<(Field, Value)>);

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
        assert_eq!(Field::from_name(Field::Title.name()), Some(Field::Title));
        assert_eq!(Field::from_name(Field::Body.name()), Some(Field::Body));
        assert_eq!(Field::from_name(Field::Tags.name()), Some(Field::Tags));
        assert_eq!(
            Field::from_name(Field::Hashtags.name()),
            Some(Field::Hashtags)
        );
        assert_eq!(
            Field::from_name(Field::ThumbnailText.name()),
            Some(Field::ThumbnailText)
        );
        assert_eq!(Field::ThumbnailText.name(), "thumbnail_text");
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
    fn chapters_is_not_a_field() {
        assert_eq!(Field::from_name("chapters"), None);
        assert_eq!(Field::from_name("hook"), None);
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
