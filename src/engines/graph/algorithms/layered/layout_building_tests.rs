use std::collections::HashSet;

use super::kernel::{
    DiGraph, Direction as LayeredDirection, LayoutConfig as LayeredConfig, LayoutResult, NodeId,
    layout,
};
use super::layout_building::{build_layered_layout, compute_sublayouts, layered_config_for_layout};
use crate::diagrams::flowchart::compile_to_graph;
use crate::engines::graph::EngineConfig;
use crate::engines::graph::contracts::{GraphEngine, GraphGeometryContract, GraphSolveRequest};
use crate::engines::graph::flux::FluxLayeredEngine;
use crate::graph::grid::{GridLayout, GridLayoutConfig, geometry_to_grid_layout_with_routed};
use crate::graph::measure::{grid_edge_label_dimensions, grid_node_dimensions};
use crate::graph::{Direction, Graph};
use crate::mermaid::parse_flowchart;

fn compile_diagram(input: &str) -> Graph {
    let flowchart = parse_flowchart(input).expect("test flowchart should parse");
    compile_to_graph(&flowchart)
}

fn compute_layout(diagram: &Graph, config: &GridLayoutConfig) -> GridLayout {
    let engine = FluxLayeredEngine::text();
    let request = GraphSolveRequest::new(
        super::MeasurementMode::Grid,
        GraphGeometryContract::Canonical,
        crate::graph::GeometryLevel::Layout,
        None,
    );
    let result = engine
        .solve(
            diagram,
            &EngineConfig::Layered(layered_config_for_layout(diagram, config)),
            &request,
        )
        .expect("layered layout test solve failed");

    geometry_to_grid_layout_with_routed(diagram, &result.geometry, result.routed.as_ref(), config)
}

fn assert_subgraph_contains_members(layout: &GridLayout, sg_id: &str, members: &[&str]) {
    let sg = &layout.subgraph_bounds[sg_id];
    let sg_right = sg.x + sg.width;
    let sg_bottom = sg.y + sg.height;

    for member_id in members {
        let nb = &layout.node_bounds[*member_id];
        let nb_right = nb.x + nb.width;
        let nb_bottom = nb.y + nb.height;

        assert!(
            sg.x <= nb.x,
            "{sg_id} left ({}) should be <= {member_id} left ({})",
            sg.x,
            nb.x
        );
        assert!(
            sg.y <= nb.y,
            "{sg_id} top ({}) should be <= {member_id} top ({})",
            sg.y,
            nb.y
        );
        assert!(
            sg_right >= nb_right,
            "{sg_id} right ({sg_right}) should be >= {member_id} right ({nb_right})"
        );
        assert!(
            sg_bottom >= nb_bottom,
            "{sg_id} bottom ({sg_bottom}) should be >= {member_id} bottom ({nb_bottom})"
        );
    }
}

fn run_sublayout_for_sg(diagram: &Graph, sg_id: &str) -> LayoutResult {
    let sg = &diagram.subgraphs[sg_id];
    let sub_dir = sg.dir.expect("subgraph should have direction override");

    let layered_direction = match sub_dir {
        Direction::TopDown => LayeredDirection::TopBottom,
        Direction::BottomTop => LayeredDirection::BottomTop,
        Direction::LeftRight => LayeredDirection::LeftRight,
        Direction::RightLeft => LayeredDirection::RightLeft,
    };

    let mut sub_graph: DiGraph<(f64, f64)> = DiGraph::new();

    for node_id in &sg.nodes {
        if !diagram.is_subgraph(node_id)
            && let Some(node) = diagram.nodes.get(node_id)
        {
            let (w, h) = grid_node_dimensions(node, sub_dir);
            sub_graph.add_node(node_id.as_str(), (w as f64, h as f64));
        }
    }

    let sg_node_set: HashSet<&str> = sg.nodes.iter().map(|s| s.as_str()).collect();
    for edge in &diagram.edges {
        if sg_node_set.contains(edge.from.as_str()) && sg_node_set.contains(edge.to.as_str()) {
            sub_graph.add_edge(edge.from.as_str(), edge.to.as_str());
        }
    }

    let sub_config = LayeredConfig {
        direction: layered_direction,
        ..LayeredConfig::default()
    };

    layout(&sub_graph, &sub_config, |_, dims| *dims)
}

