use super::types::RoutedEdge;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextPathFamily {
    SharedRoutedDrawPath,
    WaypointFallback,
    SyntheticBackward,
    Direct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextPathRejection {
    TooShort,
    NoWaypoints,
    FaceInference,
    WaypointInsideFace,
    SegmentCollision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TextRouteProbe {
    pub(crate) path_family: TextPathFamily,
    pub(crate) rejection_reason: Option<TextPathRejection>,
}

#[derive(Debug, Clone)]
pub(crate) struct RouteEdgeResult {
    pub(crate) routed: RoutedEdge,
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) probe: TextRouteProbe,
}
