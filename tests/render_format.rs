use mmdflux::OutputFormat;
use mmdflux::registry::DiagramInstance;

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
    let mut instance = mmdflux::diagrams::mmds::MmdsInstance::default();
    instance.parse(&input).unwrap();

    for format in [OutputFormat::Text, OutputFormat::Ascii] {
        let output = instance
            .render(format, &mmdflux::RenderConfig::default())
            .expect("routed MMDS should render text/ascii by ignoring paths");
        assert!(output.contains("Start"));
    }
}

fn mmds_fixture(name: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/mmds/{name}")).unwrap()
}

#[test]
fn mmds_capability_matrix_matches_geometry_level_contract() {
    let mut layout_instance = mmdflux::diagrams::mmds::MmdsInstance::default();
    layout_instance
        .parse(&mmds_fixture("positioned/layout-basic.json"))
        .unwrap();
    for format in [
        OutputFormat::Text,
        OutputFormat::Ascii,
        OutputFormat::Svg,
        OutputFormat::Mmds,
    ] {
        assert!(
            layout_instance
                .render(format, &mmdflux::RenderConfig::default())
                .is_ok(),
            "layout payload should support {format}"
        );
    }

    let mut routed_instance = mmdflux::diagrams::mmds::MmdsInstance::default();
    routed_instance
        .parse(&mmds_fixture("positioned/routed-basic.json"))
        .unwrap();
    for format in [
        OutputFormat::Text,
        OutputFormat::Ascii,
        OutputFormat::Svg,
        OutputFormat::Mmds,
    ] {
        assert!(
            routed_instance
                .render(format, &mmdflux::RenderConfig::default())
                .is_ok(),
            "routed payload should support {format}"
        );
    }
}