#[test]
fn build_layered_layout_includes_label_positions() {
    let diagram = compile_diagram("graph TD\nA -- yes --> B\n");

    let result = build_layered_layout(
        &diagram,
        &GridLayoutConfig::default(),
        |node| (node.label.len() as f64 + 4.0, 3.0),
        |edge| {
            edge.label
                .as_ref()
                .map(|label| grid_edge_label_dimensions(label))
        },
    );

    assert!(result.label_positions.contains_key(&0));
}

#[test]
fn test_layout_subgraph_bounds_present() {
    let diagram = compile_diagram("graph TD\nsubgraph sg1[Group]\nA --> B\nend\n");
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    assert!(
        layout.subgraph_bounds.contains_key("sg1"),
        "should have bounds for sg1"
    );
    let bounds = &layout.subgraph_bounds["sg1"];
    assert!(bounds.width > 0, "width should be positive");
    assert!(bounds.height > 0, "height should be positive");
    assert_eq!(bounds.title, "Group");
}

#[test]
fn test_nested_subgraph_layout_produces_both_bounds() {
    let diagram = compile_diagram(
        "graph TD\nsubgraph outer[Outer]\nA[Node A]\nsubgraph inner[Inner]\nB[Node B]\nend\nend\nA --> B\n",
    );
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    assert!(
        layout.subgraph_bounds.contains_key("outer"),
        "should have outer bounds"
    );
    assert!(
        layout.subgraph_bounds.contains_key("inner"),
        "should have inner bounds"
    );
}

#[test]
fn test_layout_no_subgraph_bounds_simple() {
    let diagram = compile_diagram("graph TD\nA --> B\n");
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    assert!(layout.subgraph_bounds.is_empty());
}

#[test]
fn test_layout_canvas_dimensions_include_borders() {
    let diagram = compile_diagram("graph TD\nsubgraph sg1[Group]\nA --> B\nend\n");
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    let bounds = &layout.subgraph_bounds["sg1"];
    assert!(
        layout.width >= bounds.x + bounds.width,
        "canvas width {} should contain border x+w={}",
        layout.width,
        bounds.x + bounds.width
    );
    assert!(
        layout.height >= bounds.y + bounds.height,
        "canvas height {} should contain border y+h={}",
        layout.height,
        bounds.y + bounds.height
    );
}

#[test]
fn test_compute_layout_subgraph_diagram_succeeds() {
    let diagram = compile_diagram("graph TD\nsubgraph sg1[Group]\nA --> B\nend\nC --> A\n");
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    assert!(layout.draw_positions.contains_key("A"));
    assert!(layout.draw_positions.contains_key("B"));
    assert!(layout.draw_positions.contains_key("C"));
}

#[test]
fn test_compute_layout_simple_diagram_no_compound() {
    let diagram = compile_diagram("graph TD\nA --> B\n");
    assert!(!diagram.has_subgraphs());

    let layout = compute_layout(&diagram, &GridLayoutConfig::default());
    assert!(layout.draw_positions.contains_key("A"));
}

#[test]
fn label_position_within_canvas_bounds() {
    let diagram = compile_diagram("graph TD\n    A -->|yes| B");
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    let edge_idx = diagram
        .edges
        .iter()
        .find(|e| e.from == "A" && e.to == "B")
        .expect("expected A -> B edge")
        .index;
    assert!(
        layout.edge_label_positions.contains_key(&edge_idx),
        "Should have precomputed label position for A->B, got keys: {:?}",
        layout.edge_label_positions.keys().collect::<Vec<_>>()
    );

    let (lx, ly) = layout.edge_label_positions[&edge_idx];
    assert!(
        lx < layout.width && ly < layout.height,
        "Label position ({}, {}) should be within canvas ({}, {})",
        lx,
        ly,
        layout.width,
        layout.height
    );
}

