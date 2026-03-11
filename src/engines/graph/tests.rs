use super::algorithms::layered::{
    LabelDummyStrategy, LayoutConfig, MeasurementMode, run_layered_layout,
};
use super::flux::{
    FluxLayeredEngine, adapt_flux_profile_for_reversed_chain_crowding, flux_layout_profile,
};
use super::mermaid::MermaidLayeredEngine;
use super::{
    EdgeRouting, EngineAlgorithmId, EngineConfig, GeometryLevel, GraphEngine, GraphSolveRequest,
    GraphSolveResult, OutputFormat, PathSimplification, RoutingStyle,
};
use crate::graph::Diagram;
use crate::render::graph::svg_metrics::SvgTextMetrics;

#[test]
fn run_layered_layout_simple_graph() {
    let input = "graph TD\nA-->B";
    let flowchart = crate::frontends::mermaid::parse_flowchart(input).unwrap();
    let diagram = crate::diagrams::flowchart::compile_to_graph(&flowchart);

    let config = EngineConfig::Layered(LayoutConfig::default());
    let geom = run_layered_layout(&MeasurementMode::Text, &diagram, &config).unwrap();

    assert_eq!(geom.nodes.len(), 2);
    assert!(geom.nodes.contains_key("A"));
    assert!(geom.nodes.contains_key("B"));
    assert_eq!(geom.edges.len(), 1);
}

#[test]
fn run_layered_layout_with_subgraphs() {
    let input = "graph TD\nsubgraph sg1[Group]\nA-->B\nend\nC-->A";
    let flowchart = crate::frontends::mermaid::parse_flowchart(input).unwrap();
    let diagram = crate::diagrams::flowchart::compile_to_graph(&flowchart);

    let config = EngineConfig::Layered(LayoutConfig::default());
    let geom = run_layered_layout(&MeasurementMode::Text, &diagram, &config).unwrap();

    assert!(geom.nodes.contains_key("A"));
    assert!(geom.nodes.contains_key("B"));
    assert!(geom.nodes.contains_key("C"));
    assert!(!geom.subgraphs.is_empty());
}

#[test]
fn run_layered_layout_svg_mode_produces_larger_dimensions() {
    let input = "graph TD\nA-->B";
    let flowchart = crate::frontends::mermaid::parse_flowchart(input).unwrap();
    let diagram = crate::diagrams::flowchart::compile_to_graph(&flowchart);

    let config = EngineConfig::Layered(LayoutConfig::default());
    let text_geom = run_layered_layout(&MeasurementMode::Text, &diagram, &config).unwrap();
    let svg_geom = run_layered_layout(
        &MeasurementMode::Svg(SvgTextMetrics::new(16.0, 15.0, 15.0)),
        &diagram,
        &config,
    )
    .unwrap();

    let text_w = text_geom.nodes["A"].rect.width;
    let svg_w = svg_geom.nodes["A"].rect.width;
    assert!(
        svg_w > text_w * 3.0,
        "SVG width ({svg_w}) should be much larger than text width ({text_w})"
    );
}

#[test]
fn run_layered_layout_applies_subgraph_centering_and_expansion() {
    let input = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/flowchart/direction_override.mmd"
    ))
    .unwrap();
    let flowchart = crate::frontends::mermaid::parse_flowchart(&input).unwrap();
    let diagram = crate::diagrams::flowchart::compile_to_graph(&flowchart);

    let config = EngineConfig::Layered(LayoutConfig::default());
    let geom = run_layered_layout(&MeasurementMode::Text, &diagram, &config).unwrap();

    let sg_bounds = geom.subgraphs.get("sg1").expect("sg1 should exist");
    for member in &["A", "B", "C"] {
        let node = geom
            .nodes
            .get(*member)
            .unwrap_or_else(|| panic!("{member} missing"));
        let nr = &node.rect;
        let sr = &sg_bounds.rect;
        assert!(
            nr.x >= sr.x
                && nr.x + nr.width <= sr.x + sr.width
                && nr.y >= sr.y
                && nr.y + nr.height <= sr.y + sr.height,
            "Node {member} at {:?} should be within sg1 bounds {:?}",
            nr,
            sr,
        );
    }

    let start = geom.nodes.get("Start").expect("Start should exist");
    let sg_center_x = sg_bounds.rect.x + sg_bounds.rect.width / 2.0;
    let start_center_x = start.rect.x + start.rect.width / 2.0;
    assert!(
        (start_center_x - sg_center_x).abs() < sg_bounds.rect.width * 0.4,
        "Start center ({start_center_x}) should be near sg1 center ({sg_center_x})"
    );
}

