//! Sequence diagram compliance tests and snapshot assertions.
//!
//! Locks sequence rendering output with deterministic text snapshots.
//! Generate snapshots: `GENERATE_SEQUENCE_TEXT_SNAPSHOTS=1 cargo test --test compliance_sequence`

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use mmdflux::builtins::default_registry;
use mmdflux::graph::measure::{
    COMPATIBILITY_TEXT_METRICS_PROFILE_ID, RECORDED_SANS_TEXT_METRICS_PROFILE_ID,
};
use mmdflux::{OutputFormat, RenderConfig};

fn sequence_fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("sequence")
}

fn sequence_text_snapshot_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("sequence")
}

fn list_sequence_fixtures() -> Vec<String> {
    let dir = sequence_fixture_dir();
    let mut fixtures: Vec<String> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("Failed to read sequence fixtures dir: {e}"))
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension().is_some_and(|e| e == "mmd") {
                Some(path.file_name()?.to_str()?.to_string())
            } else {
                None
            }
        })
        .collect();
    fixtures.sort();
    fixtures
}

fn read_sequence_fixture(fixture: &str) -> String {
    let path = sequence_fixture_dir().join(fixture);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to read fixture {fixture}: {e}"))
}

fn render_sequence_text(fixture: &str) -> String {
    render_sequence_text_with_config(fixture, &RenderConfig::default())
}

fn render_sequence_text_with_config(fixture: &str, config: &RenderConfig) -> String {
    let input = read_sequence_fixture(fixture);
    mmdflux::render_diagram(&input, OutputFormat::Text, config)
        .expect("Failed to render sequence text fixture")
}

// --- Text snapshots ---

#[test]
fn sequence_text_snapshots() {
    let snapshot_dir = sequence_text_snapshot_dir();
    let regenerate = std::env::var("GENERATE_SEQUENCE_TEXT_SNAPSHOTS").is_ok();

    if regenerate {
        fs::create_dir_all(&snapshot_dir).unwrap();
    }

    for fixture in list_sequence_fixtures() {
        let stem = fixture.trim_end_matches(".mmd");
        let output = render_sequence_text(&fixture);
        let snapshot_path = snapshot_dir.join(format!("{stem}.txt"));

        if regenerate {
            fs::write(&snapshot_path, &output).unwrap();
        } else {
            let expected = fs::read_to_string(&snapshot_path).unwrap_or_else(|_| {
                panic!(
                    "Missing sequence text snapshot: {}. Set GENERATE_SEQUENCE_TEXT_SNAPSHOTS=1 to generate.",
                    snapshot_path.display()
                )
            });
            assert_eq!(
                output, expected,
                "Sequence text snapshot mismatch for {fixture}. Set GENERATE_SEQUENCE_TEXT_SNAPSHOTS=1 to regenerate."
            );
        }
    }
}

// --- Compliance assertions ---

#[test]
fn sequence_all_fixtures_parse() {
    for fixture in list_sequence_fixtures() {
        let path = sequence_fixture_dir().join(&fixture);
        let input = fs::read_to_string(&path).unwrap();
        let instance = default_registry()
            .create("sequence")
            .expect("sequence should be registered");
        assert!(
            instance.parse(&input).is_ok(),
            "Failed to parse sequence fixture: {fixture}"
        );
    }
}

#[test]
fn sequence_all_fixtures_render_text() {
    for fixture in list_sequence_fixtures() {
        let output = render_sequence_text(&fixture);
        assert!(
            !output.is_empty(),
            "Empty text output for sequence fixture: {fixture}"
        );
    }
}

#[test]
fn sequence_all_fixtures_render_ascii() {
    for fixture in list_sequence_fixtures() {
        let input = read_sequence_fixture(&fixture);
        let output = mmdflux::render_diagram(&input, OutputFormat::Ascii, &RenderConfig::default())
            .expect("render failed");
        assert!(
            !output.is_empty(),
            "Empty ASCII output for sequence fixture: {fixture}"
        );
    }
}

fn render_sequence_svg(fixture: &str) -> String {
    render_sequence_svg_with_config(fixture, &RenderConfig::default())
}

fn render_sequence_svg_with_config(fixture: &str, config: &RenderConfig) -> String {
    let input = read_sequence_fixture(fixture);
    mmdflux::render_diagram(&input, OutputFormat::Svg, config)
        .expect("Failed to render sequence SVG")
}

fn render_sequence_mmds_with_config(fixture: &str, config: &RenderConfig) -> String {
    let input = read_sequence_fixture(fixture);
    mmdflux::render_diagram(&input, OutputFormat::Mmds, config)
        .expect("Failed to render sequence MMDS")
}

fn sequence_svg_snapshot_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("svg-snapshots")
        .join("sequence")
}

