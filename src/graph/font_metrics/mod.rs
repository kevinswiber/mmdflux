//! Static graph-family recorded font metrics.

pub(crate) mod generated;
mod recorded;

pub(crate) use generated::mmdflux_sans_v1::{
    CSS_LINE_HEIGHT_RATIO as RECORDED_SANS_CSS_LINE_HEIGHT_RATIO,
    METRICS_PROFILE_SOURCE as RECORDED_SANS_PROFILE_SOURCE, PROFILE_ID as RECORDED_SANS_PROFILE_ID,
};
pub(crate) use recorded::RecordedMetricsProfile;