#[test]
fn selected_engine_rejects_unknown_engine_at_parse_boundary() {
    let err = EngineAlgorithmId::parse("nonexistent").unwrap_err();
    assert!(
        err.message.contains("unknown engine"),
        "error should mention unknown: {}",
        err.message
    );
}

#[test]
fn flux_layout_profile_polyline_uses_enhanced_profile() {
    let input_cfg = LayoutConfig {
        greedy_switch: false,
        model_order_tiebreak: true,
        variable_rank_spacing: false,
        always_compound_ordering: false,
        track_reversed_chains: false,
        per_edge_label_spacing: false,
        label_side_selection: false,
        label_dummy_strategy: LabelDummyStrategy::Midpoint,
        ..Default::default()
    };
    let profile = flux_layout_profile(&input_cfg, EdgeRouting::PolylineRoute);

    assert!(
        profile.greedy_switch,
        "polyline profile should enable greedy_switch"
    );
    assert_eq!(
        profile.model_order_tiebreak, input_cfg.model_order_tiebreak,
        "polyline profile should preserve model_order_tiebreak from input config"
    );
    assert!(
        profile.variable_rank_spacing,
        "polyline profile should enable variable_rank_spacing"
    );
    assert!(
        profile.track_reversed_chains,
        "polyline profile should enable track_reversed_chains by default"
    );
    assert!(
        profile.per_edge_label_spacing,
        "polyline profile should enable per_edge_label_spacing"
    );
    assert!(
        profile.label_side_selection,
        "polyline profile should enable label_side_selection"
    );
    assert_eq!(
        profile.label_dummy_strategy,
        LabelDummyStrategy::WidestLayer,
        "polyline profile should use widest-layer label dummy placement"
    );
    assert!(
        profile.always_compound_ordering,
        "polyline profile should always use compound ordering sweeps"
    );
}

#[test]
fn flux_layout_profile_all_routing_styles_use_enhanced_profile() {
    let input_cfg = LayoutConfig {
        greedy_switch: false,
        model_order_tiebreak: true,
        variable_rank_spacing: false,
        always_compound_ordering: false,
        track_reversed_chains: false,
        per_edge_label_spacing: false,
        label_side_selection: false,
        label_dummy_strategy: LabelDummyStrategy::Midpoint,
        ..Default::default()
    };

    for routing in [
        EdgeRouting::DirectRoute,
        EdgeRouting::OrthogonalRoute,
        EdgeRouting::PolylineRoute,
    ] {
        let profile = flux_layout_profile(&input_cfg, routing);
        assert!(
            profile.greedy_switch,
            "{routing:?} profile should enable greedy_switch"
        );
        assert_eq!(
            profile.model_order_tiebreak, input_cfg.model_order_tiebreak,
            "{routing:?} profile should preserve model_order_tiebreak from input config"
        );
        assert!(
            profile.variable_rank_spacing,
            "{routing:?} profile should enable variable_rank_spacing"
        );
        assert!(
            profile.track_reversed_chains,
            "{routing:?} profile should enable track_reversed_chains"
        );
        assert!(
            profile.per_edge_label_spacing,
            "{routing:?} profile should enable per_edge_label_spacing"
        );
        assert!(
            profile.label_side_selection,
            "{routing:?} profile should enable label_side_selection"
        );
        assert_eq!(
            profile.label_dummy_strategy,
            LabelDummyStrategy::WidestLayer,
            "{routing:?} profile should use widest-layer label dummy placement"
        );
        assert!(
            profile.always_compound_ordering,
            "{routing:?} profile should always use compound ordering sweeps"
        );
    }
}

