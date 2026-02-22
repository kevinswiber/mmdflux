use std::{env, fs};

use mmdflux::layered::LayoutConfig;
use mmdflux::render::node_dimensions;
use mmdflux::{Direction, build_diagram, parse_flowchart};

fn json_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

fn main() {
    let path = match env::args().nth(1) {
        Some(p) => p,
        None => {
            eprintln!("Usage: dump_dagre_input <fixture-path>");
            std::process::exit(1);
        }
    };
    let input = fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("Failed to read {}: {}", path, e);
        std::process::exit(1);
    });

    let flowchart = parse_flowchart(&input).unwrap_or_else(|e| {
        eprintln!("Failed to parse {}: {}", path, e);
        std::process::exit(1);
    });
    let diagram = build_diagram(&flowchart);

    let rankdir = match diagram.direction {
        Direction::TopDown => "TB",
        Direction::BottomTop => "BT",
        Direction::LeftRight => "LR",
        Direction::RightLeft => "RL",
    };

    // Match dagre defaults (Mermaid flowchart defaults).
    let config = LayoutConfig::default();
    let render_config = mmdflux::render::LayoutConfig::default();
    let node_sep = config.node_sep;
    let edge_sep = config.edge_sep;
    let mut ranksep = config.rank_sep;
    // Apply cluster rank_sep offset when subgraphs are present, matching
    // the engine pipeline which adds cluster_rank_sep for compound graphs.
    if diagram.has_subgraphs() && render_config.cluster_rank_sep > 0.0 {
        ranksep += render_config.cluster_rank_sep;
    }
    let margin = config.margin;

    struct NodeEntry {
        id: String,
        label: String,
        width: f64,
        height: f64,
        parent: Option<String>,
        is_subgraph: bool,
    }

    let mut nodes: Vec<NodeEntry> = Vec::new();
    for (id, node) in &diagram.nodes {
        let (w, h) = node_dimensions(node, diagram.direction);
        nodes.push(NodeEntry {
            id: id.clone(),
            label: node.label.clone(),
            width: w as f64,
            height: h as f64,
            parent: node.parent.clone(),
            is_subgraph: false,
        });
    }
    for (id, sg) in &diagram.subgraphs {
        nodes.push(NodeEntry {
            id: id.clone(),
            label: sg.title.clone(),
            width: 0.0,
            height: 0.0,
            parent: sg.parent.clone(),
            is_subgraph: true,
        });
    }
    nodes.sort_by(|a, b| a.id.cmp(&b.id));

    struct EdgeEntry {
        from: String,
        to: String,
        label: Option<String>,
        index: usize,
    }

    let edges: Vec<EdgeEntry> = diagram
        .edges
        .iter()
        .enumerate()
        .map(|(idx, edge)| EdgeEntry {
            from: edge.from.clone(),
            to: edge.to.clone(),
            label: edge.label.clone(),
            index: idx,
        })
        .collect();

    println!("{{");
    println!("  \"graph\": {{");
    println!("    \"rankdir\": \"{}\",", rankdir);
    println!("    \"nodesep\": {},", node_sep);
    println!("    \"edgesep\": {},", edge_sep);
    println!("    \"ranksep\": {},", ranksep);
    println!("    \"marginx\": {},", margin);
    println!("    \"marginy\": {},", margin);
    println!("    \"ranker\": \"network-simplex\"");
    println!("  }},");

    println!("  \"nodes\": [");
    for (i, node) in nodes.iter().enumerate() {
        let parent_json = match &node.parent {
            Some(p) => format!("\"{}\"", json_escape(p)),
            None => "null".to_string(),
        };
        let comma = if i + 1 == nodes.len() { "" } else { "," };
        println!(
            "    {{\"id\": \"{}\", \"label\": \"{}\", \"width\": {}, \"height\": {}, \"parent\": {}, \"is_subgraph\": {}}}{}",
            json_escape(&node.id),
            json_escape(&node.label),
            node.width,
            node.height,
            parent_json,
            node.is_subgraph,
            comma
        );
    }
    println!("  ],");

    println!("  \"edges\": [");
    for (i, edge) in edges.iter().enumerate() {
        let (label_json, label_w, label_h) = match &edge.label {
            Some(l) => (
                format!("\"{}\"", json_escape(l)),
                l.chars().count() + 2,
                1usize,
            ),
            None => ("null".to_string(), 0usize, 0usize),
        };
        let comma = if i + 1 == edges.len() { "" } else { "," };
        println!(
            "    {{\"from\": \"{}\", \"to\": \"{}\", \"label\": {}, \"label_width\": {}, \"label_height\": {}, \"index\": {}}}{}",
            json_escape(&edge.from),
            json_escape(&edge.to),
            label_json,
            label_w,
            label_h,
            edge.index,
            comma
        );
    }
    println!("  ]");
    println!("}}");
}
