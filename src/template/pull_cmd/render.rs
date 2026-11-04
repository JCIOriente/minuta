use crate::cli::sanitize;
use crate::template::pull::{outcome_for, Item, Outcome};

/// The file of template `name`.
pub(super) fn md(name: &str) -> String {
    format!("{name}.md")
}

pub(super) fn file_name(item: &Item<'_>) -> String {
    md(&item.entry.name)
}

/// What the prompt says will happen to a file: derived from the outcome `apply` will have.
pub(super) fn prompt_note(item: &Item<'_>, force: bool) -> String {
    match outcome_for(item.state, force) {
        Outcome::Created => "new".to_string(),
        Outcome::Updated => "update available".to_string(),
        Outcome::Overwrote => format!(
            "modified: replaced, old file saved as {}.bak",
            file_name(item)
        ),
        Outcome::KeptModified => "modified: skipped, use --force to overwrite".to_string(),
        Outcome::KeptUpToDate => "up to date: skipped".to_string(),
    }
}

pub(super) fn result_text(outcome: Outcome, name: &str) -> String {
    match outcome {
        Outcome::Created => "written".to_string(),
        Outcome::Updated => "updated".to_string(),
        Outcome::Overwrote => format!("overwritten (backup: {}.bak)", md(name)),
        Outcome::KeptUpToDate => "up to date".to_string(),
        Outcome::KeptModified => "skipped (modified; use --force to overwrite)".to_string(),
    }
}

/// The catalog as columns: name, platform, state and description. The text of the catalog
/// comes from the network, so it is made safe to print.
pub(super) fn table(items: &[Item<'_>]) -> String {
    let mut rows = vec![[
        "NAME".to_string(),
        "PLATFORM".to_string(),
        "STATE".to_string(),
        "DESCRIPTION".to_string(),
    ]];
    for item in items {
        rows.push([
            sanitize(&item.entry.name),
            sanitize(item.entry.platform.as_deref().unwrap_or("-")),
            item.state.label().to_string(),
            sanitize(&item.entry.description),
        ]);
    }
    let width = |col: usize| {
        rows.iter()
            .map(|r| r[col].chars().count())
            .max()
            .unwrap_or(0)
    };
    let (w0, w1, w2) = (width(0), width(1), width(2));
    let mut out = String::new();
    for row in &rows {
        let line = format!(
            "{:<w0$}  {:<w1$}  {:<w2$}  {}",
            row[0], row[1], row[2], row[3]
        );
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// The numbered list shown when no template was named: number, name, state, description.
pub(super) fn numbered(items: &[Item<'_>]) -> String {
    let w0 = items
        .iter()
        .map(|i| i.entry.name.chars().count())
        .max()
        .unwrap_or(0);
    let w1 = items
        .iter()
        .map(|i| i.state.label().chars().count())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for (index, item) in items.iter().enumerate() {
        let line = format!(
            "{:>2}) {:<w0$}  {:<w1$}  {}",
            index + 1,
            sanitize(&item.entry.name),
            item.state.label(),
            sanitize(&item.entry.description)
        );
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}