#[test]
fn adaptive_reversed_chain_policy_relaxes_for_inline_label_crowding() {
    let input = include_str!("../../../tests/fixtures/flowchart/inline_label_flowchart.mmd");
    let flowchart =
        crate::frontends::mermaid::parse_flowchart(input).expect("fixture should parse");
    let diagram = crate::diagrams::flowchart::compile_to_graph(&flowchart);
    let mode = MeasurementMode::Svg(SvgTextMetrics::new(16.0, 15.0, 15.0));

    let input_cfg = LayoutConfig {
        model_order_tiebreak: true,
        ..Default::default()
    };

    for routing in [
        EdgeRouting::DirectRoute,
        EdgeRouting::PolylineRoute,
        EdgeRouting::OrthogonalRoute,
    ] {
        let profile = flux_layout_profile(&input_cfg, routing);
        assert!(
            profile.track_reversed_chains,
            "{routing:?} profile should start with reversed-chain tracking enabled"
        );

        let adapted =
            adapt_flux_profile_for_reversed_chain_crowding(&mode, &diagram, routing, &profile)
                .expect("adaptive profile should succeed");

        assert!(
            !adapted.track_reversed_chains,
            "{routing:?} should relax reversed-chain tracking for inline_label_flowchart crowding"
        );
    }
}

#[test]
fn adaptive_reversed_chain_policy_preserves_crossing_minimize_ordering() {
    let input = include_str!("../../../tests/fixtures/flowchart/crossing_minimize.mmd");
    let flowchart =
        crate::frontends::mermaid::parse_flowchart(input).expect("fixture should parse");
    let diagram = crate::diagrams::flowchart::compile_to_graph(&flowchart);
    let mode = MeasurementMode::Svg(SvgTextMetrics::new(16.0, 15.0, 15.0));

    let input_cfg = LayoutConfig {
        model_order_tiebreak: true,
        ..Default::default()
    };

    for routing in [
        EdgeRouting::DirectRoute,
        EdgeRouting::PolylineRoute,
        EdgeRouting::OrthogonalRoute,
    ] {
        let profile = flux_layout_profile(&input_cfg, routing);
        let adapted =
            adapt_flux_profile_for_reversed_chain_crowding(&mode, &diagram, routing, &profile)
                .expect("adaptive profile should succeed");

        assert!(
            adapted.track_reversed_chains,
            "{routing:?} should keep reversed-chain tracking on crossing_minimize"
        );
    }
}

fn solve_svg(engine: &dyn GraphEngine, diagram: &Diagram) -> GraphSolveResult {
    let config = EngineConfig::Layered(LayoutConfig::default());
    let request = GraphSolveRequest {
        output_format: OutputFormat::Svg,
        geometry_level: GeometryLevel::Layout,
        path_simplification: PathSimplification::None,
        routing_style: Some(RoutingStyle::Polyline),
    };
    engine.solve(diagram, &config, &request).unwrap()
}

fn solve_mmds_layout(engine: &dyn GraphEngine, diagram: &Diagram) -> GraphSolveResult {
    let config = EngineConfig::Layered(LayoutConfig::default());
    let request = GraphSolveRequest {
        output_format: OutputFormat::Mmds,
        geometry_level: GeometryLevel::Layout,
        path_simplification: PathSimplification::None,
        routing_style: Some(RoutingStyle::Polyline),
    };
    engine.solve(diagram, &config, &request).unwrap()
}

