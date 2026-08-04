use fileglyph::model::{is_valid_extension, normalize_extension, IconLocation};

#[test]
fn normalizes_extensions() {
    assert_eq!(normalize_extension("ASD"), ".asd");
    assert_eq!(normalize_extension(".Foo"), ".foo");
}

#[test]
fn parses_registry_icon_locations() {
    let parsed = IconLocation::parse(r#""C:\Program Files\ASD\asd.exe",0"#);
    assert_eq!(parsed.path, r#"C:\Program Files\ASD\asd.exe"#);
    assert_eq!(parsed.index, 0);

    let parsed = IconLocation::parse(r#"%SystemRoot%\System32\imageres.dll,-102"#);
    assert_eq!(parsed.index, -102);
}

#[test]
fn validates_safe_extension_names() {
    assert!(is_valid_extension("asd"));
    assert!(is_valid_extension(".sqlite3"));
    assert!(!is_valid_extension("."));
    assert!(!is_valid_extension("bad/path"));
    assert!(!is_valid_extension("bad name"));
}
