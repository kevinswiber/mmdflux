//! Public-surface tests for fitted text rendering.

use std::fs;
use std::path::{Path, PathBuf};

use mmdflux::graph::{Direction, GeometryLevel};
use mmdflux::mmds::Document;
use mmdflux::mmds::diff::union::{UnionOptions, union_document};
use mmdflux::{
    CellSize, DirectionChange, FitConfigInput, FitLever, FitLeverScope, FitOptions, FitOutcome,
    Fitted, OutputFormat, RenderConfig, TextColorMode, materialize_diagram, render_diagram,
    render_diagram_fitted, render_document, render_document_fitted, render_document_with_relayout,
    render_document_with_relayout_fitted,
};
use unicode_width::UnicodeWidthStr;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn fixture(family: &str, name: &str) -> String {
    fs::read_to_string(fixtures_dir().join(family).join(format!("{name}.mmd")))
        .expect("fixture should exist")
}

/// Every top-level `.mmd` fixture of `family`, sorted by name.
fn family_fixtures(family: &str) -> Vec<(String, String)> {
    let mut paths: Vec<PathBuf> = fs::read_dir(fixtures_dir().join(family))
        .expect("fixture directory should exist")
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "mmd"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            (name, fs::read_to_string(&path).unwrap())
        })
        .collect()
}

fn strip_ansi(input: &str) -> String {
    let mut stripped = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && matches!(chars.peek(), Some('[')) {
            chars.next();
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        stripped.push(ch);
    }
    stripped
}

fn size_tuple(size: CellSize) -> (usize, usize) {
    (size.width, size.height)
}

fn output_size(output: &str) -> (usize, usize) {
    let plain = strip_ansi(output);
    let lines: Vec<&str> = plain.trim_end_matches('\n').lines().collect();
    let width = lines.iter().map(|line| line.width()).max().unwrap_or(0);
    (width, lines.len())
}

fn fitted(input: &str, fit: &FitOptions) -> Fitted {
    render_diagram_fitted(input, OutputFormat::Text, &RenderConfig::default(), fit)
        .expect("fitted render should succeed")
}