#[test]
fn subgraph_direction_isolated_both_engines_respect_override() {
    let input = include_str!("../../../tests/fixtures/flowchart/subgraph_direction_isolated.mmd");
    let flowchart = crate::frontends::mermaid::parse_flowchart(input).unwrap();
    let diagram = crate::diagrams::flowchart::compile_to_graph(&flowchart);
    let metrics = SvgTextMetrics::new(16.0, 15.0, 15.0);

    let flux = FluxLayeredEngine::with_mode(MeasurementMode::Svg(metrics.clone()));
    let flux_result = solve_svg(&flux, &diagram);
    let a_flux = &flux_result.geometry.nodes["A"].rect;
    let b_flux = &flux_result.geometry.nodes["B"].rect;
    assert!(
        (a_flux.y - b_flux.y).abs() < 1.0,
        "flux: A.y={} B.y={} should be similar (LR override)",
        a_flux.y,
        b_flux.y
    );
    assert!(
        (a_flux.x - b_flux.x).abs() > 10.0,
        "flux: A.x={} B.x={} should differ (LR override)",
        a_flux.x,
        b_flux.x
    );

    let mermaid = MermaidLayeredEngine::with_mode(MeasurementMode::Svg(metrics));
    let mermaid_result = solve_svg(&mermaid, &diagram);
    let a_mermaid = &mermaid_result.geometry.nodes["A"].rect;
    let b_mermaid = &mermaid_result.geometry.nodes["B"].rect;
    assert!(
        (a_mermaid.y - b_mermaid.y).abs() < 1.0,
        "mermaid: A.y={} B.y={} should be similar (LR override respected for isolated sg)",
        a_mermaid.y,
        b_mermaid.y
    );
    assert!(
        (a_mermaid.x - b_mermaid.x).abs() > 10.0,
        "mermaid: A.x={} B.x={} should differ (LR override respected for isolated sg)",
        a_mermaid.x,
        b_mermaid.x
    );
}

#[test]
fn subgraph_direction_cross_boundary_engines_diverge() {
    let input =
        include_str!("../../../tests/fixtures/flowchart/subgraph_direction_cross_boundary.mmd");
    let flowchart = crate::frontends::mermaid::parse_flowchart(input).unwrap();
    let diagram = crate::diagrams::flowchart::compile_to_graph(&flowchart);
    let metrics = SvgTextMetrics::new(16.0, 15.0, 15.0);

    let flux = FluxLayeredEngine::with_mode(MeasurementMode::Svg(metrics.clone()));
    let flux_result = solve_svg(&flux, &diagram);
    let a_flux = &flux_result.geometry.nodes["A"].rect;
    let b_flux = &flux_result.geometry.nodes["B"].rect;
    let flux_x_spread = (a_flux.x - b_flux.x).abs();
    assert!(
        (a_flux.y - b_flux.y).abs() < 10.0,
        "flux: A.y={} B.y={} should be similar (LR sublayout applied)",
        a_flux.y,
        b_flux.y
    );
    assert!(
        flux_x_spread > 10.0,
        "flux: A-B X spread={flux_x_spread} should be large (LR sublayout)",
    );

    let mermaid = MermaidLayeredEngine::with_mode(MeasurementMode::Svg(metrics));
    let mermaid_result = solve_svg(&mermaid, &diagram);
    let a_mermaid = &mermaid_result.geometry.nodes["A"].rect;
    let b_mermaid = &mermaid_result.geometry.nodes["B"].rect;
    assert!(
        (a_mermaid.y - b_mermaid.y).abs() > 10.0,
        "mermaid: A.y={} B.y={} should differ (TD sublayout, LR override ignored)",
        a_mermaid.y,
        b_mermaid.y
    );
}

#[test]
fn subgraph_direction_nested_mixed_isolation() {
    let input =
        include_str!("../../../tests/fixtures/flowchart/subgraph_direction_nested_mixed.mmd");
    let flowchart = crate::frontends::mermaid::parse_flowchart(input).unwrap();
    let diagram = crate::diagrams::flowchart::compile_to_graph(&flowchart);
    let metrics = SvgTextMetrics::new(16.0, 15.0, 15.0);

    let mermaid = MermaidLayeredEngine::with_mode(MeasurementMode::Svg(metrics.clone()));
    let mermaid_result = solve_svg(&mermaid, &diagram);
    let a_mermaid = &mermaid_result.geometry.nodes["A"].rect;
    let b_mermaid = &mermaid_result.geometry.nodes["B"].rect;
    assert!(
        b_mermaid.y < a_mermaid.y,
        "mermaid: B.y={} should be less than A.y={} (BT override respected for isolated inner)",
        b_mermaid.y,
        a_mermaid.y
    );

    let flux = FluxLayeredEngine::with_mode(MeasurementMode::Svg(metrics));
    let flux_result = solve_svg(&flux, &diagram);
    let a_flux = &flux_result.geometry.nodes["A"].rect;
    let b_flux = &flux_result.geometry.nodes["B"].rect;
    assert!(
        b_flux.y < a_flux.y,
        "flux: B.y={} should be less than A.y={} (BT override respected)",
        b_flux.y,
        a_flux.y
    );
}

