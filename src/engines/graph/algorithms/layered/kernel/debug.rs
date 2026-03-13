//! Debug and environment helpers for the layered engine.

use std::io::Write;

use super::graph::{self, LayoutGraph};
use super::types::DummyType;
use super::{EdgeLayout, LayoutResult, NodeId, Rect};

fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| value == "1")
}

pub(crate) fn skip_title_nodes() -> bool {
    env_flag("MMDFLUX_SKIP_TITLE_NODES")
}

pub(crate) fn border_nodes_enabled() -> bool {
    env_flag("MMDFLUX_DEBUG_BORDER_NODES")
}

pub(crate) fn subgraph_bounds_enabled() -> bool {
    env_flag("MMDFLUX_DEBUG_SUBGRAPH_BOUNDS")
}

pub(crate) fn dummy_parents_enabled() -> bool {
    env_flag("MMDFLUX_DEBUG_DUMMY_PARENTS")
}

pub(crate) fn order_enabled() -> bool {
    env_flag("MMDFLUX_DEBUG_ORDER")
}

pub(crate) fn conflicts_enabled() -> bool {
    env_flag("MMDFLUX_DEBUG_CONFLICTS")
}

pub(crate) fn bk_trace_enabled() -> bool {
    env_flag("MMDFLUX_DEBUG_BK_TRACE")
}

pub(crate) fn border_blocks_enabled() -> bool {
    env_flag("MMDFLUX_DEBUG_BORDER_BLOCKS")
}

fn debug_pipeline_target() -> Option<String> {
    std::env::var("MMDFLUX_DEBUG_PIPELINE").ok()
}

fn debug_layout_target() -> Option<String> {
    std::env::var("MMDFLUX_DEBUG_LAYOUT").ok()
}

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

fn fmt_f64_json(value: f64) -> String {
    if value.is_finite() {
        format!("{}", value)
    } else {
        "null".to_string()
    }
}

pub(crate) fn debug_dump_pipeline(lg: &LayoutGraph, stage: &str) {
    let Some(target) = debug_pipeline_target() else {
        return;
    };

    let mut entries: Vec<(i32, usize, usize)> = lg
        .ranks
        .iter()
        .enumerate()
        .map(|(idx, &rank)| (rank, lg.order[idx], idx))
        .collect();
    entries.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| lg.node_ids[a.2].0.cmp(&lg.node_ids[b.2].0))
    });

    let mut buf = String::new();
    for (rank, order, idx) in entries {
        let id = &lg.node_ids[idx].0;
        let parent = lg.parents[idx].map(|p| lg.node_ids[p].0.clone());
        let dummy = lg
            .dummy_nodes
            .get(&lg.node_ids[idx])
            .map(|d| match d.dummy_type {
                DummyType::Edge => "edge",
                DummyType::EdgeLabel => "edge_label",
            });
        let dummy_edge = lg.dummy_nodes.get(&lg.node_ids[idx]).map(|d| d.edge_index);
        let border = lg.border_type.get(&idx).map(|b| match b {
            graph::BorderType::Left => "left",
            graph::BorderType::Right => "right",
        });
        let is_position = lg.is_position_node(idx);
        let is_compound = lg.compound_nodes.contains(&idx);
        let is_excluded = lg.position_excluded_nodes.contains(&idx);

        let parent_json = match parent.as_deref() {
            Some(p) => format!("\"{}\"", json_escape(p)),
            None => "null".to_string(),
        };
        let dummy_json = match dummy {
            Some(d) => format!("\"{}\"", d),
            None => "null".to_string(),
        };
        let dummy_edge_json = match dummy_edge {
            Some(d) => d.to_string(),
            None => "null".to_string(),
        };
        let border_json = match border {
            Some(b) => format!("\"{}\"", b),
            None => "null".to_string(),
        };

        buf.push_str(&format!(
            "{{\"stage\":\"{}\",\"id\":\"{}\",\"rank\":{},\"order\":{},\"parent\":{},\"dummy\":{},\"dummy_edge\":{},\"border\":{},\"is_position\":{},\"is_compound\":{},\"is_excluded\":{}}}\n",
            json_escape(stage),
            json_escape(id),
            rank,
            order,
            parent_json,
            dummy_json,
            dummy_edge_json,
            border_json,
            is_position,
            is_compound,
            is_excluded
        ));
    }

    if target == "1" {
        eprint!("{buf}");
    } else if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&target)
    {
        let _ = file.write_all(buf.as_bytes());
    }
}

