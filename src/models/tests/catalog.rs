//! `CATALOG`: the pinned models and their entries.

use super::*;

#[test]
fn catalog_lists_pinned_models() {
    let names: Vec<_> = CATALOG.iter().map(|m| m.name).collect();
    assert_eq!(
        names,
        [
            "tiny",
            "base",
            "small",
            "medium",
            "large-v3",
            "silero-v5.1.2"
        ]
    );
    for m in CATALOG {
        assert_eq!(m.sha256.len(), 64);
        assert!(m.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
    }
}

fn catalog_spec(name: &str) -> &'static ModelSpec<'static> {
    CATALOG.iter().find(|m| m.name == name).unwrap()
}

#[test]
fn catalog_silero_vad_entry() {
    let spec = catalog_spec("silero-v5.1.2");
    assert_eq!(spec.kind, ModelKind::Vad);
    assert_eq!(spec.file, "ggml-silero-v5.1.2.bin");
    assert_eq!(
        spec.base_url,
        "https://huggingface.co/ggml-org/whisper-vad/resolve/main"
    );
    assert_eq!(
        spec.url(),
        "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v5.1.2.bin"
    );
    assert_eq!(
        spec.sha256,
        "29940d98d42b91fbd05ce489f3ecf7c72f0a42f027e4875919a28fb4c04ea2cf"
    );
    assert_eq!(spec.size, 885_098);
}

#[test]
fn catalog_whisper_entry_keeps_its_url() {
    let spec = catalog_spec("small");
    assert_eq!(spec.kind, ModelKind::Whisper);
    assert_eq!(spec.file, "ggml-small.bin");
    assert_eq!(
        spec.url(),
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin"
    );
}