#[test]
fn test_nested_borders_inner_visible() {
    let diagram = compile_diagram(
        "graph TD\nsubgraph outer[Outer]\nA\nsubgraph inner[Inner]\nB --> C\nend\nend\nA --> B\n",
    );
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    assert!(
        layout.subgraph_bounds.contains_key("outer"),
        "layout should contain outer subgraph bounds"
    );
    assert!(
        layout.subgraph_bounds.contains_key("inner"),
        "layout should contain inner subgraph bounds"
    );
    assert_eq!(layout.subgraph_bounds["outer"].title, "Outer");
    assert_eq!(layout.subgraph_bounds["inner"].title, "Inner");
}

#[test]
fn test_nested_subgraph_depth_values() {
    let diagram =
        compile_diagram("graph TD\nsubgraph outer[Outer]\nA\nsubgraph inner[Inner]\nB\nend\nend\n");
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());
    assert_eq!(layout.subgraph_bounds["outer"].depth, 0);
    assert_eq!(layout.subgraph_bounds["inner"].depth, 1);
}

#[test]
fn test_nested_subgraph_parent_contains_child_bounds() {
    let diagram = compile_diagram(
        "graph TD\nsubgraph outer[Outer]\nA\nsubgraph inner[Inner]\nB --> C\nend\nend\nA --> B\n",
    );
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());
    let outer = &layout.subgraph_bounds["outer"];
    let inner = &layout.subgraph_bounds["inner"];

    assert!(
        outer.x <= inner.x,
        "outer.x ({}) should be <= inner.x ({})",
        outer.x,
        inner.x
    );
    assert!(
        outer.y <= inner.y,
        "outer.y ({}) should be <= inner.y ({})",
        outer.y,
        inner.y
    );
    assert!(
        outer.x + outer.width >= inner.x + inner.width,
        "outer right ({}) should be >= inner right ({})",
        outer.x + outer.width,
        inner.x + inner.width
    );
    assert!(
        outer.y + outer.height >= inner.y + inner.height,
        "outer bottom ({}) should be >= inner bottom ({})",
        outer.y + outer.height,
        inner.y + inner.height
    );
}

#[test]
fn test_nested_outer_only_subgraph_gets_bounds() {
    let diagram = compile_diagram(
        "graph TD\nsubgraph outer[Outer]\nsubgraph inner[Inner]\nA --> B\nend\nend\n",
    );
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());
    assert!(
        layout.subgraph_bounds.contains_key("outer"),
        "outer should have bounds"
    );
    let outer = &layout.subgraph_bounds["outer"];
    assert!(outer.width > 0, "width should be positive");
    assert!(outer.height > 0, "height should be positive");
}

#[test]
fn test_subgraph_bounds_expanded_for_title() {
    let diagram =
        compile_diagram("graph TD\nsubgraph sg1[This Is A Very Long Title]\nA --> B\nend\n");
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    let bounds = layout
        .subgraph_bounds
        .values()
        .next()
        .expect("Expected subgraph bounds");

    let min_width = "This Is A Very Long Title".len() + 6;
    assert!(
        bounds.width >= min_width,
        "Border width {} too narrow for title (need >= {})",
        bounds.width,
        min_width
    );
}

#[test]
fn test_titled_subgraph_creates_title_rank() {
    let diagram = compile_diagram(
        r#"graph TD
subgraph sg1[Processing]
    A[Step 1] --> B[Step 2]
end"#,
    );
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    assert!(layout.subgraph_bounds.contains_key("sg1"));
    let bounds = &layout.subgraph_bounds["sg1"];
    assert!(bounds.height > 0);
}

#[test]
fn stacked_subgraphs_do_not_overlap() {
    let diagram = compile_diagram(
        "graph TD\n\
        subgraph sg1[Input]\nA[Data]\nB[Config]\nend\n\
        subgraph sg2[Output]\nC[Result]\nD[Log]\nend\n\
        A --> C\nB --> D",
    );
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    let sg1 = &layout.subgraph_bounds["sg1"];
    let sg2 = &layout.subgraph_bounds["sg2"];
    let sg1_bottom = sg1.y + sg1.height;
    let sg2_bottom = sg2.y + sg2.height;

    let (_upper, lower, upper_bottom) = if sg1.y < sg2.y {
        (sg1, sg2, sg1_bottom)
    } else {
        (sg2, sg1, sg2_bottom)
    };

    assert!(
        upper_bottom <= lower.y,
        "Subgraphs should not overlap vertically: upper bottom={upper_bottom}, lower top={}",
        lower.y
    );
}

