use super::*;
use crate::engines::graph::algorithms::layered::layout_building::{
    build_layered_layout, compute_sublayouts,
};
use crate::engines::graph::algorithms::layered::{
    self, Direction as LayeredDirection, LayoutConfig as LayeredConfig,
};
use crate::graph::geometry::FPoint;
use crate::graph::grid::GridLayout;
use crate::runtime::test_support_tests::{compute_layout, render_text_diagram};

fn test_node_bounds(x: usize, y: usize, width: usize, height: usize) -> NodeBounds {
    NodeBounds {
        x,
        y,
        width,
        height,
        layout_center_x: None,
        layout_center_y: None,
    }
}

fn segment_intersects_node(a: (usize, usize), b: (usize, usize), bounds: &NodeBounds) -> bool {
    let left = bounds.x;
    let right = bounds.x + bounds.width.saturating_sub(1);
    let top = bounds.y;
    let bottom = bounds.y + bounds.height.saturating_sub(1);

    if a.0 == b.0 {
        let x = a.0;
        let (y_min, y_max) = if a.1 <= b.1 { (a.1, b.1) } else { (b.1, a.1) };
        return x >= left && x <= right && y_min <= bottom && top <= y_max;
    }

    if a.1 == b.1 {
        let y = a.1;
        let (x_min, x_max) = if a.0 <= b.0 { (a.0, b.0) } else { (b.0, a.0) };
        return y >= top && y <= bottom && x_min <= right && left <= x_max;
    }

    false
}

fn segment_chain_clears_nodes(waypoints: &[(usize, usize)], bounds: &[NodeBounds]) -> bool {
    waypoints.windows(2).all(|pair| {
        bounds
            .iter()
            .all(|bounds| !segment_intersects_node(pair[0], pair[1], bounds))
    })
}

// =========================================================================
// Scale Factor Tests (Phase 2)
// =========================================================================

#[test]
fn scale_factors_td_typical() {
    // Typical TD: 3 nodes with widths 9,7,11 and heights all 3
    // avg_w = 9.0, max_h = 3
    // rank_sep = 50.0, node_sep = 50.0, v_spacing = 3, h_spacing = 4
    // scale_y (primary) = (3 + 3) / (3 + 50) = 6/53
    // scale_x (cross)   = (9 + 4) / (9 + 50) = 13/59
    let mut dims = HashMap::new();
    dims.insert("A".into(), (9, 3));
    dims.insert("B".into(), (7, 3));
    dims.insert("C".into(), (11, 3));

    let (sx, sy) = compute_grid_scale_factors(&dims, 50.0, 50.0, 3, 4, true, false);

    let expected_sy = 6.0 / 53.0;
    let expected_sx = 13.0 / 59.0;
    assert!(
        (sx - expected_sx).abs() < 1e-6,
        "sx: got {sx}, expected {expected_sx}"
    );
    assert!(
        (sy - expected_sy).abs() < 1e-6,
        "sy: got {sy}, expected {expected_sy}"
    );
}

#[test]
fn scale_factors_lr_direction_aware() {
    // LR: nodes widths 9,9, heights 3,3 → avg_h = 3, max_w = 9
    // scale_x (primary) = (9 + 4) / (9 + 50) = 13/59
    // scale_y (cross)   = (3 + 3) / (3 + 6) = 6/9
    let mut dims = HashMap::new();
    dims.insert("A".into(), (9, 3));
    dims.insert("B".into(), (9, 3));

    let (sx, sy) = compute_grid_scale_factors(&dims, 50.0, 6.0, 3, 4, false, false);

    let expected_sx = 13.0 / 59.0;
    let expected_sy = 6.0 / 9.0;
    assert!(
        (sx - expected_sx).abs() < 1e-6,
        "sx: got {sx}, expected {expected_sx}"
    );
    assert!(
        (sy - expected_sy).abs() < 1e-6,
        "sy: got {sy}, expected {expected_sy}"
    );
}

#[test]
fn scale_factors_single_node() {
    let mut dims = HashMap::new();
    dims.insert("X".into(), (5, 3));

    let (sx, sy) = compute_grid_scale_factors(&dims, 50.0, 50.0, 3, 4, true, false);
    assert!(sx > 0.0, "sx should be positive, got {sx}");
    assert!(sy > 0.0, "sy should be positive, got {sy}");
    assert!(sx.is_finite());
    assert!(sy.is_finite());
}

// =========================================================================
// Layered Layout Helper Tests
// =========================================================================

#[test]
fn build_layered_layout_includes_label_positions() {
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = "graph TD\nA -- yes --> B\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);

    let result = build_layered_layout(
        &diagram,
        &GridLayoutConfig::default(),
        |node| (node.label.len() as f64 + 4.0, 3.0),
        |edge| {
            edge.label
                .as_ref()
                .map(|label| crate::graph::measure::grid_edge_label_dimensions(label))
        },
    );

    assert!(result.label_positions.contains_key(&0));
}

