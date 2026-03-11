//! Hidden compatibility surface for this repository's external tests.
//!
//! This keeps the in-repo test harnesses working while the supported public
//! API is trimmed ahead of `v2.0.0`.

/// Hidden engine compatibility namespace.
pub mod engines {
    /// Hidden graph-engine compatibility surface.
    pub mod graph {
        use crate::EngineAlgorithmId;
        use crate::config::{
            EngineAlgorithmCapabilities, GeometryLevel, LayoutConfig, PathSimplification,
            RenderConfig,
        };
        use crate::errors::RenderError;
        use crate::format::{OutputFormat, RoutingStyle};
        use crate::graph::Diagram;
        use crate::graph::geometry::{GraphGeometry, RoutedGraphGeometry};

        /// Test-only mirror of the internal edge-routing mode.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum EdgeRouting {
            DirectRoute,
            PolylineRoute,
            EngineProvided,
            OrthogonalRoute,
        }

        impl From<EdgeRouting> for crate::engines::graph::EdgeRouting {
            fn from(value: EdgeRouting) -> Self {
                match value {
                    EdgeRouting::DirectRoute => Self::DirectRoute,
                    EdgeRouting::PolylineRoute => Self::PolylineRoute,
                    EdgeRouting::EngineProvided => Self::EngineProvided,
                    EdgeRouting::OrthogonalRoute => Self::OrthogonalRoute,
                }
            }
        }

        impl From<crate::engines::graph::EdgeRouting> for EdgeRouting {
            fn from(value: crate::engines::graph::EdgeRouting) -> Self {
                match value {
                    crate::engines::graph::EdgeRouting::DirectRoute => Self::DirectRoute,
                    crate::engines::graph::EdgeRouting::PolylineRoute => Self::PolylineRoute,
                    crate::engines::graph::EdgeRouting::EngineProvided => Self::EngineProvided,
                    crate::engines::graph::EdgeRouting::OrthogonalRoute => Self::OrthogonalRoute,
                }
            }
        }

        /// Test-only mirror of the internal engine config envelope.
        #[derive(Debug, Clone)]
        #[non_exhaustive]
        pub enum EngineConfig {
            Layered(crate::engines::graph::algorithms::layered::LayoutConfig),
        }

        impl From<LayoutConfig> for EngineConfig {
            fn from(config: LayoutConfig) -> Self {
                Self::Layered(config.into())
            }
        }

        impl From<&EngineConfig> for crate::engines::graph::EngineConfig {
            fn from(value: &EngineConfig) -> Self {
                match value {
                    EngineConfig::Layered(config) => Self::Layered(config.clone()),
                }
            }
        }

        /// Test-only mirror of the internal solve request.
        #[derive(Debug, Clone)]
        pub struct GraphSolveRequest {
            pub output_format: OutputFormat,
            pub geometry_level: GeometryLevel,
            pub path_simplification: PathSimplification,
            pub routing_style: Option<RoutingStyle>,
        }

        impl GraphSolveRequest {
            pub fn from_config(config: &RenderConfig, output_format: OutputFormat) -> Self {
                crate::engines::graph::GraphSolveRequest::from_config(config, output_format).into()
            }
        }

        impl From<crate::engines::graph::GraphSolveRequest> for GraphSolveRequest {
            fn from(value: crate::engines::graph::GraphSolveRequest) -> Self {
                Self {
                    output_format: value.output_format,
                    geometry_level: value.geometry_level,
                    path_simplification: value.path_simplification,
                    routing_style: value.routing_style,
                }
            }
        }

        impl From<&GraphSolveRequest> for crate::engines::graph::GraphSolveRequest {
            fn from(value: &GraphSolveRequest) -> Self {
                Self {
                    output_format: value.output_format,
                    geometry_level: value.geometry_level,
                    path_simplification: value.path_simplification,
                    routing_style: value.routing_style,
                }
            }
        }

        /// Test-only mirror of the internal solve result.
        #[derive(Debug, Clone)]
        pub struct GraphSolveResult {
            pub engine_id: EngineAlgorithmId,
            pub geometry: GraphGeometry,
            pub routed: Option<RoutedGraphGeometry>,
        }