#[test]
fn subgraph_bounds_contain_member_node_bounds() {
    let diagram =
        compile_diagram("graph TD\nsubgraph sg1[Group]\nA[Node1]\nB[Node2]\nend\nA --> B");
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    assert_subgraph_contains_members(&layout, "sg1", &["A", "B"]);
}

#[test]
fn stacked_subgraph_bounds_contain_member_nodes_after_overlap_resolution() {
    let diagram = compile_diagram(
        "graph TD\n\
        subgraph sg1[Input]\nA[Data]\nB[Config]\nend\n\
        subgraph sg2[Output]\nC[Result]\nD[Log]\nend\n\
        A --> C\nB --> D",
    );
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    assert_subgraph_contains_members(&layout, "sg1", &["A", "B"]);
    assert_subgraph_contains_members(&layout, "sg2", &["C", "D"]);
}

#[test]
fn direction_override_field_available_at_layout() {
    let diagram = compile_diagram("graph TD\nsubgraph sg1[Group]\ndirection LR\nA --> B\nend\n");

    assert_eq!(diagram.subgraphs["sg1"].dir, Some(Direction::LeftRight));

    let layout = compute_layout(&diagram, &GridLayoutConfig::default());
    assert!(!layout.node_bounds.is_empty());
}

#[test]
fn sublayout_lr_nodes_arranged_horizontally() {
    let diagram = compile_diagram(
        "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA[Step 1] --> B[Step 2] --> C[Step 3]\nend\n",
    );
    let result = run_sublayout_for_sg(&diagram, "sg1");

    let a = &result.nodes[&NodeId::from("A")];
    let b = &result.nodes[&NodeId::from("B")];
    let c = &result.nodes[&NodeId::from("C")];

    let a_cx = a.x + a.width / 2.0;
    let b_cx = b.x + b.width / 2.0;
    let c_cx = c.x + c.width / 2.0;

    assert!(
        a_cx < b_cx,
        "A center_x ({a_cx}) should be < B center_x ({b_cx})"
    );
    assert!(
        b_cx < c_cx,
        "B center_x ({b_cx}) should be < C center_x ({c_cx})"
    );

    let a_cy = a.y + a.height / 2.0;
    let b_cy = b.y + b.height / 2.0;
    let c_cy = c.y + c.height / 2.0;

    assert!(
        (a_cy - b_cy).abs() < 1.0,
        "A and B should be at similar y: {a_cy} vs {b_cy}"
    );
    assert!(
        (b_cy - c_cy).abs() < 1.0,
        "B and C should be at similar y: {b_cy} vs {c_cy}"
    );
}

#[test]
fn sublayout_dimensions_wider_than_tall_for_lr() {
    let diagram = compile_diagram(
        "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA[Step 1] --> B[Step 2] --> C[Step 3]\nend\n",
    );
    let result = run_sublayout_for_sg(&diagram, "sg1");

    assert!(
        result.width > result.height,
        "LR sub-layout should be wider than tall: {}x{}",
        result.width,
        result.height
    );
}

#[test]
fn sublayout_bt_nodes_arranged_bottom_to_top() {
    let diagram = compile_diagram(
        "graph LR\nsubgraph sg1[Vertical]\ndirection BT\nA[Start] --> B[End]\nend\n",
    );
    let result = run_sublayout_for_sg(&diagram, "sg1");

    let a = &result.nodes[&NodeId::from("A")];
    let b = &result.nodes[&NodeId::from("B")];
    let a_cy = a.y + a.height / 2.0;
    let b_cy = b.y + b.height / 2.0;

    assert!(
        a_cy > b_cy,
        "In BT layout, A (start) should be below B (end): A_cy={a_cy} B_cy={b_cy}"
    );
}

