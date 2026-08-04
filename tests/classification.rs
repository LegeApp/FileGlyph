use fileglyph::classify::{classify, is_protected_extension, make_label};
use fileglyph::model::Category;

#[test]
fn classifies_representative_extensions() {
    assert_eq!(classify("pdf", None, None, None, None), Category::Document);
    assert_eq!(
        classify("xlsx", None, None, None, None),
        Category::Spreadsheet
    );
    assert_eq!(classify("jp2", None, None, None, None), Category::Image);
    assert_eq!(
        classify("sqlite3", None, None, None, None),
        Category::Database
    );
    assert_eq!(classify("rs", None, None, None, None), Category::Code);
}

#[test]
fn disambiguates_ts_from_metadata() {
    assert_eq!(
        classify("ts", Some("video/mp2t"), None, None, None),
        Category::Video
    );
    assert_eq!(
        classify("ts", Some("text/plain"), None, Some("VSCode.ts"), None),
        Category::Code
    );
}

#[test]
fn protects_executable_and_shell_types() {
    assert!(is_protected_extension("exe"));
    assert!(is_protected_extension(".lnk"));
    assert!(!is_protected_extension(".asd"));
}

#[test]
fn labels_preserve_identity() {
    assert_eq!(make_label(".jpeg", 4), "JPG");
    assert_eq!(make_label(".sqlite3", 4), "SQLT");
    assert_eq!(make_label(".jsonlines", 4), "JSNL");
}
