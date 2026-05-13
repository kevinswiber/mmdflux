use std::cell::RefCell;
use std::collections::HashMap;

use super::EdgeRouting;
use crate::graph::attachment::EdgePort;
use crate::graph::geometry::{
    EdgeLabelSide, GraphGeometry, RoutedEdgeGeometry, RoutedGraphGeometry,
};
use crate::graph::measure::{
    TextMetricsProvider, edge_label_dimensions_for_provider_and_style,
    edge_label_dimensions_wrapped_for_provider_and_style, edge_text_style_key,
};
use crate::graph::space::{FPoint, FRect};
use crate::graph::{Direction, Graph, Shape};

thread_local! {
    static ACTIVE_TRACE: RefCell<Option<RoutingTrace>> = const { RefCell::new(None) };
}

#[derive(Debug, Default, Clone)]
pub(crate) struct RoutingTrace {
    pub(crate) stages: Vec<RoutingTraceStageSnapshot>,
}

impl RoutingTrace {
    pub(crate) fn push_stage(&mut self, stage: RoutingTraceStageSnapshot) {
        self.stages.push(stage);
    }

    pub(crate) fn stage_names(&self) -> Vec<RoutingTraceStage> {
        self.stages.iter().map(|stage| stage.stage).collect()
    }

    pub(crate) fn has_stage(&self, stage: RoutingTraceStage) -> bool {
        self.stages.iter().any(|snapshot| snapshot.stage == stage)
    }

    pub(crate) fn input(&self) -> Option<&RouteInputSnapshot> {
        self.stages.iter().find_map(|stage| stage.input.as_ref())
    }

    pub(crate) fn output(&self) -> Option<&RouteOutputSnapshot> {
        self.stages.iter().find_map(|stage| stage.output.as_ref())
    }