#[test]
fn sublayout_rl_reverses_node_order() {
    let diagram = compile_diagram(
        "graph TD\nsubgraph sg1[Reverse]\ndirection RL\nA[Left] --> B[Right]\nend\n",
    );
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    let a = layout.get_bounds("A").expect("A should have bounds");
    let b = layout.get_bounds("B").expect("B should have bounds");

    assert!(
        a.center_x() > b.center_x(),
        "In RL layout, A should be right of B: A_cx={} B_cx={}",
        a.center_x(),
        b.center_x()
    );

    let y_tolerance = 2;
    assert!(
        (a.center_y() as isize - b.center_y() as isize).abs() <= y_tolerance,
        "A and B should be at similar y in RL: {} vs {}",
        a.center_y(),
        b.center_y()
    );
}

#[test]
fn direction_override_nodes_horizontal_in_final_layout() {
    let diagram = compile_diagram(
        "graph TD\nsubgraph sg1[Horizontal Section]\ndirection LR\nA[Step 1] --> B[Step 2] --> C[Step 3]\nend\nStart --> A\nC --> End\n",
    );
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    let a = layout.get_bounds("A").expect("A should have bounds");
    let b = layout.get_bounds("B").expect("B should have bounds");
    let c = layout.get_bounds("C").expect("C should have bounds");

    assert!(
        a.center_x() < b.center_x(),
        "A ({}) should be left of B ({})",
        a.center_x(),
        b.center_x()
    );
    assert!(
        b.center_x() < c.center_x(),
        "B ({}) should be left of C ({})",
        b.center_x(),
        c.center_x()
    );

    let y_tolerance = 2;
    assert!(
        (a.center_y() as isize - b.center_y() as isize).abs() <= y_tolerance,
        "A and B should be at similar y: {} vs {}",
        a.center_y(),
        b.center_y()
    );
    assert!(
        (b.center_y() as isize - c.center_y() as isize).abs() <= y_tolerance,
        "B and C should be at similar y: {} vs {}",
        b.center_y(),
        c.center_y()
    );
}

#[test]
fn direction_override_subgraph_wider_than_tall() {
    let diagram = compile_diagram(
        "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA[Step 1] --> B[Step 2] --> C[Step 3]\nend\n",
    );
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    let sg = &layout.subgraph_bounds["sg1"];
    assert!(
        sg.width > sg.height,
        "LR subgraph should be wider than tall: {}x{}",
        sg.width,
        sg.height
    );
}

#[test]
fn direction_override_bt_subgraph_taller_than_wide() {
    let diagram = compile_diagram(
        "graph LR\nsubgraph sg1[Vertical]\ndirection BT\nA[Top] --> B[Mid] --> C[Bot]\nend\n",
    );
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    let sg = &layout.subgraph_bounds["sg1"];
    assert!(
        sg.height > sg.width,
        "BT subgraph should be taller than wide: {}w x {}h",
        sg.width,
        sg.height
    );
}

#[test]
fn direction_override_subgraph_title_width_minimum() {
    let diagram = compile_diagram(
        "graph TD\nsubgraph sg1[A Very Long Section Title]\ndirection LR\nA --> B\nend\n",
    );
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    let sg = &layout.subgraph_bounds["sg1"];
    let title = "A Very Long Section Title";
    assert!(
        sg.width >= title.len(),
        "Subgraph width ({}) should accommodate title length ({})",
        sg.width,
        title.len()
    );
}

#[test]
fn direction_override_nodes_inside_subgraph_bounds() {
    let diagram = compile_diagram(
        "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA[Step 1] --> B[Step 2] --> C[Step 3]\nend\nStart --> A\nC --> End\n",
    );
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    assert_subgraph_contains_members(&layout, "sg1", &["A", "B", "C"]);
}

#[test]
fn direction_override_no_node_overlap() {
    let diagram = compile_diagram(
        "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA[Step 1] --> B[Step 2] --> C[Step 3]\nend\nStart --> A\nC --> End\n",
    );
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    let nodes = ["A", "B", "C"];
    for i in 0..nodes.len() {
        for j in (i + 1)..nodes.len() {
            let a = layout
                .get_bounds(nodes[i])
                .expect("node should have bounds");
            let b = layout
                .get_bounds(nodes[j])
                .expect("node should have bounds");
            let overlap_x = a.x < b.x + b.width && b.x < a.x + a.width;
            let overlap_y = a.y < b.y + b.height && b.y < a.y + a.height;
            assert!(
                !(overlap_x && overlap_y),
                "Nodes {} and {} should not overlap: {:?} vs {:?}",
                nodes[i],
                nodes[j],
                (a.x, a.y, a.width, a.height),
                (b.x, b.y, b.width, b.height)
            );
        }
    }
}

