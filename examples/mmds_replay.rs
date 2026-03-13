use mmdflux::mmds::{generate_mermaid_from_mmds, parse_with_profiles, render_output};
use mmdflux::{OutputFormat, RenderConfig};

const MMDS_INPUT: &str = r#"{
  "version": 1,
  "profiles": ["mmds-core-v1", "mmdflux-text-v1"],
  "defaults": {
    "node": { "shape": "rectangle" },
    "edge": {
      "stroke": "solid",
      "arrow_start": "none",
      "arrow_end": "normal",
      "minlen": 1
    }
  },
  "geometry_level": "layout",
  "metadata": {
    "diagram_type": "flowchart",
    "direction": "TD",
    "bounds": { "width": 120.0, "height": 110.0 }
  },
  "nodes": [
    {
      "id": "A",
      "label": "Input",
      "position": { "x": 60.0, "y": 25.0 },
      "size": { "width": 50.0, "height": 20.0 }
    },
    {
      "id": "B",
      "label": "Output",
      "position": { "x": 60.0, "y": 85.0 },
      "size": { "width": 52.0, "height": 20.0 }
    }
  ],
  "edges": [
    {
      "id": "e0",
      "source": "A",
      "target": "B"
    }
  ]
}"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (payload, negotiation) = parse_with_profiles(MMDS_INPUT)?;
    let text = render_output(&payload, OutputFormat::Text, &RenderConfig::default())?;
    let mermaid = generate_mermaid_from_mmds(&payload)?;

    println!("supported profiles: {:?}", negotiation.supported);
    println!("{text}");
    println!("{mermaid}");
    Ok(())
}