#[test]
fn scale_factors_halved_for_doubled_ranks() {
    // With ranks_doubled=true, effective_rank_sep = max_h + 2*rank_sep = 3 + 100 = 103
    // scale_y = (max_h + v_spacing) / (max_h + eff_rs) = 6/106
    // This is exactly half of the non-doubled scale: 6/53 / 2 = 6/106
    let mut dims = HashMap::new();
    dims.insert("A".into(), (9, 3));
    dims.insert("B".into(), (7, 3));

    let (_, sy_normal) = compute_grid_scale_factors(&dims, 50.0, 50.0, 3, 4, true, false);
    let (_, sy_doubled) = compute_grid_scale_factors(&dims, 50.0, 50.0, 3, 4, true, true);

    // Doubled-rank scale should be exactly half of normal scale
    let expected_sy = sy_normal / 2.0;
    assert!(
        (sy_doubled - expected_sy).abs() < 1e-6,
        "sy_doubled: got {sy_doubled}, expected {expected_sy} (half of {sy_normal})"
    );

    // Verify: gap_new = 2*rank_sep*scale_doubled = gap_old = rank_sep*scale_normal
    let gap_normal = 50.0 * sy_normal;
    let gap_doubled = 100.0 * sy_doubled;
    assert!(
        (gap_normal - gap_doubled).abs() < 1e-6,
        "Gaps should match: normal={gap_normal}, doubled={gap_doubled}"
    );
}

#[test]
fn scale_factors_empty_nodes() {
    let dims: HashMap<String, (usize, usize)> = HashMap::new();
    let (sx, sy) = compute_grid_scale_factors(&dims, 50.0, 50.0, 3, 4, true, false);
    assert!(sx.is_finite());
    assert!(sy.is_finite());
}

// =========================================================================
// Collision Repair Tests (Phase 3)
// =========================================================================

#[test]
fn collision_repair_pushes_overlapping_nodes_apart() {
    let layers = vec![vec!["A".into(), "B".into()]];
    let mut positions: HashMap<String, (usize, usize)> = HashMap::new();
    positions.insert("A".into(), (0, 0));
    positions.insert("B".into(), (5, 0));
    let dims: HashMap<String, (usize, usize)> = [("A".into(), (8, 3)), ("B".into(), (8, 3))].into();

    collision_repair(&layers, &mut positions, &dims, true, 4);

    assert_eq!(positions["A"], (0, 0), "A should not move");
    assert_eq!(positions["B"], (12, 0), "B pushed to right edge of A + gap");
}

#[test]
fn collision_repair_cascading() {
    let layers = vec![vec!["A".into(), "B".into(), "C".into()]];
    let mut positions: HashMap<String, (usize, usize)> = HashMap::new();
    positions.insert("A".into(), (0, 0));
    positions.insert("B".into(), (3, 0));
    positions.insert("C".into(), (8, 0));
    let dims: HashMap<String, (usize, usize)> = [
        ("A".into(), (6, 3)),
        ("B".into(), (6, 3)),
        ("C".into(), (6, 3)),
    ]
    .into();

    collision_repair(&layers, &mut positions, &dims, true, 2);

    assert_eq!(positions["A"], (0, 0));
    assert_eq!(positions["B"], (8, 0));
    assert_eq!(positions["C"], (16, 0));
}

#[test]
fn collision_repair_no_change_when_spaced() {
    let layers = vec![vec!["A".into(), "B".into()]];
    let mut positions: HashMap<String, (usize, usize)> = HashMap::new();
    positions.insert("A".into(), (0, 0));
    positions.insert("B".into(), (20, 0));
    let dims: HashMap<String, (usize, usize)> = [("A".into(), (8, 3)), ("B".into(), (8, 3))].into();

    collision_repair(&layers, &mut positions, &dims, true, 4);

    assert_eq!(positions["A"], (0, 0));
    assert_eq!(positions["B"], (20, 0));
}

#[test]
fn collision_repair_horizontal_layout() {
    let layers = vec![vec!["A".into(), "B".into()]];
    let mut positions: HashMap<String, (usize, usize)> = HashMap::new();
    positions.insert("A".into(), (0, 0));
    positions.insert("B".into(), (0, 2));
    let dims: HashMap<String, (usize, usize)> = [("A".into(), (8, 3)), ("B".into(), (8, 3))].into();

    collision_repair(&layers, &mut positions, &dims, false, 3);

    assert_eq!(positions["A"], (0, 0));
    assert_eq!(positions["B"], (0, 6));
}

#[test]
fn collision_repair_single_node_layer_noop() {
    let layers = vec![vec!["A".into()]];
    let mut positions: HashMap<String, (usize, usize)> = HashMap::new();
    positions.insert("A".into(), (5, 5));
    let dims: HashMap<String, (usize, usize)> = [("A".into(), (8, 3))].into();

    collision_repair(&layers, &mut positions, &dims, true, 4);

    assert_eq!(positions["A"], (5, 5));
}

#[test]
fn collision_repair_sorts_by_cross_axis() {
    let layers = vec![vec!["A".into(), "B".into()]];
    let mut positions: HashMap<String, (usize, usize)> = HashMap::new();
    positions.insert("A".into(), (20, 0));
    positions.insert("B".into(), (0, 0));
    let dims: HashMap<String, (usize, usize)> = [("A".into(), (8, 3)), ("B".into(), (8, 3))].into();

    collision_repair(&layers, &mut positions, &dims, true, 4);

    assert_eq!(positions["B"], (0, 0));
    assert_eq!(positions["A"], (20, 0));
}

// =========================================================================
// Waypoint Transform Tests (Phase 4)
// =========================================================================

