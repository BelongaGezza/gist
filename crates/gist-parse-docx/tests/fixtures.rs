//! The checked-in DOCX fixtures must actually parse (a previous revision of
//! `with_tracked_changes.docx` / `minimal_valid.docx` lacked `word/styles.xml`,
//! which the parser requires, so nothing could import them).

use gist_model::ParseLimits;

#[test]
fn tracked_changes_fixture_imports_and_sets_the_flag() {
    let bytes = include_bytes!("../../../fixtures/docx/with_tracked_changes.docx");
    let d = gist_parse_docx::parse(bytes, "t", &ParseLimits::default()).unwrap();
    assert!(d.metadata.tracked_changes);
    assert_eq!(d.metadata.source_type, "docx");
}

#[test]
fn minimal_fixture_imports_without_the_flag() {
    let bytes = include_bytes!("../../../fixtures/docx/minimal_valid.docx");
    let d = gist_parse_docx::parse(bytes, "t", &ParseLimits::default()).unwrap();
    assert!(!d.metadata.tracked_changes);
}