#[test]
fn mermaid_non_isolated_override_matches_parent_flow_in_svg_and_mmds() {
    let input = include_str!("../../../tests/fixtures/flowchart/direction_override.mmd");
    let flowchart = crate::frontends::mermaid::parse_flowchart(input).unwrap();
    let diagram = crate::diagrams::flowchart::compile_to_graph(&flowchart);
    let metrics = SvgTextMetrics::new(16.0, 15.0, 15.0);
    let mermaid = MermaidLayeredEngine::with_mode(MeasurementMode::Svg(metrics));

    let svg_result = solve_svg(&mermaid, &diagram);
    let start = svg_result.geometry.nodes["Start"].rect;
    let sg = svg_result.geometry.subgraphs["sg1"].rect;
    assert!(
        start.y + start.height <= sg.y + 0.001,
        "mermaid svg: Start should be above sg1 (no overlap): start_bottom={} sg_top={}",
        start.y + start.height,
        sg.y
    );

    let a_svg = svg_result.geometry.nodes["A"].rect;
    let b_svg = svg_result.geometry.nodes["B"].rect;
    let c_svg = svg_result.geometry.nodes["C"].rect;
    assert!(
        a_svg.y < b_svg.y && b_svg.y < c_svg.y,
        "mermaid svg: A/B/C should stack vertically when non-isolated override is ignored: A.y={} B.y={} C.y={}",
        a_svg.y,
        b_svg.y,
        c_svg.y
    );

    let mmds_result = solve_mmds_layout(&mermaid, &diagram);
    let a_mmds = mmds_result.geometry.nodes["A"].rect;
    let b_mmds = mmds_result.geometry.nodes["B"].rect;
    let c_mmds = mmds_result.geometry.nodes["C"].rect;
    assert!(
        a_mmds.y < b_mmds.y && b_mmds.y < c_mmds.y,
        "mermaid mmds: A/B/C should stack vertically when non-isolated override is ignored: A.y={} B.y={} C.y={}",
        a_mmds.y,
        b_mmds.y,
        c_mmds.y
    );
}

#[test]
fn mermaid_default_direction_matches_nested_with_siblings_fixture() {
    let input = include_str!("../../../tests/fixtures/flowchart/nested_with_siblings.mmd");
    let flowchart = crate::frontends::mermaid::parse_flowchart(input).unwrap();
    let diagram = crate::diagrams::flowchart::compile_to_graph(&flowchart);
    let metrics = SvgTextMetrics::new(16.0, 15.0, 15.0);
    let mermaid = MermaidLayeredEngine::with_mode(MeasurementMode::Svg(metrics));

    for (label, result) in [
        ("svg", solve_svg(&mermaid, &diagram)),
        ("mmds", solve_mmds_layout(&mermaid, &diagram)),
    ] {
        let a = result.geometry.nodes["A"].rect;
        let b = result.geometry.nodes["B"].rect;
        let c = result.geometry.nodes["C"].rect;
        let d = result.geometry.nodes["D"].rect;

        assert!(
            (a.x - b.x).abs() < 1.0 && (c.x - d.x).abs() < 1.0,
            "mermaid {label} nested_with_siblings: sibling subgraphs should stack A->B and C->D vertically (x aligned): A.x={} B.x={} C.x={} D.x={}",
            a.x,
            b.x,
            c.x,
            d.x
        );
        assert!(
            a.y < b.y && b.y < c.y && c.y < d.y,
            "mermaid {label} nested_with_siblings: expected vertical order A < B < C < D; got A.y={} B.y={} C.y={} D.y={}",
            a.y,
            b.y,
            c.y,
            d.y
        );
    }
}

