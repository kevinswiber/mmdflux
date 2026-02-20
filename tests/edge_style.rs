//! Tests for the style model types: RoutingStyle, InterpolationStyle, CornerStyle, EdgePreset.

use mmdflux::diagram::{CornerStyle, EdgePreset, InterpolationStyle, RoutingStyle};

#[test]
fn routing_style_parse_accepts_canonical_values() {
    assert_eq!(RoutingStyle::parse("direct").unwrap(), RoutingStyle::Direct);
    assert_eq!(
        RoutingStyle::parse("polyline").unwrap(),
        RoutingStyle::Polyline
    );
    assert_eq!(
        RoutingStyle::parse("orthogonal").unwrap(),
        RoutingStyle::Orthogonal
    );
}

#[test]
fn routing_style_parse_is_case_insensitive() {
    assert_eq!(RoutingStyle::parse("DIRECT").unwrap(), RoutingStyle::Direct);
    assert_eq!(
        RoutingStyle::parse("Polyline").unwrap(),
        RoutingStyle::Polyline
    );
    assert_eq!(
        RoutingStyle::parse("ORTHOGONAL").unwrap(),
        RoutingStyle::Orthogonal
    );
}

#[test]
fn routing_style_parse_rejects_unknown() {
    let err = RoutingStyle::parse("bogus").unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains("direct")
            || message.contains("polyline")
            || message.contains("orthogonal"),
        "error should list valid options: {message}"
    );
}

#[test]
fn routing_style_display_roundtrips() {
    assert_eq!(RoutingStyle::Direct.to_string(), "direct");
    assert_eq!(RoutingStyle::Polyline.to_string(), "polyline");
    assert_eq!(RoutingStyle::Orthogonal.to_string(), "orthogonal");
}

#[test]
fn interpolation_style_parse_accepts_canonical_values() {
    assert_eq!(
        InterpolationStyle::parse("linear").unwrap(),
        InterpolationStyle::Linear
    );
    assert_eq!(
        InterpolationStyle::parse("bezier").unwrap(),
        InterpolationStyle::Bezier
    );
}

#[test]
fn interpolation_style_parse_rejects_catmull_rom_as_deferred() {
    let err = InterpolationStyle::parse("catmull-rom").unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains("catmull"),
        "error should mention catmull-rom: {message}"
    );
}

#[test]
fn interpolation_style_parse_rejects_unknown() {
    let err = InterpolationStyle::parse("bogus").unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains("linear") || message.contains("bezier"),
        "error should list valid options: {message}"
    );
}

#[test]
fn interpolation_style_display_roundtrips() {
    assert_eq!(InterpolationStyle::Linear.to_string(), "linear");
    assert_eq!(InterpolationStyle::Bezier.to_string(), "bezier");
}

#[test]
fn corner_style_parse_accepts_canonical_values() {
    assert_eq!(CornerStyle::parse("sharp").unwrap(), CornerStyle::Sharp);
    assert_eq!(CornerStyle::parse("rounded").unwrap(), CornerStyle::Rounded);
}

#[test]
fn corner_style_parse_rejects_unknown() {
    let err = CornerStyle::parse("bevel").unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains("sharp") || message.contains("rounded"),
        "error should list valid options: {message}"
    );
}

#[test]
fn corner_style_display_roundtrips() {
    assert_eq!(CornerStyle::Sharp.to_string(), "sharp");
    assert_eq!(CornerStyle::Rounded.to_string(), "rounded");
}

#[test]
fn edge_preset_parse_accepts_canonical_values() {
    assert_eq!(EdgePreset::parse("straight").unwrap(), EdgePreset::Straight);
    assert_eq!(EdgePreset::parse("polyline").unwrap(), EdgePreset::Polyline);
    assert_eq!(EdgePreset::parse("step").unwrap(), EdgePreset::Step);
    assert_eq!(
        EdgePreset::parse("smoothstep").unwrap(),
        EdgePreset::SmoothStep
    );
    assert_eq!(EdgePreset::parse("bezier").unwrap(), EdgePreset::Bezier);
}

#[test]
fn edge_preset_parse_is_case_insensitive() {
    assert_eq!(EdgePreset::parse("Straight").unwrap(), EdgePreset::Straight);
    assert_eq!(EdgePreset::parse("POLYLINE").unwrap(), EdgePreset::Polyline);
    assert_eq!(EdgePreset::parse("BEZIER").unwrap(), EdgePreset::Bezier);
    assert_eq!(
        EdgePreset::parse("SmoothStep").unwrap(),
        EdgePreset::SmoothStep
    );
}

#[test]
fn edge_preset_parse_rejects_direct_as_not_a_preset() {
    let err = EdgePreset::parse("direct").unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains("routing style") || message.contains("straight"),
        "error should mention the rejected value: {message}"
    );
}

#[test]
fn edge_preset_parse_rejects_unknown() {
    let err = EdgePreset::parse("bogus").unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains("straight") || message.contains("polyline"),
        "error should list valid presets: {message}"
    );
}

#[test]
fn edge_preset_display_roundtrips() {
    assert_eq!(EdgePreset::Straight.to_string(), "straight");
    assert_eq!(EdgePreset::Polyline.to_string(), "polyline");
    assert_eq!(EdgePreset::Step.to_string(), "step");
    assert_eq!(EdgePreset::SmoothStep.to_string(), "smoothstep");
    assert_eq!(EdgePreset::Bezier.to_string(), "bezier");
}

#[test]
fn edge_preset_expand_straight() {
    let (routing, interp, corner) = EdgePreset::Straight.expand();
    assert_eq!(routing, RoutingStyle::Direct);
    assert_eq!(interp, InterpolationStyle::Linear);
    assert_eq!(corner, CornerStyle::Sharp);
}

#[test]
fn edge_preset_expand_polyline() {
    let (routing, interp, corner) = EdgePreset::Polyline.expand();
    assert_eq!(routing, RoutingStyle::Polyline);
    assert_eq!(interp, InterpolationStyle::Linear);
    assert_eq!(corner, CornerStyle::Sharp);
}

#[test]
fn edge_preset_expand_step() {
    let (routing, interp, corner) = EdgePreset::Step.expand();
    assert_eq!(routing, RoutingStyle::Orthogonal);
    assert_eq!(interp, InterpolationStyle::Linear);
    assert_eq!(corner, CornerStyle::Sharp);
}

#[test]
fn edge_preset_expand_smoothstep() {
    let (routing, interp, corner) = EdgePreset::SmoothStep.expand();
    assert_eq!(routing, RoutingStyle::Orthogonal);
    assert_eq!(interp, InterpolationStyle::Linear);
    assert_eq!(corner, CornerStyle::Rounded);
}

#[test]
fn edge_preset_expand_bezier() {
    let (routing, interp, corner) = EdgePreset::Bezier.expand();
    assert_eq!(routing, RoutingStyle::Polyline);
    assert_eq!(interp, InterpolationStyle::Bezier);
    assert_eq!(corner, CornerStyle::Sharp);
}