        impl From<crate::engines::graph::GraphSolveResult> for GraphSolveResult {
            fn from(value: crate::engines::graph::GraphSolveResult) -> Self {
                Self {
                    engine_id: value.engine_id,
                    geometry: value.geometry,
                    routed: value.routed,
                }
            }
        }

        impl From<&GraphSolveResult> for crate::engines::graph::GraphSolveResult {
            fn from(value: &GraphSolveResult) -> Self {
                Self {
                    engine_id: value.engine_id,
                    geometry: value.geometry.clone(),
                    routed: value.routed.clone(),
                }
            }
        }

        /// Hidden trait wrapper for concrete graph engines used in tests.
        pub trait GraphEngine {
            fn id(&self) -> EngineAlgorithmId;
            fn capabilities(&self) -> EngineAlgorithmCapabilities;
            fn solve(
                &self,
                diagram: &Diagram,
                config: &EngineConfig,
                request: &GraphSolveRequest,
            ) -> Result<GraphSolveResult, RenderError>;
        }

        impl GraphEngine for crate::engines::graph::flux::FluxLayeredEngine {
            fn id(&self) -> EngineAlgorithmId {
                crate::engines::graph::GraphEngine::id(self)
            }

            fn capabilities(&self) -> EngineAlgorithmCapabilities {
                crate::engines::graph::GraphEngine::capabilities(self)
            }

            fn solve(
                &self,
                diagram: &Diagram,
                config: &EngineConfig,
                request: &GraphSolveRequest,
            ) -> Result<GraphSolveResult, RenderError> {
                let internal_config: crate::engines::graph::EngineConfig = config.into();
                let internal_request: crate::engines::graph::GraphSolveRequest = request.into();
                crate::engines::graph::GraphEngine::solve(
                    self,
                    diagram,
                    &internal_config,
                    &internal_request,
                )
                .map(Into::into)
            }
        }

        impl GraphEngine for crate::engines::graph::mermaid::MermaidLayeredEngine {
            fn id(&self) -> EngineAlgorithmId {
                crate::engines::graph::GraphEngine::id(self)
            }

            fn capabilities(&self) -> EngineAlgorithmCapabilities {
                crate::engines::graph::GraphEngine::capabilities(self)
            }

            fn solve(
                &self,
                diagram: &Diagram,
                config: &EngineConfig,
                request: &GraphSolveRequest,
            ) -> Result<GraphSolveResult, RenderError> {
                let internal_config: crate::engines::graph::EngineConfig = config.into();
                let internal_request: crate::engines::graph::GraphSolveRequest = request.into();
                crate::engines::graph::GraphEngine::solve(
                    self,
                    diagram,
                    &internal_config,
                    &internal_request,
                )
                .map(Into::into)
            }
        }

        /// Hidden engine handle used by the compatibility registry wrapper.
        pub struct GraphEngineRef<'a>(&'a dyn crate::engines::graph::GraphEngine);

        impl GraphEngineRef<'_> {
            pub fn id(&self) -> EngineAlgorithmId {
                self.0.id()
            }

            pub fn capabilities(&self) -> EngineAlgorithmCapabilities {
                self.0.capabilities()
            }

            pub fn solve(
                &self,
                diagram: &Diagram,
                config: &EngineConfig,
                request: &GraphSolveRequest,
            ) -> Result<GraphSolveResult, RenderError> {
                let internal_config: crate::engines::graph::EngineConfig = config.into();
                let internal_request: crate::engines::graph::GraphSolveRequest = request.into();
                self.0
                    .solve(diagram, &internal_config, &internal_request)
                    .map(Into::into)
            }
        }

        /// Hidden wrapper for the internal graph engine registry.
        #[derive(Default)]
        pub struct GraphEngineRegistry {
            inner: crate::engines::graph::GraphEngineRegistry,
        }

        impl GraphEngineRegistry {
            pub fn get_solver(&self, id: EngineAlgorithmId) -> Option<GraphEngineRef<'_>> {
                self.inner.get_solver(id).map(GraphEngineRef)
            }
        }
    }
}

