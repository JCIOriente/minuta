use super::*;

#[test]
fn apply_hash_mismatch_writes_nothing() {
    let d = Dest::new();
    let good = template_text("good");
    let bad = template_text("tampered");
    let a = entry("alpha", &good);
    let b = entry("beta", &template_text("expected"));
    let mut f = Fetcher::new(&[("alpha", good.as_bytes()), ("beta", bad.as_bytes())]);
    let mut manifest = Manifest::default();
    let text = err_text(run(&d.dir(), &[&a, &b], false, &mut f, &mut manifest));
    assert_eq!(
        text,
        format!(
            "template beta: downloaded file does not match the catalog checksum (expected {}, got {})",
            b.sha256,
            sha256_hex(bad.as_bytes())
        )
    );
    assert!(!d.dir().join("alpha.md").exists());
    assert!(!d.dir().join("beta.md").exists());
    assert!(!d.dir().join(".pulled.toml").exists());
    assert_eq!(manifest, Manifest::default());
}

#[test]
fn apply_unparsable_template_writes_nothing() {
    let d = Dest::new();
    let good = template_text("good");
    let bad = "---\ndescription: x\nbogus: 1\n---\nBody.\n";
    let a = entry("alpha", &good);
    let b = entry("beta", bad);
    let mut f = Fetcher::new(&[("alpha", good.as_bytes()), ("beta", bad.as_bytes())]);
    let text = err_text(run(
        &d.dir(),
        &[&a, &b],
        false,
        &mut f,
        &mut Manifest::default(),
    ));
    assert!(
        text.starts_with("template beta: invalid template from the catalog: "),
        "{text}"
    );
    assert!(text.contains("bogus"), "{text}");
    // The reason is the parse error without its own "template beta: " prefix.
    assert!(
        !text.contains("from the catalog: template beta:"),
        "doubled prefix: {text}"
    );
    assert!(
        text.ends_with("; if it uses a newer format, upgrade minuta"),
        "{text}"
    );
    assert!(!d.dir().join("alpha.md").exists());
    assert!(!d.dir().join("beta.md").exists());
}

#[test]
fn apply_file_that_is_not_utf8_writes_nothing() {
    let d = Dest::new();
    let bytes: &[u8] = &[0xff, 0xfe, 0x00];
    let mut a = entry("alpha", "x");
    a.sha256 = sha256_hex(bytes); // the hash matches, so only the encoding is wrong
    let mut f = Fetcher::new(&[("alpha", bytes)]);
    assert_eq!(
        err_text(run(
            &d.dir(),
            &[&a],
            false,
            &mut f,
            &mut Manifest::default()
        )),
        "template alpha: downloaded file is not valid UTF-8"
    );
    assert!(!d.dir().join("alpha.md").exists());
}

#[test]
fn apply_fetch_error_writes_nothing() {
    let d = Dest::new();
    let good = template_text("good");
    let a = entry("alpha", &good);
    let b = entry("beta", &template_text("missing on the server"));
    let mut f = Fetcher::new(&[("alpha", good.as_bytes())]); // beta has no file
    let text = err_text(run(
        &d.dir(),
        &[&a, &b],
        false,
        &mut f,
        &mut Manifest::default(),
    ));
    assert_eq!(text, "I/O error: no fake file for beta");
    assert!(!d.dir().join("alpha.md").exists());
}

#[test]
fn apply_only_fetches_what_it_will_write() {
    let d = Dest::new();
    let kept = template_text("kept");
    d.write("kept.md", &kept);
    let fresh = template_text("fresh");
    let k = entry("kept", &kept);
    let n = entry("fresh", &fresh);
    let mut f = Fetcher::new(&[("fresh", fresh.as_bytes())]);
    let mut manifest = manifest_with(&[("kept", &kept)]);
    run(&d.dir(), &[&k, &n], false, &mut f, &mut manifest).unwrap();
    assert_eq!(f.calls, ["fresh"]);
}