#[test]
fn sequence_svg_snapshots() {
    let snapshot_dir = sequence_svg_snapshot_dir();
    let regenerate = std::env::var("GENERATE_SEQUENCE_SVG_SNAPSHOTS").is_ok();

    if regenerate {
        fs::create_dir_all(&snapshot_dir).unwrap();
    }

    for fixture in list_sequence_fixtures() {
        let stem = fixture.trim_end_matches(".mmd");
        let output = render_sequence_svg(&fixture);
        let snapshot_path = snapshot_dir.join(format!("{stem}.svg"));

        if regenerate {
            fs::write(&snapshot_path, &output).unwrap();
        } else {
            let expected = fs::read_to_string(&snapshot_path).unwrap_or_else(|_| {
                panic!(
                    "Missing sequence SVG snapshot: {}. Set GENERATE_SEQUENCE_SVG_SNAPSHOTS=1 to generate.",
                    snapshot_path.display()
                )
            });
            assert_eq!(
                output, expected,
                "Sequence SVG snapshot mismatch for {fixture}. Set GENERATE_SEQUENCE_SVG_SNAPSHOTS=1 to regenerate."
            );
        }
    }
}

#[test]
fn sequence_all_fixtures_render_svg() {
    for fixture in list_sequence_fixtures() {
        let output = render_sequence_svg(&fixture);
        assert!(
            output.starts_with("<svg"),
            "SVG output should start with <svg for {fixture}"
        );
        assert!(
            output.contains("</svg>"),
            "SVG output should contain closing tag for {fixture}"
        );
    }
}

#[test]
fn sequence_rendering_ignores_graph_family_font_metrics_profile_contract() {
    let fixture = "simple.mmd";
    let default_text = render_sequence_text(fixture);
    let default_svg = render_sequence_svg(fixture);
    let default_mmds = render_sequence_mmds_with_config(fixture, &RenderConfig::default());

    for profile in [
        RECORDED_SANS_TEXT_METRICS_PROFILE_ID,
        COMPATIBILITY_TEXT_METRICS_PROFILE_ID,
    ] {
        let config = RenderConfig {
            font_metrics_profile: Some(profile.to_string()),
            ..RenderConfig::default()
        };

        assert_eq!(
            render_sequence_text_with_config(fixture, &config),
            default_text,
            "sequence Text changed for graph-family profile {profile}"
        );
        assert_eq!(
            render_sequence_svg_with_config(fixture, &config),
            default_svg,
            "sequence SVG changed for graph-family profile {profile}"
        );
        assert_eq!(
            render_sequence_mmds_with_config(fixture, &config),
            default_mmds,
            "sequence MMDS changed for graph-family profile {profile}"
        );
    }
}

#[test]
fn sequence_dashed_uses_different_line_char() {
    let solid = render_sequence_text("simple.mmd");
    let dashed = render_sequence_text("dashed.mmd");
    // Dashed fixture should contain dotted horizontal char
    assert!(dashed.contains('┄'), "dashed output should use ┄");
    // Solid fixture should NOT contain dotted horizontal char
    assert!(!solid.contains('┄'), "solid output should not use ┄");
}

#[test]
fn sequence_autonumber_prefixes() {
    let output = render_sequence_text("autonumber.mmd");
    assert!(output.contains("1."), "should contain number 1");
    assert!(output.contains("2."), "should contain number 2");
    assert!(output.contains("3."), "should contain number 3");
}

#[test]
fn sequence_autonumber_controls_apply_start_step_and_resume() {
    let output = render_sequence_text("autonumber_controls.mmd");
    assert!(output.contains("10. Login request"));
    assert!(output.contains("12. Challenge"));
    assert!(output.contains("Background ping"));
    assert!(output.contains("14. Session ready"));
    assert!(!output.contains("13. Background ping"));
}

#[test]
fn sequence_title_renders_above_participants() {
    let output = render_sequence_text("title.mmd");
    let mut lines = output.lines();
    assert_eq!(lines.next().map(str::trim), Some("Authentication Flow"));
    assert!(
        output.contains("Alice") && output.contains("Bob"),
        "participant headers should remain present"
    );
}

#[test]
fn sequence_lifecycle_fixtures_render_create_and_destroy_markers() {
    let create_output = render_sequence_text("create_participant.mmd");
    let destroy_output = render_sequence_text("destroy_participant.mmd");

    assert!(create_output.contains("Create Bob"));
    assert!(create_output.contains("Hello Alice"));
    assert!(destroy_output.contains("XXX"));
}

#[test]
fn sequence_rendering_deterministic() {
    for fixture in list_sequence_fixtures() {
        let out1 = render_sequence_text(&fixture);
        let out2 = render_sequence_text(&fixture);
        assert_eq!(out1, out2, "Non-deterministic output for {fixture}");
    }
}