/// Hidden render compatibility namespace.
pub mod render {
    /// Hidden graph-render compatibility surface.
    pub mod graph {
        use crate::config::{PathSimplification, TextColorMode};
        use crate::engines::graph::GraphEngine as _;
        use crate::engines::graph::flux::FluxLayeredEngine;
        use crate::format::{Curve, OutputFormat, RoutingStyle};
        use crate::graph::Diagram;
        use crate::render::graph::{SvgRenderOptions, TextRenderOptions};
        use crate::testing::engines::graph::EdgeRouting;

        /// Test-only mirror of the legacy SVG option bag.
        #[derive(Debug, Clone)]
        pub struct SvgOptions {
            pub scale: f64,
            pub font_family: String,
            pub font_size: f64,
            pub node_padding_x: f64,
            pub node_padding_y: f64,
            pub routing_style: RoutingStyle,
            pub curve: Curve,
            pub edge_radius: f64,
            pub diagram_padding: f64,
        }

        impl Default for SvgOptions {
            fn default() -> Self {
                let public = SvgRenderOptions::default();
                Self {
                    scale: public.scale,
                    font_family: public.font_family,
                    font_size: public.font_size,
                    node_padding_x: public.node_padding_x,
                    node_padding_y: public.node_padding_y,
                    routing_style: public.routing_style,
                    curve: public.curve,
                    edge_radius: public.edge_radius,
                    diagram_padding: public.diagram_padding,
                }
            }
        }

        /// Test-only mirror of the legacy graph render option bag.
        #[derive(Debug, Clone)]
        pub struct RenderOptions {
            pub output_format: OutputFormat,
            pub text_color_mode: TextColorMode,
            pub svg: SvgOptions,
            pub ranker: Option<crate::engines::graph::algorithms::layered::Ranker>,
            pub node_spacing: Option<f64>,
            pub rank_spacing: Option<f64>,
            pub edge_spacing: Option<f64>,
            pub margin: Option<f64>,
            pub cluster_ranksep: Option<f64>,
            pub padding: Option<usize>,
            pub path_simplification: PathSimplification,
            pub edge_routing: Option<EdgeRouting>,
        }

        impl Default for RenderOptions {
            fn default() -> Self {
                let public = TextRenderOptions::default();
                Self {
                    output_format: public.output_format,
                    text_color_mode: public.text_color_mode,
                    svg: SvgOptions::default(),
                    ranker: None,
                    node_spacing: None,
                    rank_spacing: None,
                    edge_spacing: None,
                    margin: None,
                    cluster_ranksep: public.cluster_ranksep,
                    padding: public.padding,
                    path_simplification: public.path_simplification,
                    edge_routing: None,
                }
            }
        }

        impl RenderOptions {
            pub fn default_svg() -> Self {
                Self {
                    output_format: OutputFormat::Svg,
                    ..Self::default()
                }
            }
        }

        fn internal_edge_routing(options: &RenderOptions) -> crate::engines::graph::EdgeRouting {
            options
                .edge_routing
                .map(Into::into)
                .unwrap_or(match options.svg.routing_style {
                    RoutingStyle::Direct => crate::engines::graph::EdgeRouting::DirectRoute,
                    RoutingStyle::Polyline => crate::engines::graph::EdgeRouting::PolylineRoute,
                    RoutingStyle::Orthogonal => crate::engines::graph::EdgeRouting::OrthogonalRoute,
                })
        }

        fn request_routing_style(options: &RenderOptions) -> Option<RoutingStyle> {
            options.edge_routing.map(|routing| match routing {
                EdgeRouting::DirectRoute => RoutingStyle::Direct,
                EdgeRouting::PolylineRoute => RoutingStyle::Polyline,
                EdgeRouting::OrthogonalRoute | EdgeRouting::EngineProvided => {
                    RoutingStyle::Orthogonal
                }
            })
        }