fn has_direction_lever(levers: &[FitLever]) -> bool {
    levers
        .iter()
        .any(|lever| matches!(lever, FitLever::Direction { .. }))
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[test]
fn fit_options_builders_chain() {
    let options = FitOptions::max_width(80);
    assert_eq!(options.max_width, Some(80));
    assert_eq!(options.max_height, None);
    assert_eq!(options.direction_change, DirectionChange::Allow);
    assert!(!options.truncation);

    let options = FitOptions::max_width(40)
        .with_max_height(10)
        .with_direction_change(DirectionChange::Keep)
        .with_truncation(true);
    assert_eq!(options.max_width, Some(40));
    assert_eq!(options.max_height, Some(10));
    assert_eq!(options.direction_change, DirectionChange::Keep);
    assert!(options.truncation);
}

#[test]
fn fit_config_input_converts_to_fit_options() {
    let input: FitConfigInput =
        serde_json::from_str(r#"{"maxWidth":80,"fitDirection":"keep","truncate":true}"#).unwrap();
    assert_eq!(
        input.into_fit_options().unwrap(),
        FitOptions::max_width(80)
            .with_direction_change(DirectionChange::Keep)
            .with_truncation(true)
    );

    let empty: FitConfigInput = serde_json::from_str("{}").unwrap();
    assert_eq!(empty.into_fit_options().unwrap(), FitOptions::default());
}

#[test]
fn fit_config_input_rejects_unknown_keys_and_zero_budgets() {
    assert!(serde_json::from_str::<FitConfigInput>(r#"{"width":80}"#).is_err());

    let zero: FitConfigInput = serde_json::from_str(r#"{"maxWidth":0}"#).unwrap();
    let error = zero.into_fit_options().unwrap_err();
    assert!(error.message.contains("max width must be at least 1"));

    let zero: FitConfigInput = serde_json::from_str(r#"{"maxHeight":0}"#).unwrap();
    let error = zero.into_fit_options().unwrap_err();
    assert!(error.message.contains("max height must be at least 1"));
}

#[test]
fn fit_lever_display_strings_match_the_cli_contract() {
    let cases = [
        (
            FitLever::Direction {
                from: Direction::LeftRight,
                to: Direction::TopDown,
            },
            "direction LR→TD",
        ),
        (
            FitLever::SubgraphDirectionStrip,
            "subgraph directions relaxed",
        ),
        (FitLever::LabelAwareSpacing, "label-aware spacing"),
        (
            FitLever::GridGap {
                rank_gap: 2,
                node_gap: 1,
            },
            "gaps rank 2 node 1",
        ),
        (
            FitLever::EdgeLabelWrap { cells: 16 },
            "edge labels wrapped at 16",
        ),
        (
            FitLever::NodeLabelWrap { cells: 24 },
            "node labels wrapped at 24",
        ),
        (
            FitLever::MemberWrap { cells: 16 },
            "class members wrapped at 16",
        ),
        (
            FitLever::LabelTruncation { cells: 12 },
            "labels truncated to 12",
        ),
        (
            FitLever::MemberElision { keep: 3 },
            "class members elided to 3",
        ),
    ];
    for (lever, expected) in cases {
        assert_eq!(lever.to_string(), expected);
    }
}

// ---------------------------------------------------------------------------
// render_diagram_fitted
// ---------------------------------------------------------------------------

fn assert_identity(family: &str, name: &str, input: &str, config: &RenderConfig) {
    for format in [OutputFormat::Text, OutputFormat::Ascii] {
        let expected = render_diagram(input, format, config).unwrap();
        for fit in [FitOptions::default(), FitOptions::max_width(100_000)] {
            let result = render_diagram_fitted(input, format, config, &fit).unwrap();
            assert_eq!(
                result.output, expected,
                "{family}/{name} {format:?} {fit:?}: fitted output differs"
            );
            let report = &result.report;
            assert_eq!(report.outcome, FitOutcome::AsAuthored, "{family}/{name}");
            assert!(report.applied.is_empty(), "{family}/{name}");
            assert_eq!(report.attempts, 1, "{family}/{name}");
            assert_eq!(
                size_tuple(report.size.unwrap()),
                output_size(&expected),
                "{family}/{name} {format:?}: reported size"
            );
        }
    }
}

#[test]
fn unconstrained_fit_is_byte_identical_to_render_diagram() {
    let ansi = RenderConfig {
        text_color_mode: TextColorMode::Ansi,
        ..RenderConfig::default()
    };
    let show_ids = RenderConfig {
        show_ids: true,
        ..RenderConfig::default()
    };
    for family in ["flowchart", "class", "state", "sequence"] {
        for (name, input) in family_fixtures(family) {
            assert_identity(family, &name, &input, &RenderConfig::default());
            assert_identity(family, &name, &input, &ansi);
            if family != "sequence" {
                assert_identity(family, &name, &input, &show_ids);
            }
        }
    }
}

#[test]
fn ci_pipeline_fits_80_columns() {
    let result = fitted(
        &fixture("flowchart", "ci_pipeline"),
        &FitOptions::max_width(80),
    );
    let report = &result.report;
    assert_eq!(report.outcome, FitOutcome::Fitted);
    assert!(report.size.unwrap().width <= 80);
    assert!(!report.applied.is_empty());
    assert_eq!(report.lever_scope, FitLeverScope::Full);
    assert_eq!(
        output_size(&result.output),
        size_tuple(report.size.unwrap())
    );
}

#[test]
fn ci_pipeline_at_40_is_never_wider_than_the_narrowest_valid_candidate() {
    let result = fitted(
        &fixture("flowchart", "ci_pipeline"),
        &FitOptions::max_width(40),
    );
    let report = &result.report;
    assert!(matches!(
        report.outcome,
        FitOutcome::Fitted | FitOutcome::BestAttempt
    ));
    let narrowest = report
        .trail
        .iter()
        .filter(|attempt| attempt.valid)
        .map(|attempt| attempt.size.width)
        .min()
        .unwrap();
    assert!(report.size.unwrap().width <= narrowest.max(40));
    assert_eq!(report.attempts as usize, report.trail.len());
}

#[test]
fn keep_never_changes_direction() {
    let fit = FitOptions::max_width(40).with_direction_change(DirectionChange::Keep);
    for name in ["ci_pipeline", "git_workflow", "multi_subgraph"] {
        let report = fitted(&fixture("flowchart", name), &fit).report;
        assert!(
            report
                .trail
                .iter()
                .all(|attempt| !has_direction_lever(&attempt.levers)),
            "{name}: a Keep fit tried a direction change"
        );
        assert_eq!(report.direction, report.authored_direction, "{name}");
    }
}

#[test]
fn the_answer_is_never_an_invalid_candidate() {
    let input = fixture("flowchart", "subgraph_direction_nested_mixed");
    let report = fitted(&input, &FitOptions::max_width(30)).report;
    let size = report.size.unwrap();
    let chosen_levers = &report.applied;
    let chosen = report
        .trail
        .iter()
        .find(|attempt| &attempt.levers == chosen_levers && attempt.size == size)
        .expect("the answer is in the trail");
    assert!(chosen.valid);
}

#[test]
fn stable_for_bounds_select_the_same_output() {
    let input = fixture("flowchart", "ci_pipeline");
    for budget in [120, 80, 60, 40] {
        let result = fitted(&input, &FitOptions::max_width(budget));
        let stable = result
            .report
            .stable_for
            .expect("width-only fit has a range");
        if result.report.outcome == FitOutcome::BestAttempt {
            assert_eq!(stable.min_width, 1, "budget {budget}");
        }
        let at_min = fitted(&input, &FitOptions::max_width(stable.min_width));
        assert_eq!(at_min.output, result.output, "budget {budget} at min");
        if let Some(max) = stable.max_width {
            let at_max = fitted(&input, &FitOptions::max_width(max));
            assert_eq!(at_max.output, result.output, "budget {budget} at max");
        }
        if stable.min_width > 1 {
            let below = fitted(&input, &FitOptions::max_width(stable.min_width - 1));
            assert!(
                below.output != result.output || below.report.outcome == FitOutcome::BestAttempt,
                "budget {budget} below min"
            );
        }
    }
}

#[test]
fn a_height_budget_drops_stable_for() {
    let input = fixture("flowchart", "ci_pipeline");
    let report = fitted(&input, &FitOptions::max_width(80).with_max_height(5)).report;
    assert_eq!(report.stable_for, None);
    assert_eq!(report.budget.max_height, Some(5));
    let best_width_overflow = report
        .trail
        .iter()
        .filter(|attempt| attempt.valid)
        .map(|attempt| attempt.size.width.saturating_sub(80))
        .min()
        .unwrap();
    assert_eq!(
        report.size.unwrap().width.saturating_sub(80),
        best_width_overflow
    );
}

#[test]
fn sequence_diagrams_have_no_levers() {
    let input = fixture("sequence", "activation_nested");
    let full = render_diagram(&input, OutputFormat::Text, &RenderConfig::default()).unwrap();
    let report = fitted(&input, &FitOptions::max_width(100_000)).report;
    assert_eq!(report.lever_scope, FitLeverScope::Unavailable);
    assert!(report.applied.is_empty());
    assert_eq!(report.authored_direction, None);
    assert_eq!(report.direction, None);

    let narrow = fitted(&input, &FitOptions::max_width(5));
    assert_eq!(narrow.report.outcome, FitOutcome::BestAttempt);
    assert_eq!(narrow.output, full);
}

#[test]
fn non_text_formats_are_not_applied() {
    let input = fixture("flowchart", "ci_pipeline");
    let config = RenderConfig::default();
    for format in [OutputFormat::Svg, OutputFormat::Mmds] {
        let result =
            render_diagram_fitted(&input, format, &config, &FitOptions::max_width(40)).unwrap();
        assert_eq!(result.report.outcome, FitOutcome::NotApplied);
        assert_eq!(result.report.size, None);
        assert_eq!(
            result.output,
            render_diagram(&input, format, &config).unwrap()
        );
    }
}

#[test]
fn mmds_text_input_uses_render_only_levers() {
    let json = render_diagram(
        &fixture("flowchart", "ci_pipeline"),
        OutputFormat::Mmds,
        &RenderConfig::default(),
    )
    .unwrap();
    let unconstrained = fitted(&json, &FitOptions::default());
    assert_eq!(
        unconstrained.output,
        render_diagram(&json, OutputFormat::Text, &RenderConfig::default()).unwrap()
    );
    assert_eq!(unconstrained.report.outcome, FitOutcome::AsAuthored);

    let report = fitted(&json, &FitOptions::max_width(80)).report;
    assert_eq!(report.lever_scope, FitLeverScope::RenderOnly);
    assert_eq!(report.solves, 0);
    assert!(
        report
            .trail
            .iter()
            .all(|attempt| !has_direction_lever(&attempt.levers))
    );
}

#[test]
fn errors_match_render_diagram() {
    let config = RenderConfig::default();
    let input = fixture("flowchart", "ci_pipeline");
    let error = render_diagram_fitted(
        &input,
        OutputFormat::Text,
        &config,
        &FitOptions::max_width(0),
    )
    .unwrap_err();
    assert!(error.message.contains("max width must be at least 1"));
    let error = render_diagram_fitted(
        &input,
        OutputFormat::Text,
        &config,
        &FitOptions::default().with_max_height(0),
    )
    .unwrap_err();
    assert!(error.message.contains("max height must be at least 1"));

    let bad = "graph TD\nA --> ";
    let expected = render_diagram(bad, OutputFormat::Text, &config).unwrap_err();
    let actual =
        render_diagram_fitted(bad, OutputFormat::Text, &config, &FitOptions::max_width(40))
            .unwrap_err();
    assert_eq!(actual.message, expected.message);
}

#[test]
fn fitting_is_deterministic() {
    let input = fixture("flowchart", "ci_pipeline");
    let fit = FitOptions::max_width(40).with_truncation(true);
    assert_eq!(fitted(&input, &fit), fitted(&input, &fit));
}

#[test]
fn report_json_matches_the_contract_shape() {
    let report = fitted(
        &fixture("flowchart", "ci_pipeline"),
        &FitOptions::max_width(80),
    )
    .report;
    let json = serde_json::to_value(&report).unwrap();
    let keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    for key in [
        "budget",
        "outcome",
        "size",
        "asAuthored",
        "applied",
        "authoredDirection",
        "direction",
        "stableFor",
        "leverScope",
        "attempts",
        "solves",
    ] {
        assert!(keys.contains(&key), "missing {key}");
    }
    assert!(!keys.contains(&"trail"));
    assert_eq!(json["outcome"], "fitted");
    assert_eq!(json["authoredDirection"], "LR");
}

// ---------------------------------------------------------------------------
// Documents
// ---------------------------------------------------------------------------

fn materialize(input: &str, level: GeometryLevel) -> Document {
    let config = RenderConfig {
        geometry_level: level,
        ..RenderConfig::default()
    };
    materialize_diagram(input, &config).expect("diagram should materialize")
}

#[test]
fn unconstrained_replay_is_byte_identical_to_render_document() {
    let config = RenderConfig::default();
    for family in ["flowchart", "class", "state"] {
        for (name, input) in family_fixtures(family) {
            for level in [GeometryLevel::Layout, GeometryLevel::Routed] {
                let document = materialize(&input, level);
                let expected = render_document(&document, OutputFormat::Text, &config);
                for fit in [FitOptions::default(), FitOptions::max_width(100_000)] {
                    let result =
                        render_document_fitted(&document, OutputFormat::Text, &config, &fit);
                    let (expected, result) = match (&expected, result) {
                        (Ok(expected), Ok(result)) => (expected, result),
                        (Err(expected), Err(actual)) => {
                            assert_eq!(actual.message, expected.message, "{family}/{name}");
                            continue;
                        }
                        (expected, actual) => {
                            panic!("{family}/{name}: {expected:?} vs {actual:?}")
                        }
                    };
                    assert_eq!(&result.output, expected, "{family}/{name} {level:?}");
                    assert_eq!(result.report.outcome, FitOutcome::AsAuthored);
                    assert_eq!(result.report.lever_scope, FitLeverScope::RenderOnly);
                }
            }
        }
    }
}

#[test]
fn replay_applies_only_render_time_levers() {
    let document = materialize(&fixture("flowchart", "ci_pipeline"), GeometryLevel::Layout);
    let result = render_document_fitted(
        &document,
        OutputFormat::Text,
        &RenderConfig::default(),
        &FitOptions::max_width(120),
    )
    .unwrap();
    assert!(result.report.applied.iter().all(|lever| matches!(
        lever,
        FitLever::LabelAwareSpacing | FitLever::GridGap { .. }
    )));
    assert_eq!(result.report.solves, 0);
}

#[test]
fn unconstrained_relayout_is_byte_identical_to_render_document_with_relayout() {
    let config = RenderConfig::default();
    for name in ["ci_pipeline", "git_workflow", "multi_subgraph"] {
        let document = materialize(&fixture("flowchart", name), GeometryLevel::Layout);
        let expected =
            render_document_with_relayout(&document, OutputFormat::Text, &config).unwrap();
        let result = render_document_with_relayout_fitted(
            &document,
            OutputFormat::Text,
            &config,
            &FitOptions::default(),
        )
        .unwrap();
        assert_eq!(result.output, expected, "{name}");
        assert_eq!(result.report.outcome, FitOutcome::AsAuthored);
        assert_eq!(result.report.lever_scope, FitLeverScope::Full);
    }
}

#[test]
fn relayout_of_a_union_document_may_change_direction() {
    let before = fixture("flowchart", "ci_pipeline");
    let after = before.replacen("-->", "-->|ok|", 1);
    let union = union_document(
        &materialize(&before, GeometryLevel::Layout),
        &materialize(&after, GeometryLevel::Layout),
        &UnionOptions::default(),
    );
    let result = render_document_with_relayout_fitted(
        &union,
        OutputFormat::Text,
        &RenderConfig::default(),
        &FitOptions::max_width(80),
    )
    .unwrap();
    assert_eq!(result.report.lever_scope, FitLeverScope::Full);
    assert!(
        result
            .report
            .trail
            .iter()
            .any(|attempt| has_direction_lever(&attempt.levers))
            || result.report.outcome != FitOutcome::BestAttempt,
        "a union that does not fit must have tried the flip"
    );
}

#[test]
fn document_entry_points_do_not_fit_other_formats() {
    let config = RenderConfig::default();
    let document = materialize(&fixture("flowchart", "ci_pipeline"), GeometryLevel::Layout);
    let fit = FitOptions::max_width(40);
    let replay = render_document_fitted(&document, OutputFormat::Svg, &config, &fit).unwrap();
    assert_eq!(replay.report.outcome, FitOutcome::NotApplied);
    assert_eq!(
        replay.output,
        render_document(&document, OutputFormat::Svg, &config).unwrap()
    );
    let relayout =
        render_document_with_relayout_fitted(&document, OutputFormat::Svg, &config, &fit).unwrap();
    assert_eq!(relayout.report.outcome, FitOutcome::NotApplied);
    assert_eq!(
        relayout.output,
        render_document_with_relayout(&document, OutputFormat::Svg, &config).unwrap()
    );
}
