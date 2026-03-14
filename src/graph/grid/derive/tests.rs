use super::*;
use crate::graph::grid::GridLayoutConfig;
use crate::graph::space::FPoint;

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
// Direction Override: Field Plumbing (Phase 4, Task 4.1)
// =========================================================================

#[test]
fn direction_override_none_when_not_specified() {
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::mermaid::parse_flowchart;

    let input = "graph TD\nsubgraph sg1[Group]\nA --> B\nend\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);

    // No direction override: field should be None
    assert_eq!(diagram.subgraphs["sg1"].dir, None);
}