        fn engine_config_for_render(
            diagram: &Diagram,
            options: &RenderOptions,
        ) -> crate::engines::graph::EngineConfig {
            let mut config = crate::engines::graph::algorithms::layered::LayoutConfig {
                direction: match diagram.direction {
                    crate::Direction::TopDown => {
                        crate::engines::graph::algorithms::layered::Direction::TopBottom
                    }
                    crate::Direction::BottomTop => {
                        crate::engines::graph::algorithms::layered::Direction::BottomTop
                    }
                    crate::Direction::LeftRight => {
                        crate::engines::graph::algorithms::layered::Direction::LeftRight
                    }
                    crate::Direction::RightLeft => {
                        crate::engines::graph::algorithms::layered::Direction::RightLeft
                    }
                },
                ..Default::default()
            };

            if let Some(node_spacing) = options.node_spacing {
                config.node_sep = node_spacing;
            }
            if let Some(rank_spacing) = options.rank_spacing {
                config.rank_sep = rank_spacing;
            }
            if let Some(edge_spacing) = options.edge_spacing {
                config.edge_sep = edge_spacing;
            }
            if let Some(margin) = options.margin {
                config.margin = margin;
            }
            if let Some(ranker) = options.ranker {
                config.ranker = ranker;
            }

            crate::engines::graph::EngineConfig::Layered(config)
        }

        fn svg_render_options(options: &RenderOptions) -> SvgRenderOptions {
            SvgRenderOptions {
                scale: options.svg.scale,
                font_family: options.svg.font_family.clone(),
                font_size: options.svg.font_size,
                node_padding_x: options.svg.node_padding_x,
                node_padding_y: options.svg.node_padding_y,
                routing_style: options.svg.routing_style,
                curve: options.svg.curve,
                edge_radius: options.svg.edge_radius,
                diagram_padding: options.svg.diagram_padding,
                path_simplification: options.path_simplification,
            }
        }

        fn text_render_options(options: &RenderOptions) -> TextRenderOptions {
            TextRenderOptions {
                output_format: options.output_format,
                text_color_mode: options.text_color_mode,
                routing_style: request_routing_style(options).unwrap_or(RoutingStyle::Orthogonal),
                cluster_ranksep: options.cluster_ranksep,
                padding: options.padding,
                path_simplification: options.path_simplification,
            }
        }

        /// Hidden wrapper for the legacy direct render helper.
        pub fn render(diagram: &Diagram, options: &RenderOptions) -> String {
            let engine = FluxLayeredEngine::text();
            let engine_config = engine_config_for_render(diagram, options);
            let request_config = crate::RenderConfig {
                routing_style: request_routing_style(options),
                path_simplification: options.path_simplification,
                ..crate::RenderConfig::default()
            };
            let request = crate::engines::graph::GraphSolveRequest::from_config(
                &request_config,
                options.output_format,
            );
            let result = engine
                .solve(diagram, &engine_config, &request)
                .expect("engine solve failed in testing::render");

            match options.output_format {
                OutputFormat::Svg => crate::render::graph::render_svg_from_geometry_with_routing(
                    diagram,
                    &result.geometry,
                    &svg_render_options(options),
                    internal_edge_routing(options),
                ),
                OutputFormat::Text | OutputFormat::Ascii => {
                    crate::render::graph::render_text_from_geometry(
                        diagram,
                        &result.geometry,
                        result.routed.as_ref(),
                        &text_render_options(options),
                    )
                }
                OutputFormat::Mmds => {
                    panic!("use testing::render::graph::backends::mmds::render_mmds instead")
                }
                other => panic!("testing::render does not support {other} output"),
            }
        }

        /// Hidden wrapper for the legacy direct SVG helper.
        pub fn render_svg(diagram: &Diagram, options: &RenderOptions) -> String {
            let engine = FluxLayeredEngine::text();
            let engine_config = engine_config_for_render(diagram, options);
            let request_config = crate::RenderConfig {
                routing_style: request_routing_style(options).or(Some(options.svg.routing_style)),
                path_simplification: options.path_simplification,
                ..crate::RenderConfig::default()
            };
            let request = crate::engines::graph::GraphSolveRequest::from_config(
                &request_config,
                OutputFormat::Svg,
            );
            let result = engine
                .solve(diagram, &engine_config, &request)
                .expect("engine solve failed in testing::render_svg");
            crate::render::graph::svg::render_svg_from_geometry(
                diagram,
                &svg_render_options(options),
                &result.geometry,
                internal_edge_routing(options),
            )
        }

