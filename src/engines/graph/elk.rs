//! ELK subprocess layout engine adapter.
//!
//! Converts `Diagram` to ELK JSON, invokes an external ELK process,
//! and parses the output back to `GraphGeometry`. Feature-gated behind
//! `engine-elk`.

use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::diagram::{EngineConfig, RenderError};
use crate::diagrams::flowchart::geometry::{
    FPoint, FRect, GraphGeometry, LayoutEdge, PositionedNode, SubgraphGeometry,
};
use crate::graph::{Diagram, Direction, Shape};

/// ELK layout engine adapter.
///
/// Delegates layout computation to an external ELK process (elkjs via Node.js).
/// The subprocess must be available as `mmdflux-elk` on `PATH`, or configured
/// via the `MMDFLUX_ELK_CMD` environment variable.
pub struct ElkLayoutEngine;

impl ElkLayoutEngine {
    /// Resolve the ELK subprocess command.
    fn elk_command() -> String {
        std::env::var("MMDFLUX_ELK_CMD").unwrap_or_else(|_| "mmdflux-elk".to_string())
    }
}

impl ElkLayoutEngine {
    pub fn layout(
        &self,
        diagram: &Diagram,
        _config: &EngineConfig,
    ) -> Result<GraphGeometry, RenderError> {
        let elk_input = diagram_to_elk_json(diagram);
        let elk_output = invoke_elk_subprocess(&elk_input)?;
        parse_elk_output(&elk_output, diagram)
    }
}

// ---------------------------------------------------------------------------
// Diagram → ELK JSON conversion
// ---------------------------------------------------------------------------

fn diagram_to_elk_json(diagram: &Diagram) -> String {
    let direction = match diagram.direction {
        Direction::TopDown => "DOWN",
        Direction::BottomTop => "UP",
        Direction::LeftRight => "RIGHT",
        Direction::RightLeft => "LEFT",
    };

    let mut children = Vec::new();
    for (id, node) in &diagram.nodes {
        let (w, h) = elk_node_dimensions(node);
        children.push(format!(
            r#"    {{ "id": {id_json}, "width": {w}, "height": {h}, "labels": [{{ "text": {label_json} }}] }}"#,
            id_json = json_string(id),
            label_json = json_string(&node.label),
        ));
    }

    let mut edges = Vec::new();
    for (i, edge) in diagram.edges.iter().enumerate() {
        let mut edge_json = format!(
            r#"    {{ "id": "e{i}", "sources": [{src}], "targets": [{tgt}]"#,
            src = json_string(&edge.from),
            tgt = json_string(&edge.to),
        );
        if let Some(label) = &edge.label {
            edge_json.push_str(&format!(
                r#", "labels": [{{ "text": {label_json}, "width": {w}, "height": 14 }}]"#,
                label_json = json_string(label),
                w = label.len() * 8 + 16,
            ));
        }
        edge_json.push_str(" }");
        edges.push(edge_json);
    }

    format!(
        r#"{{
  "id": "root",
  "properties": {{
    "elk.algorithm": "layered",
    "elk.direction": "{direction}"
  }},
  "children": [
{children}
  ],
  "edges": [
{edges}
  ]
}}"#,
        children = children.join(",\n"),
        edges = edges.join(",\n"),
    )
}

fn elk_node_dimensions(node: &crate::graph::Node) -> (f64, f64) {
    let label_width = node.label.len() as f64 * 8.0 + 16.0;
    let base_w = label_width.max(40.0);
    let base_h = 30.0;
    match node.shape {
        Shape::Diamond => (base_w * 1.5, base_h * 1.5),
        _ => (base_w, base_h),
    }
}

fn json_string(s: &str) -> String {
    format!(
        "\"{}\"",
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    )
}

// ---------------------------------------------------------------------------
// Subprocess invocation
// ---------------------------------------------------------------------------