#[test]
fn mermaid_subgraph_as_node_edge_uses_isolated_default_direction() {
    let input = include_str!("../../../tests/fixtures/flowchart/subgraph_as_node_edge.mmd");
    let flowchart = crate::frontends::mermaid::parse_flowchart(input).unwrap();
    let diagram = crate::diagrams::flowchart::compile_to_graph(&flowchart);
    let metrics = SvgTextMetrics::new(16.0, 15.0, 15.0);
    let mermaid = MermaidLayeredEngine::with_mode(MeasurementMode::Svg(metrics.clone()));
    let svg_result = solve_svg(&mermaid, &diagram);

    let api_svg = svg_result.geometry.nodes["API"].rect;
    let db_svg = svg_result.geometry.nodes["DB"].rect;
    assert!(
        (api_svg.y - db_svg.y).abs() < 1.0 && (api_svg.x - db_svg.x).abs() > 10.0,
        "mermaid svg subgraph_as_node_edge: API and DB should be side-by-side (isolated default dir): API=({}, {}), DB=({}, {})",
        api_svg.x,
        api_svg.y,
        db_svg.x,
        db_svg.y
    );

    let mmds_result = solve_mmds_layout(&mermaid, &diagram);
    let api_mmds = mmds_result.geometry.nodes["API"].rect;
    let db_mmds = mmds_result.geometry.nodes["DB"].rect;
    assert!(
        (api_mmds.y - db_mmds.y).abs() < 1.0 && (api_mmds.x - db_mmds.x).abs() > 10.0,
        "mermaid mmds subgraph_as_node_edge: API and DB should be side-by-side (isolated default dir): API=({}, {}), DB=({}, {})",
        api_mmds.x,
        api_mmds.y,
        db_mmds.x,
        db_mmds.y
    );
}

#[test]
fn mermaid_mmds_keeps_isolated_direction_override_layouted() {
    let input = include_str!("../../../tests/fixtures/flowchart/subgraph_direction_isolated.mmd");
    let flowchart = crate::frontends::mermaid::parse_flowchart(input).unwrap();
    let diagram = crate::diagrams::flowchart::compile_to_graph(&flowchart);
    let metrics = SvgTextMetrics::new(16.0, 15.0, 15.0);
    let mermaid = MermaidLayeredEngine::with_mode(MeasurementMode::Svg(metrics));
    let mmds_result = solve_mmds_layout(&mermaid, &diagram);

    let a = mmds_result.geometry.nodes["A"].rect;
    let b = mmds_result.geometry.nodes["B"].rect;
    let c = mmds_result.geometry.nodes["C"].rect;
    assert!(
        (a.y - b.y).abs() < 1.0 && (b.y - c.y).abs() < 1.0,
        "mermaid mmds subgraph_direction_isolated: A/B/C should share row in LR override; A.y={} B.y={} C.y={}",
        a.y,
        b.y,
        c.y
    );
    assert!(
        a.x < b.x && b.x < c.x,
        "mermaid mmds subgraph_direction_isolated: A/B/C should be ordered left-to-right; A.x={} B.x={} C.x={}",
        a.x,
        b.x,
        c.x
    );
}

#[test]
fn mermaid_nested_subgraph_bounds_are_compact_after_policy_normalization() {
    let input = include_str!("../../../tests/fixtures/flowchart/nested_subgraph.mmd");
    let flowchart = crate::frontends::mermaid::parse_flowchart(input).unwrap();
    let diagram = crate::diagrams::flowchart::compile_to_graph(&flowchart);
    let metrics = SvgTextMetrics::new(16.0, 15.0, 15.0);
    let mermaid = MermaidLayeredEngine::with_mode(MeasurementMode::Svg(metrics));
    let result = solve_svg(&mermaid, &diagram);

    let outer = result.geometry.subgraphs["outer"].rect;
    let inner = result.geometry.subgraphs["inner"].rect;
    assert!(
        inner.height < 160.0,
        "mermaid nested_subgraph: inner height should stay compact; got {}",
        inner.height
    );
    assert!(
        outer.height < 220.0,
        "mermaid nested_subgraph: outer height should stay compact; got {}",
        outer.height
    );
}

