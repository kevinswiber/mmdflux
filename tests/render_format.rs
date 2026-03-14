use mmdflux::mmds::{render_input, supports_format};
use mmdflux::{EngineAlgorithmId, OutputFormat, RenderConfig};
#[test]
fn output_format_from_render_module() {
    let text = OutputFormat::Text;
    let ascii = OutputFormat::Ascii;
    let svg = OutputFormat::Svg;

    assert_eq!(text, OutputFormat::default());
    assert_ne!(ascii, svg);
}

#[test]
fn output_format_debug() {
    assert_eq!(format!("{:?}", OutputFormat::Text), "Text");
    assert_eq!(format!("{:?}", OutputFormat::Svg), "Svg");
}

#[test]
fn output_format_json_display() {
    assert_eq!(format!("{}", OutputFormat::Mmds), "mmds");
}

#[test]
fn mmds_routed_payload_renders_text_and_ascii_by_ignoring_paths() {
    let input =
        std::fs::read_to_string("tests/fixtures/mmds/positioned/routed-basic.json").unwrap();

    for format in [OutputFormat::Text, OutputFormat::Ascii] {
        let output = render_input(&input, format, &mmdflux::RenderConfig::default())
            .expect("routed MMDS should render text/ascii by ignoring paths");
        assert!(output.contains("Start"));
    }
}

#[test]
fn routed_mmds_text_render_does_not_reenter_runtime_engine_selection() {
    let input =
        std::fs::read_to_string("tests/fixtures/mmds/positioned/routed-basic.json").unwrap();
    let config = RenderConfig {
        layout_engine: Some(EngineAlgorithmId::ELK_LAYERED),
        ..RenderConfig::default()
    };

    let output = render_input(&input, OutputFormat::Text, &config)
        .expect("routed MMDS text replay should not depend on runtime engine availability");
    assert!(output.contains("Start"));
}

fn mmds_fixture(name: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/mmds/{name}")).unwrap()
}

#[test]
fn mmds_capability_matrix_matches_geometry_level_contract() {
    let layout_payload = mmds_fixture("positioned/layout-basic.json");
    for format in [
        OutputFormat::Text,
        OutputFormat::Ascii,
        OutputFormat::Svg,
        OutputFormat::Mmds,
    ] {
        assert!(
            supports_format(format),
            "frontend should advertise {format}"
        );
        assert!(
            render_input(&layout_payload, format, &mmdflux::RenderConfig::default()).is_ok(),
            "layout payload should support {format}"
        );
    }

    let routed_payload = mmds_fixture("positioned/routed-basic.json");
    for format in [
        OutputFormat::Text,
        OutputFormat::Ascii,
        OutputFormat::Svg,
        OutputFormat::Mmds,
    ] {
        assert!(
            supports_format(format),
            "frontend should advertise {format}"
        );
        assert!(
            render_input(&routed_payload, format, &mmdflux::RenderConfig::default()).is_ok(),
            "routed payload should support {format}"
        );
    }
}
