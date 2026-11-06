use std::collections::BTreeMap;

use super::*;
use crate::config::{Config, Overrides, Source};

/// A key line of the template once uncommented: `<key> = <value>` or `[table]`.
fn is_setting(line: &str) -> bool {
    let is_key = line.split_once(" = ").is_some_and(|(key, _)| {
        !key.is_empty() && key.chars().all(|c| c.is_ascii_lowercase() || c == '_')
    });
    let is_table = line
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .is_some_and(|name| !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase()));
    is_key || is_table
}

/// The template with every commented key and table line uncommented; prose stays a comment.
fn uncommented(template: &str) -> String {
    template
        .lines()
        .map(|line| match line.strip_prefix("# ") {
            Some(rest) if is_setting(rest) => rest,
            _ => line,
        })
        .map(|line| format!("{line}\n"))
        .collect()
}

/// Every `key = value` in the template, commented or not, by dotted key name.
fn template_keys(template: &str) -> BTreeMap<String, String> {
    let mut table = String::new();
    let mut keys = BTreeMap::new();
    for line in uncommented(template).lines() {
        if let Some(name) = line.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            table = name.to_string();
        } else if let Some((key, value)) = line.split_once(" = ") {
            if !is_setting(line) {
                continue;
            }
            let dotted = if table.is_empty() {
                key.to_string()
            } else {
                format!("{table}.{key}")
            };
            keys.insert(dotted, value.to_string());
        }
    }
    keys
}

fn defaults() -> Config {
    Config::resolve(Overrides::default(), &|_| None, None).unwrap()
}

#[test]
fn template_covers_every_key() {
    let keys = template_keys(TEMPLATE);
    let config = defaults();
    let entries = config.entries();
    let entry_names: Vec<&str> = entries.iter().map(|(k, _)| *k).collect();
    let template_names: Vec<&str> = keys.keys().map(String::as_str).collect();
    let mut sorted = entry_names.clone();
    sorted.sort_unstable();
    assert_eq!(
        template_names, sorted,
        "template keys must match Config::entries()"
    );

    // Each key is commented out: only the `[whisper]` header is live.
    let live: Vec<&str> = TEMPLATE
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    assert_eq!(live, ["[whisper]"]);

    // Each key with a default shows exactly that default once uncommented.
    let parsed = resolve_uncommented();
    for ((key, default), (parsed_key, from_file)) in entries.iter().zip(parsed.entries()) {
        assert_eq!(*key, parsed_key);
        let from_file = from_file.unwrap_or_else(|| panic!("{key}: no value in the template"));
        assert_eq!(from_file.source, Source::File, "key {key}");
        if let Some(default) = default {
            assert_eq!(from_file.value, default.value, "key {key}");
        }
    }
}

#[test]
fn template_parses_when_uncommented() {
    let config = resolve_uncommented();
    let want: Vec<(&str, &str)> = vec![
        ("out_dir", "./minuta-out"),
        ("whisper.bin", "whisper-cli"),
        ("whisper.model", "small"),
        ("whisper.lang", "auto"),
        ("whisper.vad_model", "silero-v5.1.2"),
        ("whisper.models_dir", "~/.minuta/models"),
        ("ffmpeg.bin", "ffmpeg"),
        ("ffprobe.bin", "ffprobe"),
        ("llm.default", "ollama:gemma4:26b"),
        ("llm.output_lang", "en"),
        ("llm.concurrency", "2"),
        ("llm.retries", "3"),
        ("llm.backoff_ms", "2000"),
        ("llm.repairs", "2"),
        ("ollama.url", "http://127.0.0.1:11434"),
        ("ollama.num_ctx", "16384"),
        ("claude.bin", "claude"),
        ("codex.bin", "codex"),
    ];
    let got: Vec<(&str, &str)> = config
        .entries()
        .into_iter()
        .map(|(k, s)| (k, s.map_or("(unset)", |s| s.value.as_str())))
        .collect();
    assert_eq!(got, want);
}

fn resolve_uncommented() -> Config {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, uncommented(TEMPLATE)).unwrap();
    Config::resolve(Overrides::default(), &|_| None, Some(path.as_path())).unwrap()
}

#[test]
fn documents_codex_bin() {
    assert!(
        TEMPLATE.contains("# [codex]\n# bin = \"codex\"\n"),
        "{TEMPLATE}"
    );
}

#[test]
fn documents_endpoints() {
    assert!(TEMPLATE.ends_with(
        "# Any OpenAI-compatible server, such as LM Studio or Colibri. Name it here, then use\n\
         # `--llm lmstudio:<model>`. Repeat the table for each server.\n\
         #\n\
         # [endpoints.lmstudio]\n\
         #   Base URL, including /v1. Required.\n\
         #   url = \"http://127.0.0.1:1234/v1\"\n\
         #   Prompt and reply budget per call. Must fit the context the server gives the model.\n\
         #   context_tokens = 16384\n\
         #   Seconds to wait for one reply. Raise it for slow models.\n\
         #   timeout_secs = 600\n\
         #   The name of the environment variable holding the API key, never the key itself.\n\
         #   api_key_env = \"MY_SERVER_KEY\"\n"
    ));
}

#[test]
fn endpoint_example_parses_once_uncommented() {
    let example = TEMPLATE
        .split_once("# [endpoints.lmstudio]")
        .map(|(_, rest)| format!("[endpoints.lmstudio]{rest}"))
        .unwrap();
    let text: String = example
        .lines()
        .filter(|line| !line.starts_with("#   ") || line.contains(" = "))
        .map(|line| line.strip_prefix("#   ").unwrap_or(line))
        .map(|line| format!("{line}\n"))
        .collect();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, text).unwrap();
    let cfg = Config::resolve(Overrides::default(), &|_| None, Some(&path)).unwrap();
    let endpoint = &cfg.endpoints["lmstudio"];
    assert_eq!(endpoint.url.value, "http://127.0.0.1:1234/v1");
    assert_eq!(endpoint.context_tokens(), 16384);
    assert_eq!(
        endpoint.api_key_env.as_ref().unwrap().value,
        "MY_SERVER_KEY"
    );
}

/// The library half of the binary tests in `tests/cli/config_init.rs`, which compare the file
/// `config init` writes with the same fixture.
#[test]
fn template_matches_the_committed_fixture() {
    assert_eq!(
        TEMPLATE,
        include_str!("../../tests/fixtures/config_init/config.toml")
    );
}

/// The library half of `config_init_creates_file_and_parent` in `tests/cli/config_init.rs`,
/// which compares `config show` after a fresh `config init` with the same fixture.
#[test]
fn defaults_match_the_committed_show_fixture() {
    let rendered: String = defaults()
        .entries()
        .into_iter()
        .map(|(key, setting)| match setting {
            Some(s) => format!("{key} = {} (default)\n", s.value),
            None => format!("{key} = (unset)\n"),
        })
        .collect();
    assert_eq!(
        rendered,
        include_str!("../../tests/fixtures/config_init/show.txt")
    );
}
