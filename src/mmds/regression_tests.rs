use std::fs;
use std::path::Path;

use super::from_mmds_str;

#[test]
fn hydration_applies_defaults_to_omitted_node_and_edge_fields() {
    let payload = mmds_fixture("defaults-minimal.json");
    let diagram = from_mmds_str(&payload).expect("valid hydration");

    assert_eq!(diagram.nodes["A"].shape, crate::Shape::Round);
    assert_eq!(diagram.edges[0].minlen, 2);
}

fn mmds_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mmds")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}