#[test]
fn direction_override_external_nodes_outside_subgraph() {
    let diagram = compile_diagram(
        "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA[Step 1] --> B[Step 2]\nend\nStart --> A\nB --> End\n",
    );
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    let sg = &layout.subgraph_bounds["sg1"];
    for ext_node in &["Start", "End"] {
        let bounds = layout
            .get_bounds(ext_node)
            .expect("node should have bounds");
        let inside_x = bounds.x >= sg.x && bounds.x + bounds.width <= sg.x + sg.width;
        let inside_y = bounds.y >= sg.y && bounds.y + bounds.height <= sg.y + sg.height;
        assert!(
            !(inside_x && inside_y),
            "External node {} should not be fully inside sg1 bounds",
            ext_node
        );
    }
}

#[test]
fn cross_boundary_edge_no_panic() {
    let diagram = compile_diagram(
        "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA --> B\nend\nC --> A\nB --> D\n",
    );
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    assert!(layout.node_bounds.contains_key("A"));
    assert!(layout.node_bounds.contains_key("B"));
    assert!(layout.node_bounds.contains_key("C"));
    assert!(layout.node_bounds.contains_key("D"));
    assert!(layout.subgraph_bounds.contains_key("sg1"));
    assert_eq!(layout.subgraph_bounds["sg1"].title, "Horizontal");
}

#[test]
fn node_effective_direction_populated() {
    let diagram = compile_diagram(
        "graph TD\nsubgraph sg1[Group]\ndirection LR\nA --> B\nend\nC --> A\nB --> D\n",
    );
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    assert_eq!(
        layout.node_directions.get("A"),
        Some(&Direction::LeftRight),
        "A should have LR direction"
    );
    assert_eq!(
        layout.node_directions.get("B"),
        Some(&Direction::LeftRight),
        "B should have LR direction"
    );
    assert_eq!(
        layout.node_directions.get("C"),
        Some(&Direction::TopDown),
        "C should have TD direction"
    );
    assert_eq!(
        layout.node_directions.get("D"),
        Some(&Direction::TopDown),
        "D should have TD direction"
    );
}

#[test]
fn sublayout_excludes_cross_boundary_edges() {
    let diagram = compile_diagram(
        "graph TD\nsubgraph sg1[Group]\ndirection LR\nA --> B\nend\nStart --> A\nB --> End\n",
    );
    let result = run_sublayout_for_sg(&diagram, "sg1");

    assert!(result.nodes.contains_key(&NodeId::from("A")));
    assert!(result.nodes.contains_key(&NodeId::from("B")));
    assert!(!result.nodes.contains_key(&NodeId::from("Start")));
    assert!(!result.nodes.contains_key(&NodeId::from("End")));
}

#[test]
fn compute_sublayouts_skips_non_isolated_when_flag_set() {
    let diagram =
        compile_diagram("graph TD\nsubgraph sg1[Group]\ndirection LR\nA --> B\nend\nC --> A");
    let layered_config = LayeredConfig::default();

    let subs_false = compute_sublayouts(
        &diagram,
        &layered_config,
        |_node| (40.0, 20.0),
        |_edge| None,
        false,
    );
    let lr_result = &subs_false["sg1"];
    let a_lr = lr_result.result.nodes[&NodeId::from("A")];
    let b_lr = lr_result.result.nodes[&NodeId::from("B")];
    assert!(
        (a_lr.y - b_lr.y).abs() < 1.0,
        "LR: A.y={} B.y={} should be similar",
        a_lr.y,
        b_lr.y
    );

    let subs_true = compute_sublayouts(
        &diagram,
        &layered_config,
        |_node| (40.0, 20.0),
        |_edge| None,
        true,
    );
    assert!(
        !subs_true.contains_key("sg1"),
        "non-isolated sublayout should be skipped"
    );
}