        /// Hidden routed-geometry helpers.
        pub mod routing {
            use crate::graph::Diagram;
            use crate::graph::geometry::{FPoint, GraphGeometry, RoutedGraphGeometry};
            use crate::testing::engines::graph::EdgeRouting;

            pub fn route_graph_geometry(
                diagram: &Diagram,
                geometry: &GraphGeometry,
                edge_routing: EdgeRouting,
            ) -> RoutedGraphGeometry {
                crate::graph::routing::route_graph_geometry(diagram, geometry, edge_routing.into())
            }

            pub fn snap_path_to_grid_preview(
                path: &[FPoint],
                scale_x: f64,
                scale_y: f64,
            ) -> Vec<FPoint> {
                crate::graph::routing::snap_path_to_grid_preview(path, scale_x, scale_y)
            }
        }

        /// Hidden text-layout adapter helpers.
        pub mod text_adapter {
            use crate::engines::graph::algorithms::layered::{
                Direction as LayeredDirection, LayoutConfig as LayeredConfig, Ranker,
            };
            use crate::engines::graph::flux::FluxLayeredEngine;
            use crate::engines::graph::{
                EngineConfig, GraphEngine, GraphSolveRequest, OutputFormat, RenderConfig,
            };
            use crate::graph::grid_projection::{GridLayoutConfig, GridRanker};
            use crate::graph::{Diagram, Direction};
            pub use crate::render::graph::text_adapter::{
                geometry_to_text_layout, geometry_to_text_layout_with_routed,
            };
            use crate::render::graph::text_types::Layout;

            /// Hidden compatibility helper for tests that still want
            /// solve-and-project text layout in one step.
            pub fn compute_layout(diagram: &Diagram, config: &GridLayoutConfig) -> Layout {
                let engine = FluxLayeredEngine::text();
                let engine_config = EngineConfig::Layered(LayeredConfig {
                    direction: match diagram.direction {
                        Direction::TopDown => LayeredDirection::TopBottom,
                        Direction::BottomTop => LayeredDirection::BottomTop,
                        Direction::LeftRight => LayeredDirection::LeftRight,
                        Direction::RightLeft => LayeredDirection::RightLeft,
                    },
                    node_sep: config.node_sep,
                    edge_sep: config.edge_sep,
                    rank_sep: config.rank_sep,
                    margin: config.margin,
                    acyclic: true,
                    ranker: match config.ranker.unwrap_or_default() {
                        GridRanker::NetworkSimplex => Ranker::NetworkSimplex,
                        GridRanker::LongestPath => Ranker::LongestPath,
                    },
                    ..Default::default()
                });
                let request =
                    GraphSolveRequest::from_config(&RenderConfig::default(), OutputFormat::Text);
                let result = engine
                    .solve(diagram, &engine_config, &request)
                    .expect("engine solve failed");
                geometry_to_text_layout(diagram, &result.geometry, config)
            }
        }

        /// Hidden text-edge helpers.
        pub mod text_edge {
            pub use crate::render::graph::text_edge::render_all_edges_with_labels;
        }

        /// Hidden text-router helpers.
        pub mod text_router {
            pub use crate::render::graph::text_router::{RoutedEdge, Segment, route_all_edges};
        }

        /// Hidden text-shape helpers.
        pub mod text_shape {
            pub use crate::render::graph::text_shape::{NodeBounds, render_node};
        }

        /// Hidden text-type helpers.
        pub mod text_types {
            pub use crate::render::graph::text_types::{GridLayoutConfig, Layout};
        }

        /// Hidden graph-family backend helpers.
        pub mod backends {
            use crate::graph::Diagram;
            use crate::testing::engines::graph::GraphSolveResult;

            /// Hidden text backend wrapper.
            pub mod text {
                use super::{Diagram, GraphSolveResult};

                pub fn render_text(diagram: &Diagram, result: &GraphSolveResult) -> String {
                    let internal: crate::engines::graph::GraphSolveResult = result.into();
                    crate::render::graph::render_text_from_geometry(
                        diagram,
                        &internal.geometry,
                        internal.routed.as_ref(),
                        &crate::render::graph::TextRenderOptions::default(),
                    )
                }
            }

            /// Hidden SVG backend wrapper.
            pub mod svg {
                use super::{Diagram, GraphSolveResult};