#[test]
fn sequence_interaction_operators_render_block_labels() {
    let alt = render_sequence_text("alt_else.mmd");
    let loop_output = render_sequence_text("loop.mmd");
    let opt = render_sequence_text("opt.mmd");
    let par = render_sequence_text("par_and.mmd");
    let critical = render_sequence_text("critical_option.mmd");
    let break_output = render_sequence_text("break_block.mmd");

    assert!(alt.contains("[alt] available"));
    assert!(alt.contains("[else] busy"));
    assert!(loop_output.contains("[loop] Every 5 seconds"));
    assert!(opt.contains("[opt] Extra data needed"));
    assert!(par.contains("[par] Notifications"));
    assert!(par.contains("[and]"));
    assert!(critical.contains("[critical] Establish connection"));
    assert!(critical.contains("[option] Timeout"));
    assert!(break_output.contains("[break] Success"));
}

#[test]
fn sequence_svg_interaction_operators_use_operator_tabs() {
    let svg = render_sequence_svg("alt_else.mmd");

    assert!(
        svg.contains("<polygon "),
        "expected SVG operator tab polygon"
    );
    assert!(svg.contains(">alt</text>"), "expected alt operator tab");
    assert!(
        svg.contains(">Check status</text>"),
        "message text should remain present"
    );
    assert!(
        svg.contains("[available]</text>"),
        "expected bracketed fragment guard"
    );
    assert!(
        svg.contains("[busy]</text>"),
        "expected bracketed else guard"
    );
}

#[test]
fn sequence_participant_boxes_render_group_labels() {
    let output = render_sequence_text("participant_boxes.mmd");
    let unlabeled = render_sequence_text("participant_box_color_only.mmd");

    assert!(output.contains("Frontend"));
    assert!(output.contains("Backend"));
    assert!(unlabeled.contains("Alice"));
    assert!(unlabeled.contains("Bob"));
}

#[test]
fn sequence_svg_participant_boxes_render_group_backgrounds() {
    let svg = render_sequence_svg("participant_boxes.mmd");
    let color_only_svg = render_sequence_svg("participant_box_color_only.mmd");

    assert!(svg.contains("participant-boxes"));
    assert!(svg.contains(">Frontend</text>"));
    assert!(svg.contains(">Backend</text>"));
    assert!(svg.contains("fill=\"lightblue\""));
    assert!(color_only_svg.contains("fill=\"aqua\""));
}

#[test]
fn sequence_svg_title_renders() {
    let svg = render_sequence_svg("title.mmd");

    assert!(svg.contains("<g class=\"title\">"));
    assert!(svg.contains(">Authentication Flow</text>"));
    assert!(svg.contains(">Login request</text>"));
}

fn text_width(output: &str) -> usize {
    output
        .lines()
        .map(mmdflux::format::display_width)
        .max()
        .unwrap_or(0)
}

#[test]
fn sequence_text_keeps_long_participant_names_whole() {
    let output = render_sequence_text("long_participant_names.mmd");
    for label in [
        "A participant with a very long display name",
        "Another participant with a long name too",
    ] {
        assert!(output.contains(label), "missing {label:?} in\n{output}");
    }
}

#[test]
fn sequence_text_sizes_cjk_text_in_cells() {
    let output = render_sequence_text("cjk_participants.mmd");
    for text in [
        "部署フロー",
        "日本語の参加者",
        "こんにちは世界、元気ですか",
        "注意してください",
    ] {
        assert!(output.contains(text), "missing {text:?} in\n{output}");
    }
    // Byte-measured layout drew this 59 cells wide.
    assert!(
        text_width(&output) < 59,
        "{}\n{output}",
        text_width(&output)
    );
}

fn render_sequence_source(input: &str) -> String {
    mmdflux::render_diagram(input, OutputFormat::Text, &RenderConfig::default())
        .expect("Failed to render sequence text")
}

#[test]
fn sequence_text_keeps_combining_marks_and_joined_emoji_whole() {
    // Decomposed accents (e + U+0301) and ZWJ-joined emoji take more canvas
    // cells than their terminal width; boxes must leave room for every one.
    for label in ["e\u{301}e\u{301}e\u{301}", "👩\u{200d}💻👩\u{200d}💻"] {
        let input = format!(
            "sequenceDiagram\nparticipant A as {label}\nparticipant B as Bob\nA->>B: hi\nNote right of A: {label}\n"
        );
        let output = render_sequence_source(&input);
        assert_eq!(
            output.matches(label).count(),
            2,
            "{label:?} not whole in\n{output}"
        );
    }
}