#[test]
fn waypoint_transform_vertical_basic() {
    use crate::graph::{Arrow, Stroke};
    let edges = vec![
        Edge::new("A", "C")
            .with_stroke(Stroke::Solid)
            .with_arrows(Arrow::None, Arrow::Normal),
    ];

    let mut waypoints = HashMap::new();
    waypoints.insert(0usize, vec![(FPoint::new(100.0, 75.0), 1)]);

    let layer_starts = vec![1, 5, 9];
    let ctx = TransformContext {
        layout_min_x: 50.0,
        layout_min_y: 25.0,
        scale_x: 0.22,
        scale_y: 0.11,
        padding: 1,
        left_label_margin: 0,
        overhang_x: 0,
        overhang_y: 0,
    };
    let result = transform_waypoints_direct(&waypoints, &edges, &ctx, &layer_starts, true, 80, 20);

    assert!(
        result.contains_key(&0),
        "should have waypoints for edge 0 (A→C)"
    );
    let wps = &result[&0];
    assert_eq!(wps.len(), 1);
    assert_eq!(wps[0].1, 5, "y should be layer_starts[1]");
    assert_eq!(wps[0].0, 12, "x should be scaled layout x + padding");
}

#[test]
fn waypoint_transform_horizontal_basic() {
    use crate::graph::{Arrow, Stroke};
    let edges = vec![
        Edge::new("A", "C")
            .with_stroke(Stroke::Solid)
            .with_arrows(Arrow::None, Arrow::Normal),
    ];

    let mut waypoints = HashMap::new();
    waypoints.insert(0usize, vec![(FPoint::new(75.0, 100.0), 1)]);

    let layer_starts = vec![1, 8, 15];
    let ctx = TransformContext {
        layout_min_x: 25.0,
        layout_min_y: 50.0,
        scale_x: 0.22,
        scale_y: 0.67,
        padding: 1,
        left_label_margin: 0,
        overhang_x: 0,
        overhang_y: 0,
    };
    let result = transform_waypoints_direct(&waypoints, &edges, &ctx, &layer_starts, false, 40, 80);

    let wps = &result[&0];
    assert_eq!(wps[0].0, 8, "x should be layer_starts[1]");
    assert_eq!(wps[0].1, 35, "y should be scaled layout y + padding");
}

#[test]
fn waypoint_transform_clamps_to_canvas() {
    use crate::graph::{Arrow, Stroke};
    let edges = vec![
        Edge::new("A", "B")
            .with_stroke(Stroke::Solid)
            .with_arrows(Arrow::None, Arrow::Normal),
    ];

    let mut waypoints = HashMap::new();
    waypoints.insert(0usize, vec![(FPoint::new(5000.0, 50.0), 0)]);

    let layer_starts = vec![1];
    let ctx = TransformContext {
        layout_min_x: 0.0,
        layout_min_y: 0.0,
        scale_x: 0.5,
        scale_y: 0.5,
        padding: 1,
        left_label_margin: 0,
        overhang_x: 0,
        overhang_y: 0,
    };
    let result = transform_waypoints_direct(&waypoints, &edges, &ctx, &layer_starts, true, 30, 20);

    let wps = &result[&0];
    assert!(wps[0].0 <= 29, "x clamped to canvas_width - 1");
}

#[test]
fn waypoint_transform_empty_input() {
    let edges: Vec<Edge> = vec![];
    let waypoints: HashMap<usize, Vec<(FPoint, i32)>> = HashMap::new();
    let ctx = TransformContext {
        layout_min_x: 0.0,
        layout_min_y: 0.0,
        scale_x: 0.2,
        scale_y: 0.1,
        padding: 1,
        left_label_margin: 0,
        overhang_x: 0,
        overhang_y: 0,
    };
    let result = transform_waypoints_direct(&waypoints, &edges, &ctx, &[], true, 80, 20);
    assert!(result.is_empty());
}

#[test]
fn nudge_colliding_waypoints_repairs_segment_collision_not_just_point_collision() {
    let mut edge_waypoints = HashMap::from([(0usize, vec![(20, 10), (40, 10)])]);
    let blocking_node = test_node_bounds(28, 8, 8, 4);
    let node_bounds = HashMap::from([("blocker".to_string(), blocking_node)]);

    nudge_colliding_waypoints(&mut edge_waypoints, &node_bounds, true, 80, 40);

    let repaired = edge_waypoints
        .get(&0)
        .expect("test edge should still have waypoints");
    assert!(
        segment_chain_clears_nodes(repaired, &[blocking_node]),
        "segment-wise repair should clear nodes even when waypoint points stay outside the node: {repaired:?}"
    );
}

// =========================================================================
// Label Transform Tests (Phase 5)
// =========================================================================

#[test]
fn label_transform_basic_scaling() {
    use crate::graph::{Arrow, Stroke};
    let edges = vec![
        Edge::new("A", "B")
            .with_label("yes")
            .with_stroke(Stroke::Solid)
            .with_arrows(Arrow::None, Arrow::Normal),
    ];

    let mut labels = HashMap::new();
    labels.insert(0usize, (FPoint::new(150.0, 100.0), 1));

    let ctx = TransformContext {
        layout_min_x: 50.0,
        layout_min_y: 50.0,
        scale_x: 0.22,
        scale_y: 0.11,
        padding: 1,
        left_label_margin: 0,
        overhang_x: 0,
        overhang_y: 0,
    };
    // layer_starts: rank 0 → y=0, rank 1 → y=8, rank 2 → y=16
    let layer_starts = vec![0, 8, 16];
    let result =
        transform_label_positions_direct(&labels, &edges, &ctx, &layer_starts, true, 50, 20);

    assert!(result.contains_key(&0));
    // x uses uniform scale: (150-50)*0.22 + 1 = 23
    // y = layer_starts[rank=1] = 8
    assert_eq!(result[&0], (23, 8));
}