fn invoke_elk_subprocess(input: &str) -> Result<String, RenderError> {
    let cmd = ElkLayoutEngine::elk_command();

    let mut child = Command::new(&cmd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                RenderError {
                    message: format!(
                        "ELK subprocess not found: {cmd:?}. \
                         Install elkjs and ensure '{cmd}' is on PATH, \
                         or set MMDFLUX_ELK_CMD to the path of your ELK layout script."
                    ),
                }
            } else {
                RenderError {
                    message: format!("failed to start ELK subprocess: {e}"),
                }
            }
        })?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(input.as_bytes()).map_err(|e| RenderError {
            message: format!("failed to write to ELK subprocess stdin: {e}"),
        })?;
    }

    let output = child.wait_with_output().map_err(|e| RenderError {
        message: format!("ELK subprocess failed: {e}"),
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RenderError {
            message: format!(
                "ELK subprocess exited with {}: {}",
                output.status,
                stderr.trim()
            ),
        });
    }

    String::from_utf8(output.stdout).map_err(|e| RenderError {
        message: format!("ELK subprocess produced invalid UTF-8: {e}"),
    })
}

// ---------------------------------------------------------------------------
// ELK JSON output → GraphGeometry conversion
// ---------------------------------------------------------------------------

fn parse_elk_output(output: &str, diagram: &Diagram) -> Result<GraphGeometry, RenderError> {
    let root: serde_json::Value = serde_json::from_str(output).map_err(|e| RenderError {
        message: format!("failed to parse ELK output as JSON: {e}"),
    })?;

    let mut nodes = HashMap::new();
    let mut subgraphs = HashMap::new();
    let mut max_x: f64 = 0.0;
    let mut max_y: f64 = 0.0;

    // Parse children (nodes)
    if let Some(children) = root["children"].as_array() {
        for child in children {
            let id = child["id"]
                .as_str()
                .ok_or_else(|| RenderError::from("ELK node missing 'id'"))?;

            let x = child["x"].as_f64().unwrap_or(0.0);
            let y = child["y"].as_f64().unwrap_or(0.0);
            let w = child["width"].as_f64().unwrap_or(40.0);
            let h = child["height"].as_f64().unwrap_or(30.0);

            max_x = max_x.max(x + w);
            max_y = max_y.max(y + h);

            // Look up shape and label from original diagram
            let shape = diagram
                .nodes
                .get(id)
                .map(|n| n.shape)
                .unwrap_or(Shape::Rectangle);

            let label = diagram
                .nodes
                .get(id)
                .map(|n| n.label.clone())
                .unwrap_or_else(|| id.to_string());

            let parent = diagram.nodes.get(id).and_then(|n| n.parent.clone());

            // ELK uses top-left coordinates; convert to center for consistency with flux-layered layout
            nodes.insert(
                id.to_string(),
                PositionedNode {
                    id: id.to_string(),
                    rect: FRect::new(x + w / 2.0, y + h / 2.0, w, h),
                    shape,
                    label,
                    parent,
                },
            );

            // If this child has nested children, treat as subgraph
            if child["children"].is_array() && !child["children"].as_array().unwrap().is_empty() {
                subgraphs.insert(
                    id.to_string(),
                    SubgraphGeometry {
                        id: id.to_string(),
                        rect: FRect::new(x + w / 2.0, y + h / 2.0, w, h),
                        title: diagram
                            .subgraphs
                            .get(id)
                            .map(|sg| sg.title.clone())
                            .unwrap_or_default(),
                        depth: diagram
                            .subgraphs
                            .get(id)
                            .map(|_| diagram.subgraph_depth(id))
                            .unwrap_or(0),
                    },
                );
            }
        }
    }

    // Parse edges
    let mut edges = Vec::new();
    if let Some(elk_edges) = root["edges"].as_array() {
        for edge in elk_edges {
            let idx = parse_elk_edge_index(edge)?;
            let from = edge["sources"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let to = edge["targets"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // Extract edge waypoints from ELK sections
            let mut waypoints = Vec::new();
            let mut path_hint = Vec::new();
            if let Some(sections) = edge["sections"].as_array() {
                for section in sections {
                    if let Some(start) = section.get("startPoint")
                        && let (Some(x), Some(y)) = (start["x"].as_f64(), start["y"].as_f64())
                    {
                        path_hint.push(FPoint::new(x, y));
                    }
                    if let Some(bends) = section["bendPoints"].as_array() {
                        for bend in bends {
                            if let (Some(x), Some(y)) = (bend["x"].as_f64(), bend["y"].as_f64()) {
                                waypoints.push(FPoint::new(x, y));
                                path_hint.push(FPoint::new(x, y));
                            }
                        }
                    }
                    if let Some(end) = section.get("endPoint")
                        && let (Some(x), Some(y)) = (end["x"].as_f64(), end["y"].as_f64())
                    {
                        path_hint.push(FPoint::new(x, y));
                    }
                }
            }

            // Find label position from ELK edge labels
            let label_position = edge["labels"]
                .as_array()
                .and_then(|labels| labels.first())
                .and_then(|l| {
                    let lx = l["x"].as_f64()?;
                    let ly = l["y"].as_f64()?;
                    let lw = l["width"].as_f64().unwrap_or(0.0);
                    let lh = l["height"].as_f64().unwrap_or(0.0);
                    Some(FPoint::new(lx + lw / 2.0, ly + lh / 2.0))
                });

            let from_subgraph = if diagram.is_subgraph(&from) {
                Some(from.clone())
            } else {
                None
            };
            let to_subgraph = if diagram.is_subgraph(&to) {
                Some(to.clone())
            } else {
                None
            };

            edges.push(LayoutEdge {
                index: idx,
                from,
                to,
                waypoints,
                label_position,
                from_subgraph,
                to_subgraph,
                layout_path_hint: if path_hint.is_empty() {
                    None
                } else {
                    Some(path_hint)
                },
            });
        }
    }

    // Build per-node directions (all nodes use root direction for ELK)
    let node_directions: HashMap<String, Direction> = nodes
        .keys()
        .map(|id| (id.clone(), diagram.direction))
        .collect();

    Ok(GraphGeometry {
        nodes,
        edges,
        subgraphs,
        self_edges: Vec::new(),
        direction: diagram.direction,
        node_directions,
        bounds: FRect::new(max_x / 2.0, max_y / 2.0, max_x, max_y),
        reversed_edges: Vec::new(),
        engine_hints: None,
        rerouted_edges: std::collections::HashSet::new(),
    })
}

fn parse_elk_edge_index(edge: &serde_json::Value) -> Result<usize, RenderError> {
    let id = edge["id"]
        .as_str()
        .ok_or_else(|| RenderError::from("ELK edge missing 'id'"))?;
    let numeric = id
        .strip_prefix('e')
        .ok_or_else(|| RenderError::from(format!("ELK edge id must start with 'e': {id:?}")))?;
    numeric
        .parse::<usize>()
        .map_err(|_| RenderError::from(format!("ELK edge id has invalid index: {id:?}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elk_command_defaults_to_mmdflux_elk() {
        // SAFETY: test runs single-threaded; no other thread reads this env var
        unsafe {
            std::env::remove_var("MMDFLUX_ELK_CMD");
        }
        assert_eq!(ElkLayoutEngine::elk_command(), "mmdflux-elk");
    }

    #[test]
    fn elk_command_respects_env_override() {
        // SAFETY: test runs single-threaded; no other thread reads this env var
        unsafe {
            std::env::set_var("MMDFLUX_ELK_CMD", "elk-custom");
        }
        assert_eq!(ElkLayoutEngine::elk_command(), "elk-custom");
        unsafe {
            std::env::remove_var("MMDFLUX_ELK_CMD");
        }
    }

    #[test]
    fn diagram_to_elk_json_simple() {
        let input = "graph TD\nA-->B";
        let flowchart = crate::parser::parse_flowchart(input).unwrap();
        let diagram = crate::graph::build_diagram(&flowchart);

        let json = diagram_to_elk_json(&diagram);
        assert!(json.contains("\"elk.algorithm\": \"layered\""));
        assert!(json.contains("\"elk.direction\": \"DOWN\""));
        assert!(json.contains("\"A\""));
        assert!(json.contains("\"B\""));
    }

    #[test]
    fn diagram_to_elk_json_lr_direction() {
        let input = "graph LR\nA-->B";
        let flowchart = crate::parser::parse_flowchart(input).unwrap();
        let diagram = crate::graph::build_diagram(&flowchart);

        let json = diagram_to_elk_json(&diagram);
        assert!(json.contains("\"elk.direction\": \"RIGHT\""));
    }

    #[test]
    fn parse_elk_output_simple() {
        let input = "graph TD\nA-->B";
        let flowchart = crate::parser::parse_flowchart(input).unwrap();
        let diagram = crate::graph::build_diagram(&flowchart);

        let elk_output = r#"{
            "id": "root",
            "children": [
                { "id": "A", "x": 10, "y": 10, "width": 40, "height": 30 },
                { "id": "B", "x": 10, "y": 80, "width": 40, "height": 30 }
            ],
            "edges": [
                {
                    "id": "e0",
                    "sources": ["A"],
                    "targets": ["B"],
                    "sections": [{
                        "startPoint": { "x": 30, "y": 40 },
                        "endPoint": { "x": 30, "y": 80 },
                        "bendPoints": []
                    }]
                }
            ]
        }"#;

        let geom = parse_elk_output(elk_output, &diagram).unwrap();
        assert_eq!(geom.nodes.len(), 2);
        assert!(geom.nodes.contains_key("A"));
        assert!(geom.nodes.contains_key("B"));
        assert_eq!(geom.edges.len(), 1);
        // path_hint has startPoint + endPoint
        let path = geom.edges[0].layout_path_hint.as_ref().unwrap();
        assert_eq!(path.len(), 2);
    }

    #[test]
    fn parse_elk_output_center_coordinates() {
        let input = "graph TD\nA-->B";
        let flowchart = crate::parser::parse_flowchart(input).unwrap();
        let diagram = crate::graph::build_diagram(&flowchart);

        let elk_output = r#"{
            "id": "root",
            "children": [
                { "id": "A", "x": 0, "y": 0, "width": 40, "height": 30 }
            ],
            "edges": []
        }"#;

        let geom = parse_elk_output(elk_output, &diagram).unwrap();
        let a = &geom.nodes["A"];
        // ELK top-left (0,0) with size (40,30) → center (20,15)
        assert_eq!(a.rect.x, 20.0);
        assert_eq!(a.rect.y, 15.0);
    }

    #[test]
    fn parse_elk_output_uses_edge_ids_for_indices() {
        let input = "graph TD\nA-->B\nB-->C";
        let flowchart = crate::parser::parse_flowchart(input).unwrap();
        let diagram = crate::graph::build_diagram(&flowchart);

        let elk_output = r#"{
            "id": "root",
            "children": [
                { "id": "A", "x": 10, "y": 10, "width": 40, "height": 30 },
                { "id": "B", "x": 10, "y": 80, "width": 40, "height": 30 },
                { "id": "C", "x": 10, "y": 150, "width": 40, "height": 30 }
            ],
            "edges": [
                { "id": "e1", "sources": ["B"], "targets": ["C"], "sections": [] },
                { "id": "e0", "sources": ["A"], "targets": ["B"], "sections": [] }
            ]
        }"#;

        let geom = parse_elk_output(elk_output, &diagram).unwrap();
        assert_eq!(geom.edges.len(), 2);
        assert_eq!(geom.edges[0].index, 1);
        assert_eq!(geom.edges[0].from, "B");
        assert_eq!(geom.edges[0].to, "C");
        assert_eq!(geom.edges[1].index, 0);
        assert_eq!(geom.edges[1].from, "A");
        assert_eq!(geom.edges[1].to, "B");
    }

    #[test]
    fn elk_subprocess_not_found_gives_actionable_error() {
        // SAFETY: test runs single-threaded; no other thread reads this env var
        unsafe {
            std::env::set_var("MMDFLUX_ELK_CMD", "nonexistent-elk-binary-12345");
        }
        let result = invoke_elk_subprocess("{}");
        unsafe {
            std::env::remove_var("MMDFLUX_ELK_CMD");
        }

        let err = result.unwrap_err();
        assert!(
            err.message.contains("not found"),
            "error should mention not found: {}",
            err.message
        );
    }

    #[test]
    fn json_string_escapes_special_chars() {
        assert_eq!(json_string("hello"), "\"hello\"");
        assert_eq!(json_string("he\"llo"), "\"he\\\"llo\"");
        assert_eq!(json_string("he\\llo"), "\"he\\\\llo\"");
    }
}
