#![cfg(feature = "encoding-detection")]
//! SC-01: encoding detection against the synthetic fixtures in
//! `fixtures/txt/encodings/`.

use gist_model::ParseLimits;
use gist_parse_txt::{parse_with_encoding, EncodingChoice, EncodingSource, ParsedText};
use std::path::PathBuf;

fn load(name: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/txt/encodings")
        .join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("read fixture {name}: {e}"))
}

fn run(name: &str, choice: EncodingChoice<'_>) -> ParsedText {
    parse_with_encoding(&load(name), "t", &ParseLimits::default(), choice).unwrap()
}

fn text(p: &ParsedText) -> String {
    p.document.sections[0]
        .blocks
        .iter()
        .map(|b| b.plain_text())
        .collect::<Vec<_>>()
        .join("|")
}

#[test]
fn utf8_bom_fixture() {
    let p = run("utf8_bom.txt", EncodingChoice::Auto);
    assert_eq!(p.encoding.source, EncodingSource::Bom);
    assert!(text(&p).starts_with("The old lighthouse"));
}

#[test]
fn utf16_bom_fixtures() {
    for (f, name) in [
        ("utf16le_bom.txt", "UTF-16LE"),
        ("utf16be_bom.txt", "UTF-16BE"),
    ] {
        let p = run(f, EncodingChoice::Auto);
        assert_eq!(p.encoding.name, name);
        assert!(!p.encoding.uncertain && !p.encoding.lossy);
        let t = text(&p);
        assert!(t.starts_with("The old lighthouse"), "{t}");
        assert!(t.contains("café 日本"), "{t}");
    }
}

#[test]
fn utf16_without_bom_fixture() {
    let p = run("utf16le_nobom.txt", EncodingChoice::Auto);
    assert_eq!(p.encoding.name, "UTF-16LE");
    assert_eq!(p.encoding.source, EncodingSource::Detected);
    assert_eq!(p.document.sections[0].blocks.len(), 2);
}

#[test]
fn windows_1252_fixture_with_override() {
    let p = run(
        "windows1252_french.txt",
        EncodingChoice::Named("windows-1252"),
    );
    assert_eq!(p.encoding.source, EncodingSource::Override);
    let t = text(&p);
    assert!(t.contains("café") && t.contains("croîssants"));
    assert!(t.contains('\u{2014}'));
}

#[test]
fn windows_1252_fixture_auto_is_decoded_without_loss() {
    let p = run("windows1252_french.txt", EncodingChoice::Auto);
    assert_eq!(p.encoding.source, EncodingSource::Detected);
    assert!(!p.encoding.lossy);
    assert!(text(&p).contains("café"));
}

#[test]
fn windows_1251_fixture_override_decodes_cyrillic() {
    let p = run(
        "windows1251_russian.txt",
        EncodingChoice::Named("windows-1251"),
    );
    assert!(text(&p).starts_with("Старый маяк"));
}

#[test]
fn windows_1251_fixture_auto_picks_a_cyrillic_encoding() {
    let p = run("windows1251_russian.txt", EncodingChoice::Auto);
    assert!(
        text(&p).contains("маяк") || p.encoding.uncertain,
        "auto result was wrong and not flagged: {} / {}",
        p.encoding.name,
        text(&p)
    );
}

#[test]
fn invalid_utf8_fixture_is_decoded_not_rejected() {
    let p = run("invalid_utf8_mixed.txt", EncodingChoice::Auto);
    assert_ne!(p.encoding.source, EncodingSource::Utf8);
    assert!(text(&p).starts_with("Valid start"));
}

#[test]
fn forcing_utf8_on_invalid_fixture_reports_lossy() {
    let p = run("invalid_utf8_mixed.txt", EncodingChoice::Named("utf-8"));
    assert!(p.encoding.lossy);
    assert!(text(&p).contains('\u{FFFD}'));
}
