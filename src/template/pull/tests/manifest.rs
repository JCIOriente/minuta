use super::*;

#[test]
fn read_manifest_without_a_file_is_empty() {
    let d = Dest::new();
    d.create();
    let (manifest, warning) = read_manifest(&d.dir()).unwrap();
    assert_eq!(manifest, Manifest::default());
    assert_eq!(warning, None);
}

#[test]
fn read_manifest_without_the_folder_is_empty() {
    let d = Dest::new();
    let (manifest, warning) = read_manifest(&d.dir()).unwrap();
    assert_eq!(manifest, Manifest::default());
    assert_eq!(warning, None);
}

#[test]
fn read_manifest_reads_a_valid_file() {
    let d = Dest::new();
    let m = manifest_with(&[("alpha", "a")]);
    d.write(".pulled.toml", &m.to_toml());
    let (read, warning) = read_manifest(&d.dir()).unwrap();
    assert_eq!(read, m);
    assert_eq!(warning, None);
}

#[test]
fn read_manifest_invalid_file_is_empty_with_a_warning() {
    let d = Dest::new();
    d.write(".pulled.toml", "version = [");
    let (manifest, warning) = read_manifest(&d.dir()).unwrap();
    assert_eq!(manifest, Manifest::default());
    let w = warning.expect("a warning");
    assert_eq!(
        w,
        "invalid manifest, treating every file as modified: TOML parse error at line 1, column 12\n  |\n1 | version = [\n  |            ^\nunclosed array, expected `]`\n"
    );
}

#[test]
fn read_manifest_not_utf8_is_empty_with_a_warning() {
    let d = Dest::new();
    let dir = d.create();
    fs::write(dir.join(".pulled.toml"), [0xff, 0xfe]).unwrap();
    let (manifest, warning) = read_manifest(&dir).unwrap();
    assert_eq!(manifest, Manifest::default());
    let w = warning.expect("a warning");
    assert_eq!(
        w,
        "invalid manifest, treating every file as modified: TOML parse error at line 1, column 3\n  |\n1 | \u{fffd}\u{fffd}\n  |   ^\nkey with no value, expected `=`\n"
    );
}

#[test]
fn read_manifest_io_error_names_the_path() {
    let d = Dest::new();
    let dir = d.create();
    let path = dir.join(".pulled.toml");
    fs::create_dir(&path).unwrap(); // reading a directory fails with something other than NotFound
    let text = err_text(read_manifest(&dir));
    assert!(
        text.starts_with(&format!("I/O error: cannot read {}: ", path.display())),
        "{text}"
    );
}

#[cfg(unix)]
#[test]
fn read_manifest_refuses_a_symlink() {
    let d = Dest::new();
    let dir = d.create();
    let secret = d.outside("secret.txt", "version = 1\n");
    let path = dir.join(".pulled.toml");
    std::os::unix::fs::symlink(&secret, &path).unwrap();
    assert_eq!(
        err_text(read_manifest(&dir)),
        format!(
            "I/O error: {} is a symlink; refusing to use it",
            path.display()
        )
    );
}

#[test]
fn read_manifest_io_error_keeps_the_error_kind() {
    let d = Dest::new();
    let dir = d.create();
    fs::create_dir(dir.join(".pulled.toml")).unwrap();
    match read_manifest(&dir) {
        Err(Error::Io(e)) => assert_ne!(e.kind(), std::io::ErrorKind::Other, "{e}"),
        other => panic!("expected an I/O error, got {other:?}"),
    }
}

#[test]
fn read_manifest_over_the_local_cap_is_an_error() {
    let d = Dest::new();
    let path = d.write(".pulled.toml", &"#".repeat(LOCAL_CAP as usize + 1));
    assert_eq!(
        err_text(read_manifest(&d.dir())),
        format!("I/O error: {} is larger than 1048576 bytes", path.display())
    );
}