#[test]
fn label_transform_with_left_margin() {
    use crate::graph::{Arrow, Stroke};
    let edges = vec![
        Edge::new("A", "B")
            .with_label("yes")
            .with_stroke(Stroke::Solid)
            .with_arrows(Arrow::None, Arrow::Normal),
    ];

    let mut labels = HashMap::new();
    labels.insert(0usize, (FPoint::new(150.0, 100.0), 1));

    let ctx = TransformContext {
        layout_min_x: 50.0,
        layout_min_y: 50.0,
        scale_x: 0.22,
        scale_y: 0.11,
        padding: 1,
        left_label_margin: 3,
        overhang_x: 0,
        overhang_y: 0,
    };
    let layer_starts = vec![0, 8, 16];
    let result =
        transform_label_positions_direct(&labels, &edges, &ctx, &layer_starts, true, 50, 20);

    // x = 23 + 3 (left_label_margin) = 26
    assert_eq!(result[&0].0, 26);
}

#[test]
fn label_transform_empty_input() {
    let edges: Vec<Edge> = vec![];
    let labels: HashMap<usize, (FPoint, i32)> = HashMap::new();
    let ctx = TransformContext {
        layout_min_x: 0.0,
        layout_min_y: 0.0,
        scale_x: 0.2,
        scale_y: 0.1,
        padding: 1,
        left_label_margin: 0,
        overhang_x: 0,
        overhang_y: 0,
    };
    let layer_starts: Vec<usize> = vec![];
    let result =
        transform_label_positions_direct(&labels, &edges, &ctx, &layer_starts, true, 50, 20);
    assert!(result.is_empty());
}

// =========================================================================
// Compound Graph Wiring Tests
// =========================================================================

#[test]
fn test_layout_subgraph_bounds_present() {
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = "graph TD\nsubgraph sg1[Group]\nA --> B\nend\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
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
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = "graph TD\nsubgraph outer[Outer]\nA[Node A]\nsubgraph inner[Inner]\nB[Node B]\nend\nend\nA --> B\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
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
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = "graph TD\nA --> B\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    assert!(layout.subgraph_bounds.is_empty());
}

#[test]
fn test_layout_canvas_dimensions_include_borders() {
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = "graph TD\nsubgraph sg1[Group]\nA --> B\nend\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
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
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = "graph TD\nsubgraph sg1[Group]\nA --> B\nend\nC --> A\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);

    // Should not panic
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());
    assert!(layout.draw_positions.contains_key("A"));
    assert!(layout.draw_positions.contains_key("B"));
    assert!(layout.draw_positions.contains_key("C"));
}

#[test]
fn test_compute_layout_simple_diagram_no_compound() {
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = "graph TD\nA --> B\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    assert!(!diagram.has_subgraphs());

    let layout = compute_layout(&diagram, &GridLayoutConfig::default());
    assert!(layout.draw_positions.contains_key("A"));
}

