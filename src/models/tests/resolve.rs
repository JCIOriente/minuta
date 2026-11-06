//! `resolve` and `default_dir_for`: from a name or path to a model file.

use super::*;

#[test]
fn resolve_path_vs_name() {
    let dir = tempfile::tempdir().unwrap();
    let models_dir = dir.path().join("models");
    let custom = dir.path().join("custom.bin");
    std::fs::write(&custom, b"x").unwrap();
    let given = custom.to_str().unwrap();
    assert_eq!(resolve(given, &models_dir).unwrap(), custom);
    assert_eq!(
        resolve("small", &models_dir).unwrap(),
        models_dir.join("ggml-small.bin")
    );
    assert_eq!(resolve("nope", &models_dir).unwrap_err().exit_code(), 2);
}

#[test]
fn vad_model_resolves_into_vad_subfolder() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        resolve("silero-v5.1.2", dir.path()).unwrap(),
        dir.path().join("vad").join("ggml-silero-v5.1.2.bin")
    );
}

#[test]
fn whisper_model_resolves_flat() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        resolve("tiny", dir.path()).unwrap(),
        dir.path().join("ggml-tiny.bin")
    );
}

#[test]
fn default_dir_is_minuta_models() {
    let home = Path::new("/home/u");
    assert_eq!(default_dir_for(home), Path::new("/home/u/.minuta/models"));
}

#[test]
fn resolve_does_not_accept_a_directory() {
    let dir = tempfile::tempdir().unwrap();
    let models_dir = dir.path().join("models");
    // A directory is not a model file, so it must not be returned as is.
    let lookalike = dir.path().join("small");
    std::fs::create_dir(&lookalike).unwrap();
    assert_eq!(
        resolve(lookalike.to_str().unwrap(), &models_dir)
            .unwrap_err()
            .exit_code(),
        2
    );
}