    pub(crate) fn label_lanes(&self) -> Option<&LabelLaneTraceSnapshot> {
        self.stages
            .iter()
            .find_map(|stage| stage.label_lanes.as_ref())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RoutingTraceStage {
    Input,
    LabelLanes,
    Output,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RoutingTraceStageSnapshot {
    pub(crate) stage: RoutingTraceStage,
    pub(crate) input: Option<RouteInputSnapshot>,
    pub(crate) label_lanes: Option<LabelLaneTraceSnapshot>,
    pub(crate) output: Option<RouteOutputSnapshot>,
}

impl RoutingTraceStageSnapshot {
    pub(crate) fn empty(stage: RoutingTraceStage) -> Self {
        Self {
            stage,
            input: (stage == RoutingTraceStage::Input).then(RouteInputSnapshot::empty),
            label_lanes: (stage == RoutingTraceStage::LabelLanes)
                .then(LabelLaneTraceSnapshot::default),
            output: (stage == RoutingTraceStage::Output).then(RouteOutputSnapshot::default),
        }
    }

    pub(crate) fn label_lanes(label_lanes: LabelLaneTraceSnapshot) -> Self {
        Self {
            stage: RoutingTraceStage::LabelLanes,
            input: None,
            label_lanes: Some(label_lanes),
            output: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RouteInputSnapshot {
    pub(crate) edge_routing: EdgeRouting,
    pub(crate) nodes: Vec<RouteNodeInput>,
    pub(crate) edges: Vec<RouteEdgeInput>,
    pub(crate) labels: Vec<RouteLabelInput>,
}

impl RouteInputSnapshot {
    fn empty() -> Self {
        Self {
            edge_routing: EdgeRouting::PolylineRoute,
            nodes: Vec::new(),
            edges: Vec::new(),
            labels: Vec::new(),
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct RouteOutputSnapshot {
    pub(crate) edges: Vec<RouteEdgeOutput>,
    pub(crate) labels: Vec<RouteLabelOutput>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct LabelLaneTraceSnapshot {
    pub(crate) edges: Vec<LabelLaneEdgeSnapshot>,
    pub(crate) compartments: Vec<LabelLaneCompartmentSnapshot>,
    pub(crate) subclusters: Vec<LabelLaneSubclusterSnapshot>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LabelLaneEdgeSnapshot {
    pub(crate) edge_index: usize,
    pub(crate) mmds_edge_id: String,
    pub(crate) compartment_id: String,
    pub(crate) subcluster_id: String,
    pub(crate) sort_position: usize,
    pub(crate) track: i32,
    pub(crate) track_center: f64,
    pub(crate) label_step: f64,
    pub(crate) label_rect: FRect,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LabelLaneCompartmentSnapshot {
    pub(crate) id: String,
    pub(crate) member_edge_indices: Vec<usize>,
    pub(crate) member_edge_ids: Vec<String>,
    pub(crate) scope_parent: Option<String>,
    pub(crate) cross_min: f64,
    pub(crate) cross_max: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LabelLaneSubclusterSnapshot {
    pub(crate) id: String,
    pub(crate) compartment_id: String,
    pub(crate) member_edge_indices: Vec<usize>,
    pub(crate) member_edge_ids: Vec<String>,
    pub(crate) sweep_order: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RouteNodeInput {
    pub(crate) id: String,
    pub(crate) rect: FRect,
    pub(crate) shape: Shape,
    pub(crate) label: String,
    pub(crate) parent: Option<String>,
    pub(crate) direction: Direction,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RouteEdgeInput {
    pub(crate) index: usize,
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) waypoints: Vec<FPoint>,
    pub(crate) layout_path_hint: Option<Vec<FPoint>>,
    pub(crate) label_position: Option<FPoint>,
    pub(crate) label_side: Option<EdgeLabelSide>,
    pub(crate) source_port: Option<RoutePortInput>,
    pub(crate) target_port: Option<RoutePortInput>,
    pub(crate) is_backward: bool,
    pub(crate) preserve_orthogonal_topology: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RouteLabelInput {
    pub(crate) edge_index: usize,
    pub(crate) label_text: Option<String>,
    pub(crate) width: f64,
    pub(crate) height: f64,
    pub(crate) axis_min: f64,
    pub(crate) axis_max: f64,
    pub(crate) cross_min: f64,
    pub(crate) cross_max: f64,
    pub(crate) side: Option<EdgeLabelSide>,
    pub(crate) direction_sign: i32,
    pub(crate) scope_parent: Option<String>,
    pub(crate) midpoint: FPoint,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RoutePortInput {
    pub(crate) face: String,
    pub(crate) fraction: f64,
    pub(crate) group_size: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RouteEdgeOutput {
    pub(crate) index: usize,
    pub(crate) path: Vec<FPoint>,
    pub(crate) source_port: Option<RoutePortInput>,
    pub(crate) target_port: Option<RoutePortInput>,
    pub(crate) is_backward: bool,
    pub(crate) preserve_orthogonal_topology: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RouteLabelOutput {
    pub(crate) edge_index: usize,
    pub(crate) center: FPoint,
    pub(crate) rect: FRect,
    pub(crate) side: EdgeLabelSide,
    pub(crate) track: i32,
    pub(crate) compartment_size: usize,
}

pub(crate) fn begin_capture() {
    ACTIVE_TRACE.with(|trace| {
        *trace.borrow_mut() = Some(RoutingTrace::default());
    });
}

pub(crate) fn finish_capture() -> RoutingTrace {
    ACTIVE_TRACE.with(|trace| trace.borrow_mut().take().unwrap_or_default())
}

pub(crate) fn capture_route_input(
    diagram: &Graph,
    geometry: &GraphGeometry,
    edge_routing: EdgeRouting,
    port_attachments: &HashMap<usize, (Option<EdgePort>, Option<EdgePort>)>,
    metrics: &dyn TextMetricsProvider,
) {
    ACTIVE_TRACE.with(|trace| {
        let mut trace = trace.borrow_mut();
        let Some(active) = trace.as_mut() else {
            return;
        };
        active.push_stage(RoutingTraceStageSnapshot {
            stage: RoutingTraceStage::Input,
            input: Some(RouteInputSnapshot::from_geometry(
                diagram,
                geometry,
                edge_routing,
                port_attachments,
                metrics,
            )),
            label_lanes: None,
            output: None,
        });
    });
}

pub(crate) fn capture_route_output(routed: &RoutedGraphGeometry) {
    ACTIVE_TRACE.with(|trace| {
        let mut trace = trace.borrow_mut();
        let Some(active) = trace.as_mut() else {
            return;
        };
        active.push_stage(RoutingTraceStageSnapshot {
            stage: RoutingTraceStage::Output,
            input: None,
            label_lanes: None,
            output: Some(RouteOutputSnapshot::from_routed(routed)),
        });
    });
}

pub(crate) fn capture_label_lanes(snapshot: LabelLaneTraceSnapshot) {
    ACTIVE_TRACE.with(|trace| {
        let mut trace = trace.borrow_mut();
        let Some(active) = trace.as_mut() else {
            return;
        };
        active.push_stage(RoutingTraceStageSnapshot::label_lanes(snapshot));
    });
}

impl RouteInputSnapshot {
    fn from_geometry(
        diagram: &Graph,
        geometry: &GraphGeometry,
        edge_routing: EdgeRouting,
        port_attachments: &HashMap<usize, (Option<EdgePort>, Option<EdgePort>)>,
        metrics: &dyn TextMetricsProvider,
    ) -> Self {
        let mut nodes = geometry
            .nodes
            .values()
            .map(|node| RouteNodeInput {
                id: node.id.clone(),
                rect: node.rect,
                shape: node.shape,
                label: node.label.clone(),
                parent: node.parent.clone(),
                direction: geometry
                    .node_directions
                    .get(&node.id)
                    .copied()
                    .unwrap_or(geometry.direction),
            })
            .collect::<Vec<_>>();
        nodes.sort_by(|a, b| a.id.cmp(&b.id));

        let edges = geometry
            .edges
            .iter()
            .map(|edge| {
                let (source_port, target_port) = port_attachments
                    .get(&edge.index)
                    .cloned()
                    .unwrap_or((None, None));
                RouteEdgeInput {
                    index: edge.index,
                    from: edge.from.clone(),
                    to: edge.to.clone(),
                    waypoints: edge.waypoints.clone(),
                    layout_path_hint: edge.layout_path_hint.clone(),
                    label_position: edge.label_position,
                    label_side: edge.label_side,
                    source_port: source_port.as_ref().map(RoutePortInput::from_edge_port),
                    target_port: target_port.as_ref().map(RoutePortInput::from_edge_port),
                    is_backward: geometry.reversed_edges.contains(&edge.index),
                    preserve_orthogonal_topology: edge.preserve_orthogonal_topology,
                }
            })
            .collect();

        let labels = geometry
            .edges
            .iter()
            .filter_map(|edge| {
                let diagram_edge = diagram.edges.get(edge.index)?;
                let label = diagram_edge.label.as_deref()?;
                if label.is_empty() {
                    return None;
                }
                let midpoint = edge.label_position?;
                let style = edge_text_style_key(metrics, diagram_edge);
                let (width, height) = match diagram_edge.wrapped_label_lines.as_deref() {
                    Some(lines) => {
                        edge_label_dimensions_wrapped_for_provider_and_style(metrics, &style, lines)
                    }
                    None => edge_label_dimensions_for_provider_and_style(metrics, &style, label),
                };
                let (axis_dim, cross_dim, axis_center, cross_center) = match geometry.direction {
                    Direction::TopDown | Direction::BottomTop => {
                        (height, width, midpoint.y, midpoint.x)
                    }
                    Direction::LeftRight | Direction::RightLeft => {
                        (width, height, midpoint.x, midpoint.y)
                    }
                };
                Some(RouteLabelInput {
                    edge_index: edge.index,
                    label_text: Some(label.to_string()),
                    width,
                    height,
                    axis_min: axis_center - axis_dim / 2.0,
                    axis_max: axis_center + axis_dim / 2.0,
                    cross_min: cross_center - cross_dim / 2.0,
                    cross_max: cross_center + cross_dim / 2.0,
                    side: edge.label_side,
                    direction_sign: if geometry.reversed_edges.contains(&edge.index) {
                        -1
                    } else {
                        1
                    },
                    scope_parent: shared_direct_parent(geometry, &edge.from, &edge.to),
                    midpoint,
                })
            })
            .collect();

        Self {
            edge_routing,
            nodes,
            edges,
            labels,
        }
    }
}

impl RouteOutputSnapshot {
    fn from_routed(routed: &RoutedGraphGeometry) -> Self {
        Self {
            edges: routed
                .edges
                .iter()
                .map(RouteEdgeOutput::from_routed_edge)
                .collect(),
            labels: routed
                .edges
                .iter()
                .filter_map(|edge| {
                    let label = edge.label_geometry.as_ref()?;
                    Some(RouteLabelOutput {
                        edge_index: edge.index,
                        center: label.center,
                        rect: label.rect,
                        side: label.side,
                        track: label.track,
                        compartment_size: label.compartment_size,
                    })
                })
                .collect(),
        }
    }
}

impl RouteEdgeOutput {
    fn from_routed_edge(edge: &RoutedEdgeGeometry) -> Self {
        Self {
            index: edge.index,
            path: edge.path.clone(),
            source_port: edge
                .source_port
                .as_ref()
                .map(RoutePortInput::from_edge_port),
            target_port: edge
                .target_port
                .as_ref()
                .map(RoutePortInput::from_edge_port),
            is_backward: edge.is_backward,
            preserve_orthogonal_topology: edge.preserve_orthogonal_topology,
        }
    }
}

impl RoutePortInput {
    fn from_edge_port(port: &EdgePort) -> Self {
        Self {
            face: port.face.as_str().to_string(),
            fraction: port.fraction,
            group_size: port.group_size,
        }
    }
}

fn shared_direct_parent(geometry: &GraphGeometry, from: &str, to: &str) -> Option<String> {
    let from_parent = geometry.nodes.get(from)?.parent.as_ref()?;
    let to_parent = geometry.nodes.get(to)?.parent.as_ref()?;
    (from_parent == to_parent).then(|| from_parent.clone())
}

#[test]
fn route_trace_records_input_and_output_stages_in_order() {
    let mut trace = RoutingTrace::default();
    trace.push_stage(RoutingTraceStageSnapshot::empty(RoutingTraceStage::Input));
    trace.push_stage(RoutingTraceStageSnapshot::empty(RoutingTraceStage::Output));

    assert_eq!(
        trace.stage_names(),
        vec![RoutingTraceStage::Input, RoutingTraceStage::Output]
    );
    assert!(trace.input().is_some());
    assert!(trace.output().is_some());
}

#[test]
fn label_lane_trace_records_stage_and_snapshot() {
    let mut trace = RoutingTrace::default();
    trace.push_stage(RoutingTraceStageSnapshot::label_lanes(
        LabelLaneTraceSnapshot {
            edges: vec![LabelLaneEdgeSnapshot {
                edge_index: 2,
                mmds_edge_id: "e2".to_string(),
                compartment_id: "scope:none|members:e2,e4".to_string(),
                subcluster_id: "scope:none|members:e2,e4|cluster:e2,e4".to_string(),
                sort_position: 1,
                track: -1,
                track_center: -0.5,
                label_step: 32.0,
                label_rect: FRect::new(10.0, 20.0, 40.0, 16.0),
            }],
            compartments: vec![LabelLaneCompartmentSnapshot {
                id: "scope:none|members:e2,e4".to_string(),
                member_edge_indices: vec![2, 4],
                member_edge_ids: vec!["e2".to_string(), "e4".to_string()],
                scope_parent: None,
                cross_min: 10.0,
                cross_max: 80.0,
            }],
            subclusters: vec![LabelLaneSubclusterSnapshot {
                id: "scope:none|members:e2,e4|cluster:e2,e4".to_string(),
                compartment_id: "scope:none|members:e2,e4".to_string(),
                member_edge_indices: vec![2, 4],
                member_edge_ids: vec!["e2".to_string(), "e4".to_string()],
                sweep_order: vec!["e4".to_string(), "e2".to_string()],
            }],
        },
    ));

    assert_eq!(trace.stage_names(), vec![RoutingTraceStage::LabelLanes]);
    let snapshot = trace.label_lanes().expect("label-lane snapshot");
    assert_eq!(snapshot.edges[0].mmds_edge_id, "e2");
    assert_eq!(snapshot.edges[0].track, -1);
    assert_eq!(snapshot.compartments[0].member_edge_ids, vec!["e2", "e4"]);
}

#[test]
fn route_input_snapshot_carries_label_and_port_facts() {
    let snapshot = RouteInputSnapshot {
        edge_routing: EdgeRouting::PolylineRoute,
        nodes: vec![RouteNodeInput {
            id: "A".to_string(),
            rect: FRect::new(10.0, 20.0, 40.0, 30.0),
            shape: Shape::Rectangle,
            label: "A".to_string(),
            parent: None,
            direction: Direction::TopDown,
        }],
        edges: vec![RouteEdgeInput {
            index: 0,
            from: "A".to_string(),
            to: "B".to_string(),
            waypoints: vec![FPoint::new(10.0, 50.0)],
            layout_path_hint: None,
            label_position: Some(FPoint::new(10.0, 35.0)),
            label_side: Some(EdgeLabelSide::Above),
            source_port: Some(RoutePortInput {
                face: "south".to_string(),
                fraction: 0.5,
                group_size: 1,
            }),
            target_port: None,
            is_backward: false,
            preserve_orthogonal_topology: false,
        }],
        labels: vec![RouteLabelInput {
            edge_index: 0,
            label_text: None,
            width: 80.0,
            height: 28.0,
            axis_min: 21.0,
            axis_max: 49.0,
            cross_min: -30.0,
            cross_max: 50.0,
            side: Some(EdgeLabelSide::Above),
            direction_sign: 1,
            scope_parent: None,
            midpoint: FPoint::new(10.0, 35.0),
        }],
    };

    assert_eq!(snapshot.edge_routing, EdgeRouting::PolylineRoute);
    assert_eq!(snapshot.labels[0].width, 80.0);
    assert_eq!(
        snapshot.edges[0].source_port.as_ref().unwrap().face,
        "south"
    );
}