#[test]
fn label_position_within_canvas_bounds() {
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = "graph TD\n    A -->|yes| B";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    // Label position should exist — edge A→B is at index 0
    let edge_idx = diagram
        .edges
        .iter()
        .find(|e| e.from == "A" && e.to == "B")
        .unwrap()
        .index;
    assert!(
        layout.edge_label_positions.contains_key(&edge_idx),
        "Should have precomputed label position for A->B, got keys: {:?}",
        layout.edge_label_positions.keys().collect::<Vec<_>>()
    );

    let (lx, ly) = layout.edge_label_positions[&edge_idx];
    // Should be within canvas bounds
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
fn label_transform_skips_missing_edge() {
    use crate::graph::{Arrow, Stroke};
    let edges = vec![
        Edge::new("A", "B")
            .with_label("x")
            .with_stroke(Stroke::Solid)
            .with_arrows(Arrow::None, Arrow::Normal),
    ];

    let mut labels = HashMap::new();
    labels.insert(5usize, (FPoint::new(100.0, 100.0), 0));

    let ctx = TransformContext {
        layout_min_x: 0.0,
        layout_min_y: 0.0,
        scale_x: 0.2,
        scale_y: 0.1,
        padding: 1,
        left_label_margin: 0,
        overhang_x: 0,
        overhang_y: 0,
    };
    let layer_starts = vec![0];
    let result =
        transform_label_positions_direct(&labels, &edges, &ctx, &layer_starts, true, 50, 20);

    assert!(
        result.is_empty(),
        "out-of-bounds edge index should be skipped"
    );
}

// =========================================================================
// Nested Subgraph Tests (Plan 0032)
// =========================================================================

#[test]
fn test_nested_borders_inner_visible() {
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input =
        "graph TD\nsubgraph outer[Outer]\nA\nsubgraph inner[Inner]\nB --> C\nend\nend\nA --> B\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    let output = render_text_diagram(&diagram);
    assert!(
        output.contains("Outer"),
        "Output should contain 'Outer' title"
    );
    assert!(
        output.contains("Inner"),
        "Output should contain 'Inner' title"
    );
}

#[test]
fn test_nested_subgraph_depth_values() {
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = "graph TD\nsubgraph outer[Outer]\nA\nsubgraph inner[Inner]\nB\nend\nend\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());
    assert_eq!(layout.subgraph_bounds["outer"].depth, 0);
    assert_eq!(layout.subgraph_bounds["inner"].depth, 1);
}

#[test]
fn test_nested_subgraph_parent_contains_child_bounds() {
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input =
        "graph TD\nsubgraph outer[Outer]\nA\nsubgraph inner[Inner]\nB --> C\nend\nend\nA --> B\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());
    let outer = &layout.subgraph_bounds["outer"];
    let inner = &layout.subgraph_bounds["inner"];
    // Parent must fully contain child
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
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = "graph TD\nsubgraph outer[Outer]\nsubgraph inner[Inner]\nA --> B\nend\nend\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
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
fn test_build_children_map() {
    use crate::graph::Subgraph;
    let mut subgraphs = HashMap::new();
    subgraphs.insert(
        "inner".to_string(),
        Subgraph {
            id: "inner".to_string(),
            title: "Inner".to_string(),
            nodes: vec!["A".to_string()],
            parent: Some("outer".to_string()),
            dir: None,
        },
    );
    subgraphs.insert(
        "outer".to_string(),
        Subgraph {
            id: "outer".to_string(),
            title: "Outer".to_string(),
            nodes: vec!["A".to_string()],
            parent: None,
            dir: None,
        },
    );
    let children_map = build_children_map(&subgraphs);
    assert_eq!(children_map["outer"], vec!["inner".to_string()]);
    assert!(!children_map.contains_key("inner"));
}

// =========================================================================
// Subgraph Bounds Tests (Layout-derived bounds)
// =========================================================================

#[test]
fn test_subgraph_bounds_no_overlap_from_separated_rects() {
    use crate::graph::Subgraph;

    let mut subgraphs = HashMap::new();
    subgraphs.insert(
        "sg1".to_string(),
        Subgraph {
            id: "sg1".to_string(),
            title: "Left".to_string(),
            nodes: vec!["A".to_string()],
            parent: None,
            dir: None,
        },
    );
    subgraphs.insert(
        "sg2".to_string(),
        Subgraph {
            id: "sg2".to_string(),
            title: "Right".to_string(),
            nodes: vec!["B".to_string()],
            parent: None,
            dir: None,
        },
    );

    let mut layout_bounds = HashMap::new();
    layout_bounds.insert(
        "sg1".to_string(),
        FRect {
            x: 10.0,
            y: 10.0,
            width: 10.0,
            height: 5.0,
        },
    );
    layout_bounds.insert(
        "sg2".to_string(),
        FRect {
            x: 40.0,
            y: 10.0,
            width: 10.0,
            height: 5.0,
        },
    );

    let config = GridLayoutConfig {
        padding: 0,
        left_label_margin: 0,
        ..GridLayoutConfig::default()
    };

    let transform = CoordTransform {
        scale_x: 1.0,
        scale_y: 1.0,
        layout_min_x: 0.0,
        layout_min_y: 0.0,
        max_overhang_x: 0,
        max_overhang_y: 0,
        config: &config,
    };

    let result = subgraph_bounds_to_draw(&subgraphs, &layout_bounds, &transform);

    let a = &result["sg1"];
    let b = &result["sg2"];

    // Separated member nodes should produce non-overlapping draw bounds
    let no_x_overlap = a.x + a.width <= b.x || b.x + b.width <= a.x;
    let no_y_overlap = a.y + a.height <= b.y || b.y + b.height <= a.y;
    assert!(
        no_x_overlap || no_y_overlap,
        "Bounds should not overlap: sg1=({},{} {}x{}) sg2=({},{} {}x{})",
        a.x,
        a.y,
        a.width,
        a.height,
        b.x,
        b.y,
        b.width,
        b.height
    );
}

#[test]
fn test_subgraph_bounds_maps_rects() {
    use crate::graph::Subgraph;

    let mut subgraphs = HashMap::new();
    subgraphs.insert(
        "sg1".to_string(),
        Subgraph {
            id: "sg1".to_string(),
            title: "G".to_string(),
            nodes: vec!["A".to_string()],
            parent: None,
            dir: None,
        },
    );

    let mut layout_bounds = HashMap::new();
    layout_bounds.insert(
        "sg1".to_string(),
        FRect {
            x: 10.0,
            y: 10.0,
            width: 5.0,
            height: 3.0,
        },
    );

    let config = GridLayoutConfig {
        padding: 0,
        left_label_margin: 0,
        ..GridLayoutConfig::default()
    };

    let transform = CoordTransform {
        scale_x: 1.0,
        scale_y: 1.0,
        layout_min_x: 0.0,
        layout_min_y: 0.0,
        max_overhang_x: 0,
        max_overhang_y: 0,
        config: &config,
    };

    let result = subgraph_bounds_to_draw(&subgraphs, &layout_bounds, &transform);

    let b = &result["sg1"];
    // Title "G" requires min width = len("G") + 6 = 7, which exceeds rect width 5.
    // Title-width enforcement expands by (7-5)=2 and shifts x left by 2/2=1.
    assert_eq!(b.x, 9, "x shifted left by 1 due to title-width expansion");
    assert_eq!(b.y, 10, "y should match layout rect y");
    assert_eq!(b.width, 7, "width expanded to fit title");
    assert_eq!(b.height, 3, "height should match layout rect height");
}

// =========================================================================
// Title Width Enforcement Tests (Plan 0026, Task 2.3)
// =========================================================================

#[test]
fn test_subgraph_bounds_expanded_for_title() {
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = "graph TD\nsubgraph sg1[This Is A Very Long Title]\nA --> B\nend\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    let bounds = layout
        .subgraph_bounds
        .values()
        .next()
        .expect("Expected subgraph bounds");

    // Border must be wide enough for: corners (2) + "─ " (2) + title + " ─" (2)
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
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = r#"graph TD
subgraph sg1[Processing]
    A[Step 1] --> B[Step 2]
end"#;

    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    let config = GridLayoutConfig::default();
    let layout = compute_layout(&diagram, &config);

    assert!(layout.subgraph_bounds.contains_key("sg1"));
    let bounds = &layout.subgraph_bounds["sg1"];
    assert!(bounds.height > 0);
}

// =========================================================================
// to_grid_rect() Tests (Plan 0028, Task 1.1)
// =========================================================================

#[test]
fn to_ascii_rect_at_layout_minimum() {
    // A rect centered at the layout minimum should produce draw coords near origin + padding
    let ctx = TransformContext {
        layout_min_x: 50.0,
        layout_min_y: 30.0,
        scale_x: 0.2,
        scale_y: 0.1,
        overhang_x: 2,
        overhang_y: 1,
        padding: 1,
        left_label_margin: 0,
    };
    let rect = FRect {
        x: 50.0,
        y: 30.0,
        width: 40.0,
        height: 20.0,
    };
    let (_x, _y, w, h) = ctx.to_grid_rect(&rect);
    assert!(w > 0, "width should be positive, got {w}");
    assert!(h > 0, "height should be positive, got {h}");
}

#[test]
fn to_ascii_rect_offset_from_minimum() {
    // A rect offset from layout minimum should have proportionally offset draw coords
    let ctx = TransformContext {
        layout_min_x: 0.0,
        layout_min_y: 0.0,
        scale_x: 0.2,
        scale_y: 0.1,
        overhang_x: 0,
        overhang_y: 0,
        padding: 0,
        left_label_margin: 0,
    };
    let rect1 = FRect {
        x: 50.0,
        y: 50.0,
        width: 40.0,
        height: 20.0,
    };
    let rect2 = FRect {
        x: 100.0,
        y: 100.0,
        width: 40.0,
        height: 20.0,
    };
    let (x1, y1, _, _) = ctx.to_grid_rect(&rect1);
    let (x2, y2, _, _) = ctx.to_grid_rect(&rect2);
    assert!(x2 > x1, "rect2 should be further right: x2={x2} vs x1={x1}");
    assert!(y2 > y1, "rect2 should be further down: y2={y2} vs y1={y1}");
}

#[test]
fn to_ascii_rect_dimensions_scale_with_layout_size() {
    let ctx = TransformContext {
        layout_min_x: 0.0,
        layout_min_y: 0.0,
        scale_x: 0.5,
        scale_y: 0.5,
        overhang_x: 0,
        overhang_y: 0,
        padding: 0,
        left_label_margin: 0,
    };
    let small = FRect {
        x: 50.0,
        y: 50.0,
        width: 20.0,
        height: 10.0,
    };
    let large = FRect {
        x: 50.0,
        y: 50.0,
        width: 60.0,
        height: 30.0,
    };
    let (_, _, w1, h1) = ctx.to_grid_rect(&small);
    let (_, _, w2, h2) = ctx.to_grid_rect(&large);
    assert!(
        w2 > w1,
        "larger rect should have larger width: w2={w2} vs w1={w1}"
    );
    assert!(
        h2 > h1,
        "larger rect should have larger height: h2={h2} vs h1={h1}"
    );
}

// =========================================================================
// Non-overlap Tests (Plan 0028, Task 2.1)
// =========================================================================

#[test]
fn stacked_subgraphs_do_not_overlap() {
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = "graph TD\n\
        subgraph sg1[Input]\nA[Data]\nB[Config]\nend\n\
        subgraph sg2[Output]\nC[Result]\nD[Log]\nend\n\
        A --> C\nB --> D";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    let sg1 = &layout.subgraph_bounds["sg1"];
    let sg2 = &layout.subgraph_bounds["sg2"];

    let sg1_bottom = sg1.y + sg1.height;
    let sg2_bottom = sg2.y + sg2.height;

    // Determine which is "upper" and which is "lower"
    let (_upper, lower, upper_bottom) = if sg1.y < sg2.y {
        (sg1, sg2, sg1_bottom)
    } else {
        (sg2, sg1, sg2_bottom)
    };

    // Upper subgraph's bottom must be strictly above lower's top
    assert!(
        upper_bottom <= lower.y,
        "Subgraphs should not overlap vertically: upper bottom={upper_bottom}, lower top={}",
        lower.y
    );
}

// =========================================================================
// Containment Tests (Plan 0028, Task 1.2)
// =========================================================================

#[test]
fn subgraph_bounds_contain_member_node_bounds() {
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = "graph TD\nsubgraph sg1[Group]\nA[Node1]\nB[Node2]\nend\nA --> B";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    assert_subgraph_contains_members(&layout, "sg1", &["A", "B"]);
}

#[test]
fn stacked_subgraph_bounds_contain_member_nodes_after_overlap_resolution() {
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = "graph TD\n\
        subgraph sg1[Input]\nA[Data]\nB[Config]\nend\n\
        subgraph sg2[Output]\nC[Result]\nD[Log]\nend\n\
        A --> C\nB --> D";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());

    assert_subgraph_contains_members(&layout, "sg1", &["A", "B"]);
    assert_subgraph_contains_members(&layout, "sg2", &["C", "D"]);
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

// =========================================================================
// Direction Override: Field Plumbing (Phase 4, Task 4.1)
// =========================================================================

#[test]
fn direction_override_field_available_at_layout() {
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = "graph TD\nsubgraph sg1[Group]\ndirection LR\nA --> B\nend\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);

    // Direction override is present on the subgraph
    assert_eq!(diagram.subgraphs["sg1"].dir, Some(Direction::LeftRight));

    // Layout computation succeeds without panic
    let config = GridLayoutConfig::default();
    let layout = compute_layout(&diagram, &config);
    assert!(!layout.node_bounds.is_empty());
}

#[test]
fn direction_override_none_when_not_specified() {
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = "graph TD\nsubgraph sg1[Group]\nA --> B\nend\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);

    // No direction override: field should be None
    assert_eq!(diagram.subgraphs["sg1"].dir, None);
}

// =========================================================================
// Direction Override Sub-Layout Tests (Phase 4, Tasks 4.2-4.4)
// =========================================================================

/// Helper: compute a sub-layout for a direction-override subgraph.
/// Returns the LayoutResult for just the subgraph's internal nodes/edges.
fn run_sublayout_for_sg(diagram: &Diagram, sg_id: &str) -> layered::LayoutResult {
    let sg = &diagram.subgraphs[sg_id];
    let sub_dir = sg.dir.expect("subgraph should have direction override");

    let layered_direction = match sub_dir {
        Direction::TopDown => LayeredDirection::TopBottom,
        Direction::BottomTop => LayeredDirection::BottomTop,
        Direction::LeftRight => LayeredDirection::LeftRight,
        Direction::RightLeft => LayeredDirection::RightLeft,
    };

    let mut sub_graph: layered::DiGraph<(f64, f64)> = layered::DiGraph::new();

    // Add leaf nodes (not child subgraphs)
    for node_id in &sg.nodes {
        if !diagram.is_subgraph(node_id)
            && let Some(node) = diagram.nodes.get(node_id)
        {
            let (w, h) = grid_node_dimensions(node, sub_dir);
            sub_graph.add_node(node_id.as_str(), (w as f64, h as f64));
        }
    }

    // Add internal edges
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

    layered::layout(&sub_graph, &sub_config, |_, dims| *dims)
}

#[test]
fn sublayout_lr_nodes_arranged_horizontally() {
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA[Step 1] --> B[Step 2] --> C[Step 3]\nend\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);

    let result = run_sublayout_for_sg(&diagram, "sg1");

    // In LR layout, nodes should be arranged horizontally (increasing x, similar y)
    let a = &result.nodes[&layered::NodeId::from("A")];
    let b = &result.nodes[&layered::NodeId::from("B")];
    let c = &result.nodes[&layered::NodeId::from("C")];

    // Centers should have increasing x
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

    // Centers should have similar y (within tolerance for same-rank nodes)
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
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA[Step 1] --> B[Step 2] --> C[Step 3]\nend\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);

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
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = "graph LR\nsubgraph sg1[Vertical]\ndirection BT\nA[Start] --> B[End]\nend\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);

    let result = run_sublayout_for_sg(&diagram, "sg1");

    let a = &result.nodes[&layered::NodeId::from("A")];
    let b = &result.nodes[&layered::NodeId::from("B")];

    // BT: A should be below B (higher y means lower on screen)
    let a_cy = a.y + a.height / 2.0;
    let b_cy = b.y + b.height / 2.0;

    assert!(
        a_cy > b_cy,
        "In BT layout, A (start) should be below B (end): A_cy={a_cy} B_cy={b_cy}"
    );
}

