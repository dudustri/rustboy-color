use super::*;

#[test]
fn percent_escapes_are_decoded() {
    assert_eq!(percent_decode("my%20game.gb"), "my game.gb");
    assert_eq!(percent_decode("plain.js"), "plain.js");
}

#[test]
fn climbing_out_of_the_root_is_refused() {
    let root = fs::canonicalize(".").unwrap();
    assert_eq!(resolve("/../../etc/passwd", &root), None);
    assert_eq!(resolve("/%2e%2e/secret", &root), None);
}

#[test]
fn wasm_gets_the_type_browsers_insist_on() {
    assert_eq!(content_type(Path::new("a.wasm")), "application/wasm");
    assert_eq!(
        content_type(Path::new("a.mystery")),
        "application/octet-stream"
    );
}