                pub fn render_svg(diagram: &Diagram, result: &GraphSolveResult) -> String {
                    let internal: crate::engines::graph::GraphSolveResult = result.into();
                    crate::render::graph::render_svg_from_geometry_with_routing(
                        diagram,
                        &internal.geometry,
                        &crate::render::graph::SvgRenderOptions::default(),
                        crate::graph::routing::EdgeRouting::OrthogonalRoute,
                    )
                }
            }

            /// Hidden MMDS backend wrapper.
            pub mod mmds {
                use super::{Diagram, GraphSolveResult};
                use crate::config::{GeometryLevel, PathSimplification};
                use crate::errors::RenderError;

                pub fn render_mmds(
                    diagram_type: &str,
                    diagram: &Diagram,
                    result: &GraphSolveResult,
                ) -> String {
                    render_mmds_full(
                        diagram_type,
                        diagram,
                        result,
                        GeometryLevel::Routed,
                        PathSimplification::default(),
                    )
                    .unwrap_or_else(|error| panic!("MMDS rendering failed: {}", error.message))
                }

                pub fn render_mmds_full(
                    diagram_type: &str,
                    diagram: &Diagram,
                    result: &GraphSolveResult,
                    level: GeometryLevel,
                    path_simplification: PathSimplification,
                ) -> Result<String, RenderError> {
                    let internal: crate::engines::graph::GraphSolveResult = result.into();
                    let routed_owned;
                    let routed_ref = match internal.routed.as_ref() {
                        Some(routed) => Some(routed),
                        None if matches!(level, GeometryLevel::Routed) => {
                            routed_owned = crate::graph::routing::route_graph_geometry(
                                diagram,
                                &internal.geometry,
                                crate::graph::routing::EdgeRouting::OrthogonalRoute,
                            );
                            Some(&routed_owned)
                        }
                        None => None,
                    };
                    crate::mmds::to_mmds_json_typed(
                        diagram_type,
                        diagram,
                        &internal.geometry,
                        routed_ref,
                        level,
                        path_simplification,
                        Some(internal.engine_id),
                    )
                }
            }
        }
    }
}

pub use self::engines::graph::{
    EdgeRouting, EngineConfig, GraphEngine, GraphEngineRegistry, GraphSolveRequest,
    GraphSolveResult,
};
pub use self::render::graph::{RenderOptions, SvgOptions, render, render_svg};
pub use crate::engines::graph::algorithms::layered::MeasurementMode;

/// Hidden wrapper for the layered measurement runner used by external tests.
pub fn run_layered_layout(
    mode: &MeasurementMode,
    diagram: &crate::graph::Diagram,
    config: &EngineConfig,
) -> Result<crate::graph::geometry::GraphGeometry, crate::errors::RenderError> {
    let internal: crate::engines::graph::EngineConfig = config.into();
    crate::engines::graph::algorithms::layered::run_layered_layout(mode, diagram, &internal)
}

/// Hidden routed-geometry compatibility helpers.
pub mod routing {
    pub use crate::testing::render::graph::routing::{
        route_graph_geometry, snap_path_to_grid_preview,
    };
}

/// Hidden text-layout adapter compatibility helpers.
pub mod text_adapter {
    pub use crate::testing::render::graph::text_adapter::{
        compute_layout, geometry_to_text_layout, geometry_to_text_layout_with_routed,
    };
}

/// Hidden text-edge compatibility helpers.
pub mod text_edge {
    pub use crate::testing::render::graph::text_edge::render_all_edges_with_labels;
}

/// Hidden text-router compatibility helpers.
pub mod text_router {
    pub use crate::testing::render::graph::text_router::{RoutedEdge, Segment, route_all_edges};
}

/// Hidden text-shape compatibility helpers.
pub mod text_shape {
    pub use crate::testing::render::graph::text_shape::{NodeBounds, render_node};
}

/// Hidden text-type compatibility helpers.
pub mod text_types {
    pub use crate::testing::render::graph::text_types::{GridLayoutConfig, Layout};
}

/// Hidden graph-family backend compatibility helpers.
pub mod backends {
    pub use crate::testing::render::graph::backends::{mmds, svg, text};
}