#[test]
fn sublayout_rl_reverses_node_order() {
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = "graph TD\nsubgraph sg1[Reverse]\ndirection RL\nA[Left] --> B[Right]\nend\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    let config = GridLayoutConfig::default();
    let layout = compute_layout(&diagram, &config);

    let a = layout.get_bounds("A").unwrap();
    let b = layout.get_bounds("B").unwrap();

    // RL: A (start) should be RIGHT of B (end) since flow goes right-to-left
    assert!(
        a.center_x() > b.center_x(),
        "In RL layout, A should be right of B: A_cx={} B_cx={}",
        a.center_x(),
        b.center_x()
    );

    // Both should be at similar y
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
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = "graph TD\nsubgraph sg1[Horizontal Section]\ndirection LR\nA[Step 1] --> B[Step 2] --> C[Step 3]\nend\nStart --> A\nC --> End\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    let config = GridLayoutConfig::default();
    let layout = compute_layout(&diagram, &config);

    let a = layout.get_bounds("A").unwrap();
    let b = layout.get_bounds("B").unwrap();
    let c = layout.get_bounds("C").unwrap();

    // In an LR subgraph within a TD parent:
    // A, B, C should be arranged horizontally (increasing x, similar y)
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

    // All should be at similar y (within a small tolerance for rounding)
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
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA[Step 1] --> B[Step 2] --> C[Step 3]\nend\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    let config = GridLayoutConfig::default();
    let layout = compute_layout(&diagram, &config);

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
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    // BT subgraph inside an LR parent: subgraph should be taller than wide
    let input =
        "graph LR\nsubgraph sg1[Vertical]\ndirection BT\nA[Top] --> B[Mid] --> C[Bot]\nend\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    let config = GridLayoutConfig::default();
    let layout = compute_layout(&diagram, &config);

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
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    // Subgraph with a long title should have bounds wide enough for the title
    let input = "graph TD\nsubgraph sg1[A Very Long Section Title]\ndirection LR\nA --> B\nend\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    let config = GridLayoutConfig::default();
    let layout = compute_layout(&diagram, &config);

    let sg = &layout.subgraph_bounds["sg1"];
    let title = "A Very Long Section Title";
    // Title with padding characters on either side
    assert!(
        sg.width >= title.len(),
        "Subgraph width ({}) should accommodate title length ({})",
        sg.width,
        title.len()
    );
}

