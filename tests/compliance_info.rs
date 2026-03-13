// Compliance tests for the info diagram parser.
//
// These tests verify that `parse_info` correctly handles the Mermaid info
// diagram syntax, translated from the upstream Mermaid test suites:
//   - packages/mermaid/src/diagrams/info/info.spec.ts
//   - packages/parser/tests/info.test.ts

use mmdflux::mermaid::parse_info;

// ---------------------------------------------------------------------------
// Minimal input
// ---------------------------------------------------------------------------

#[test]
fn info_minimal_newline() {
    let result = parse_info("info\n").unwrap();
    assert!(!result.show_info);
    assert!(result.title.is_none());
}

#[test]
fn info_minimal_no_trailing_newline() {
    // Upstream accepts bare "info" without trailing newline.
    // Grammar requires line_end (newline | EOI), so EOI should satisfy it.
    let result = parse_info("info");
    assert!(result.is_ok(), "bare 'info' should parse: {:?}", result);
    let info = result.unwrap();
    assert!(!info.show_info);
    assert!(info.title.is_none());
}

// ---------------------------------------------------------------------------
// showInfo flag
// ---------------------------------------------------------------------------

#[test]
fn info_show_info() {
    let result = parse_info("info\nshowInfo\n").unwrap();
    assert!(result.show_info);
    assert!(result.title.is_none());
}

#[test]
fn info_show_info_no_trailing_newline() {
    let result = parse_info("info\nshowInfo");
    assert!(
        result.is_ok(),
        "showInfo without trailing newline should parse: {:?}",
        result
    );
    assert!(result.unwrap().show_info);
}

// ---------------------------------------------------------------------------
// Title
// ---------------------------------------------------------------------------

#[test]
fn info_with_title() {
    let result = parse_info("info\ntitle My Info\n").unwrap();
    assert_eq!(result.title.as_deref(), Some("My Info"));
    assert!(!result.show_info);
}

#[test]
fn info_with_title_no_trailing_newline() {
    let result = parse_info("info\ntitle My Info");
    assert!(
        result.is_ok(),
        "title without trailing newline should parse: {:?}",
        result
    );
    assert_eq!(result.unwrap().title.as_deref(), Some("My Info"));
}

// ---------------------------------------------------------------------------
// showInfo + title combined
// ---------------------------------------------------------------------------

#[test]
fn info_show_info_and_title() {
    let result = parse_info("info\nshowInfo\ntitle My Info\n").unwrap();
    assert!(result.show_info);
    assert_eq!(result.title.as_deref(), Some("My Info"));
}

// ---------------------------------------------------------------------------
// Whitespace tolerance (from upstream parser tests)
// ---------------------------------------------------------------------------

#[test]
fn info_leading_whitespace() {
    // Upstream: leading blank line before keyword should parse.
    let result = parse_info("\n  info\n");
    assert!(
        result.is_ok(),
        "leading blank line should be tolerated: {:?}",
        result
    );
}

#[test]
fn info_trailing_whitespace_on_keyword_line() {
    // Upstream: trailing spaces on keyword line should be tolerated.
    // Grammar uses implicit WHITESPACE so trailing spaces are consumed.
    let result = parse_info("info   \n");
    assert!(
        result.is_ok(),
        "trailing spaces on keyword line: {:?}",
        result
    );
}

#[test]
fn info_show_info_same_line() {
    // Upstream: `info showInfo`
    let result = parse_info("info showInfo\n");
    assert!(result.is_ok(), "same-line showInfo: {:?}", result);
    assert!(result.unwrap().show_info);
}

// ---------------------------------------------------------------------------
// Invalid input
// ---------------------------------------------------------------------------

#[test]
fn info_invalid_keyword() {
    let result = parse_info("not info\n");
    assert!(result.is_err(), "non-info input should be rejected");
}

#[test]
fn info_unsupported_keyword_after_info() {
    // Upstream: `info unsupported` should reject.
    let result = parse_info("info unsupported\n");
    assert!(result.is_err(), "'info unsupported' should be rejected");
}

#[test]
fn info_empty_input() {
    let result = parse_info("");
    assert!(result.is_err(), "empty input should be rejected");
}

#[test]
fn info_only_whitespace() {
    let result = parse_info("   \n  \n");
    assert!(result.is_err(), "whitespace-only input should be rejected");
}
