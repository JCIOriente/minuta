//! Tests for the template catalog, the manifest of pulled files, template states and
//! selection, split by behavior.

use super::*;

mod docs;
mod manifest;
mod parse;
mod repo;
mod select;

fn a() -> String {
    "a".repeat(64)
}

fn b() -> String {
    "b".repeat(64)
}

fn c() -> String {
    "c".repeat(64)
}

fn catalog_text(entries: &str) -> String {
    format!("version = 1\n{entries}")
}

fn entry_text(name: &str, sha: &str) -> String {
    format!("\n[[template]]\nname = \"{name}\"\ndescription = \"d\"\nsha256 = \"{sha}\"\n")
}

fn parse_err(text: &str) -> String {
    match Catalog::parse(text) {
        Err(Error::Parse(m)) => m,
        other => panic!("expected parse error, got {other:?}"),
    }
}

fn usage(r: Result<impl std::fmt::Debug>) -> String {
    match r {
        Err(Error::Usage(m)) => m,
        other => panic!("expected usage error, got {other:?}"),
    }
}

// ---- state ----

#[test]
fn state_new() {
    assert_eq!(state(None, None, &a()), State::New);
}

#[test]
fn state_new_even_when_manifest_remembers_it() {
    assert_eq!(state(None, Some(&a()), &a()), State::New);
}

#[test]
fn state_up_to_date() {
    assert_eq!(state(Some(&a()), Some(&a()), &a()), State::UpToDate);
}

#[test]
fn state_update_available() {
    assert_eq!(state(Some(&a()), Some(&a()), &b()), State::UpdateAvailable);
}

#[test]
fn state_modified_when_local_differs_from_manifest() {
    assert_eq!(state(Some(&c()), Some(&a()), &b()), State::Modified);
}

#[test]
fn state_modified_when_local_differs_from_manifest_and_catalog_unchanged() {
    assert_eq!(state(Some(&c()), Some(&a()), &a()), State::Modified);
}

#[test]
fn state_modified_when_no_manifest_entry() {
    assert_eq!(state(Some(&c()), None, &b()), State::Modified);
}

#[test]
fn state_identical_to_catalog_is_up_to_date_without_manifest_entry() {
    assert_eq!(state(Some(&b()), None, &b()), State::UpToDate);
}

#[test]
fn state_identical_to_catalog_is_up_to_date_when_manifest_differs() {
    assert_eq!(state(Some(&b()), Some(&a()), &b()), State::UpToDate);
}

#[test]
fn state_labels() {
    assert_eq!(State::New.label(), "new");
    assert_eq!(State::UpToDate.label(), "up to date");
    assert_eq!(State::UpdateAvailable.label(), "update available");
    assert_eq!(State::Modified.label(), "modified");
}

// ---- hash ----

#[test]
fn sha256_hex_matches_known_vector() {
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn sha256_hex_of_empty_input() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}