#[test]
fn direction_override_nodes_inside_subgraph_bounds() {
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA[Step 1] --> B[Step 2] --> C[Step 3]\nend\nStart --> A\nC --> End\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    let config = GridLayoutConfig::default();
    let layout = compute_layout(&diagram, &config);

    assert_subgraph_contains_members(&layout, "sg1", &["A", "B", "C"]);
}

#[test]
fn direction_override_no_node_overlap() {
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA[Step 1] --> B[Step 2] --> C[Step 3]\nend\nStart --> A\nC --> End\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    let config = GridLayoutConfig::default();
    let layout = compute_layout(&diagram, &config);

    // Verify no overlap between A, B, C
    let nodes = ["A", "B", "C"];
    for i in 0..nodes.len() {
        for j in (i + 1)..nodes.len() {
            let a = layout.get_bounds(nodes[i]).unwrap();
            let b = layout.get_bounds(nodes[j]).unwrap();
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
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA[Step 1] --> B[Step 2]\nend\nStart --> A\nB --> End\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    let config = GridLayoutConfig::default();
    let layout = compute_layout(&diagram, &config);

    let sg = &layout.subgraph_bounds["sg1"];

    // Start and End should NOT be inside the subgraph bounds
    // (they are external to sg1)
    for ext_node in &["Start", "End"] {
        let bounds = layout.get_bounds(ext_node).unwrap();
        let inside_x = bounds.x >= sg.x && bounds.x + bounds.width <= sg.x + sg.width;
        let inside_y = bounds.y >= sg.y && bounds.y + bounds.height <= sg.y + sg.height;
        // At least one dimension should be outside
        assert!(
            !(inside_x && inside_y),
            "External node {} should not be fully inside sg1 bounds",
            ext_node
        );
    }
}

// =========================================================================
// Cross-Boundary Edge Routing (Phase 4, Task 4.5)
// =========================================================================

#[test]
fn cross_boundary_edge_no_panic() {
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input =
        "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA --> B\nend\nC --> A\nB --> D\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    let output = render_text_diagram(&diagram);
    assert!(output.contains("A"));
    assert!(output.contains("B"));
    assert!(output.contains("C"));
    assert!(output.contains("D"));
    assert!(output.contains("Horizontal"));
}

#[test]
fn node_effective_direction_populated() {
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input = "graph TD\nsubgraph sg1[Group]\ndirection LR\nA --> B\nend\nC --> A\nB --> D\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    let config = GridLayoutConfig::default();
    let layout = compute_layout(&diagram, &config);

    // Nodes inside the LR subgraph should have LR effective direction
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

    // Nodes outside the subgraph should have the parent direction (TD)
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
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    let input =
        "graph TD\nsubgraph sg1[Group]\ndirection LR\nA --> B\nend\nStart --> A\nB --> End\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);

    let result = run_sublayout_for_sg(&diagram, "sg1");

    // Sub-layout should only have A and B, not Start or End
    assert!(result.nodes.contains_key(&layered::NodeId::from("A")));
    assert!(result.nodes.contains_key(&layered::NodeId::from("B")));
    assert!(!result.nodes.contains_key(&layered::NodeId::from("Start")));
    assert!(!result.nodes.contains_key(&layered::NodeId::from("End")));
}

#[test]
fn compute_sublayouts_skips_non_isolated_when_flag_set() {
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

    // sg1 has direction LR but cross-boundary edge C --> A
    let input = "graph TD\nsubgraph sg1[Group]\ndirection LR\nA --> B\nend\nC --> A";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    let layered_config = LayeredConfig::default(); // direction = TopBottom

    // With flag false: sublayout uses override direction (LR)
    let subs_false = compute_sublayouts(
        &diagram,
        &layered_config,
        |_node| (40.0, 20.0),
        |_edge| None,
        false,
    );
    let lr_result = &subs_false["sg1"];
    let a_lr = lr_result.result.nodes[&layered::NodeId::from("A")];
    let b_lr = lr_result.result.nodes[&layered::NodeId::from("B")];
    // LR: A and B should be side-by-side (different x, similar y)
    assert!(
        (a_lr.y - b_lr.y).abs() < 1.0,
        "LR: A.y={} B.y={} should be similar",
        a_lr.y,
        b_lr.y
    );

    // With flag true: non-isolated override is skipped entirely.
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
