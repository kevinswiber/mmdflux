//! Tests for the style model types: RoutingStyle, Curve, CornerStyle, EdgePreset.

use mmdflux::diagram::{CornerStyle, Curve, EdgePreset, RoutingStyle};

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
fn curve_parse_accepts_basis_and_linear_variants() {
    assert_eq!(Curve::parse("basis").unwrap(), Curve::Basis);
    assert_eq!(
        Curve::parse("linear").unwrap(),
        Curve::Linear(CornerStyle::Sharp)
    );
    assert_eq!(
        Curve::parse("linear-sharp").unwrap(),
        Curve::Linear(CornerStyle::Sharp)
    );
    assert_eq!(
        Curve::parse("linear-rounded").unwrap(),
        Curve::Linear(CornerStyle::Rounded)
    );
}

#[test]
fn curve_parse_rejects_legacy_aliases() {
    let err = Curve::parse("bezier").unwrap_err();
    assert!(err.message.contains("unknown curve"));
}

#[test]
fn curve_parse_rejects_catmull_rom_as_deferred() {
    let err = Curve::parse("catmull-rom").unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains("catmull"),
        "error should mention catmull-rom: {message}"
    );
}

#[test]
fn curve_parse_rejects_unknown() {
    let err = Curve::parse("bogus").unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains("basis") || message.contains("linear"),
        "error should list valid options: {message}"
    );
}

#[test]
fn curve_display_roundtrips() {
    assert_eq!(Curve::Basis.to_string(), "basis");
    assert_eq!(Curve::Linear(CornerStyle::Sharp).to_string(), "linear");
    assert_eq!(
        Curve::Linear(CornerStyle::Rounded).to_string(),
        "linear-rounded"
    );
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
        EdgePreset::parse("smooth-step").unwrap(),
        EdgePreset::SmoothStep
    );
    assert_eq!(
        EdgePreset::parse("curved-step").unwrap(),
        EdgePreset::CurvedStep
    );
    assert_eq!(EdgePreset::parse("basis").unwrap(), EdgePreset::Basis);
}

#[test]
fn edge_preset_parse_is_case_insensitive() {
    assert_eq!(EdgePreset::parse("Straight").unwrap(), EdgePreset::Straight);
    assert_eq!(EdgePreset::parse("POLYLINE").unwrap(), EdgePreset::Polyline);
    assert_eq!(EdgePreset::parse("BASIS").unwrap(), EdgePreset::Basis);
    assert_eq!(
        EdgePreset::parse("Smooth-Step").unwrap(),
        EdgePreset::SmoothStep
    );
    assert_eq!(
        EdgePreset::parse("Curved-Step").unwrap(),
        EdgePreset::CurvedStep
    );
}

#[test]
fn edge_preset_parse_accepts_legacy_smoothstep_aliases() {
    assert_eq!(
        EdgePreset::parse("smoothstep").unwrap(),
        EdgePreset::SmoothStep
    );
    assert_eq!(
        EdgePreset::parse("smooth-step").unwrap(),
        EdgePreset::SmoothStep
    );
}

#[test]
fn edge_preset_parse_rejects_legacy_bezier_alias() {
    let err = EdgePreset::parse("bezier").unwrap_err();
    assert!(err.message.contains("unknown edge preset"));
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
    assert_eq!(EdgePreset::SmoothStep.to_string(), "smooth-step");
    assert_eq!(EdgePreset::CurvedStep.to_string(), "curved-step");
    assert_eq!(EdgePreset::Basis.to_string(), "basis");
}

#[test]
fn edge_preset_expand_straight() {
    let (routing, curve) = EdgePreset::Straight.expand();
    assert_eq!(routing, RoutingStyle::Direct);
    assert_eq!(curve, Curve::Linear(CornerStyle::Sharp));
}

#[test]
fn edge_preset_expand_polyline() {
    let (routing, curve) = EdgePreset::Polyline.expand();
    assert_eq!(routing, RoutingStyle::Polyline);
    assert_eq!(curve, Curve::Linear(CornerStyle::Sharp));
}

#[test]
fn edge_preset_expand_step() {
    let (routing, curve) = EdgePreset::Step.expand();
    assert_eq!(routing, RoutingStyle::Orthogonal);
    assert_eq!(curve, Curve::Linear(CornerStyle::Sharp));
}

#[test]
fn edge_preset_expand_smooth_step() {
    let (routing, curve) = EdgePreset::SmoothStep.expand();
    assert_eq!(routing, RoutingStyle::Orthogonal);
    assert_eq!(curve, Curve::Linear(CornerStyle::Rounded));
}

#[test]
fn edge_preset_expand_curved_step() {
    let (routing, curve) = EdgePreset::CurvedStep.expand();
    assert_eq!(routing, RoutingStyle::Orthogonal);
    assert_eq!(curve, Curve::Basis);
}

#[test]
fn edge_preset_expand_basis() {
    let (routing, curve) = EdgePreset::Basis.expand();
    assert_eq!(routing, RoutingStyle::Polyline);
    assert_eq!(curve, Curve::Basis);
}

#[test]
fn edge_preset_expand_returns_routing_plus_curve() {
    let (routing, curve) = EdgePreset::SmoothStep.expand();
    assert_eq!(routing, RoutingStyle::Orthogonal);
    assert_eq!(curve, Curve::Linear(CornerStyle::Rounded));
}