#[test]
fn mermaid_multi_subgraph_direction_override_bottom_cluster_is_compact_and_centered() {
    let input =
        include_str!("../../../tests/fixtures/flowchart/multi_subgraph_direction_override.mmd");
    let flowchart = crate::frontends::mermaid::parse_flowchart(input).unwrap();
    let diagram = crate::diagrams::flowchart::compile_to_graph(&flowchart);
    let metrics = SvgTextMetrics::new(16.0, 15.0, 15.0);
    let mermaid = MermaidLayeredEngine::with_mode(MeasurementMode::Svg(metrics));
    let result = solve_svg(&mermaid, &diagram);

    let g = result.geometry.subgraphs["G"].rect;
    let e = result.geometry.nodes["E"].rect;
    let f = result.geometry.nodes["F"].rect;
    let g_center_x = g.x + g.width / 2.0;
    let feed_center_x = ((e.x + e.width / 2.0) + (f.x + f.width / 2.0)) / 2.0;

    assert!(
        g.height < 180.0,
        "mermaid multi_subgraph_direction_override: G height should be compact; got {}",
        g.height
    );
    assert!(
        g.y > e.y,
        "mermaid multi_subgraph_direction_override: G should be below middle tier; G.y={} E.y={}",
        g.y,
        e.y
    );
    assert!(
        (g_center_x - feed_center_x).abs() < 120.0,
        "mermaid multi_subgraph_direction_override: G should stay centered under incoming feeds; G.cx={} feeds.cx={}",
        g_center_x,
        feed_center_x
    );
}

#[test]
fn mermaid_nested_subgraph_edge_keeps_compact_subgraph_to_node_gap() {
    let input = include_str!("../../../tests/fixtures/flowchart/nested_subgraph_edge.mmd");
    let flowchart = crate::frontends::mermaid::parse_flowchart(input).unwrap();
    let diagram = crate::diagrams::flowchart::compile_to_graph(&flowchart);
    let metrics = SvgTextMetrics::new(16.0, 15.0, 15.0);
    let mermaid = MermaidLayeredEngine::with_mode(MeasurementMode::Svg(metrics));
    let result = solve_svg(&mermaid, &diagram);

    let cloud = result.geometry.subgraphs["cloud"].rect;
    let monitoring = result.geometry.nodes["Monitoring"].rect;
    let gap = monitoring.y - (cloud.y + cloud.height);

    assert!(
        gap > 8.0,
        "mermaid nested_subgraph_edge: subgraph->node gap should remain visible; got {}",
        gap
    );
    assert!(
        gap < 90.0,
        "mermaid nested_subgraph_edge: subgraph->node gap should stay compact; got {}",
        gap
    );
}

#[test]
fn flux_layered_uses_per_edge_label_spacing() {
    let input = "graph TD\nA -->|yes| B --> C";
    let flowchart = crate::frontends::mermaid::parse_flowchart(input).unwrap();
    let diagram = crate::diagrams::flowchart::compile_to_graph(&flowchart);
    let mode = MeasurementMode::Text;

    let config_per_edge = EngineConfig::Layered(LayoutConfig {
        per_edge_label_spacing: true,
        ..Default::default()
    });
    let geom_per_edge = run_layered_layout(&mode, &diagram, &config_per_edge).unwrap();

    let config_global = EngineConfig::Layered(LayoutConfig::default());
    let geom_global = run_layered_layout(&mode, &diagram, &config_global).unwrap();

    let bc_edge_per_edge = geom_per_edge
        .edges
        .iter()
        .find(|e| e.from == "B" && e.to == "C")
        .expect("B->C edge in per-edge");
    let bc_edge_global = geom_global
        .edges
        .iter()
        .find(|e| e.from == "B" && e.to == "C")
        .expect("B->C edge in global");

    assert!(
        bc_edge_per_edge.waypoints.len() < bc_edge_global.waypoints.len(),
        "per-edge B->C should have fewer waypoints ({}) than global ({})",
        bc_edge_per_edge.waypoints.len(),
        bc_edge_global.waypoints.len()
    );
}