pub(crate) fn debug_dump_layout_result(result: &LayoutResult, original_edge_count: usize) {
    let Some(target) = debug_layout_target() else {
        return;
    };

    let mut nodes: Vec<(&NodeId, &Rect)> = result.nodes.iter().collect();
    nodes.sort_by(|a, b| a.0.0.cmp(&b.0.0));

    let mut edges: Vec<EdgeLayout> = result
        .edges
        .iter()
        .filter(|e| e.index < original_edge_count)
        .cloned()
        .collect();
    edges.sort_by_key(|e| e.index);

    let mut subgraphs: Vec<(&String, &Rect)> = result.subgraph_bounds.iter().collect();
    subgraphs.sort_by(|a, b| a.0.cmp(b.0));

    let mut buf = String::new();
    buf.push_str("{\"nodes\":[");
    for (i, (id, rect)) in nodes.iter().enumerate() {
        let center_x = rect.x + rect.width / 2.0;
        let center_y = rect.y + rect.height / 2.0;
        let suffix = if i + 1 == nodes.len() { "" } else { "," };
        buf.push_str(&format!(
            "{{\"id\":\"{}\",\"x\":{},\"y\":{},\"width\":{},\"height\":{},\"center_x\":{},\"center_y\":{}}}{}",
            json_escape(&id.0),
            fmt_f64_json(rect.x),
            fmt_f64_json(rect.y),
            fmt_f64_json(rect.width),
            fmt_f64_json(rect.height),
            fmt_f64_json(center_x),
            fmt_f64_json(center_y),
            suffix
        ));
    }
    buf.push_str("],\"edges\":[");
    for (i, edge) in edges.iter().enumerate() {
        let suffix = if i + 1 == edges.len() { "" } else { "," };
        buf.push_str(&format!(
            "{{\"index\":{},\"from\":\"{}\",\"to\":\"{}\",\"points\":[",
            edge.index,
            json_escape(&edge.from.0),
            json_escape(&edge.to.0)
        ));
        for (p_idx, point) in edge.points.iter().enumerate() {
            let p_suffix = if p_idx + 1 == edge.points.len() {
                ""
            } else {
                ","
            };
            buf.push_str(&format!(
                "[{},{}]{}",
                fmt_f64_json(point.x),
                fmt_f64_json(point.y),
                p_suffix
            ));
        }
        buf.push_str(&format!("]}}{}", suffix));
    }
    buf.push_str("],\"subgraph_bounds\":[");
    for (i, (id, rect)) in subgraphs.iter().enumerate() {
        let suffix = if i + 1 == subgraphs.len() { "" } else { "," };
        buf.push_str(&format!(
            "{{\"id\":\"{}\",\"x\":{},\"y\":{},\"width\":{},\"height\":{}}}{}",
            json_escape(id),
            fmt_f64_json(rect.x),
            fmt_f64_json(rect.y),
            fmt_f64_json(rect.width),
            fmt_f64_json(rect.height),
            suffix
        ));
    }
    buf.push_str("],\"graph\":{");
    buf.push_str(&format!(
        "\"width\":{},\"height\":{}",
        fmt_f64_json(result.width),
        fmt_f64_json(result.height)
    ));
    buf.push_str("}}\n");

    if target == "1" {
        eprint!("{buf}");
    } else if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&target)
    {
        let _ = file.write_all(buf.as_bytes());
    }
}
