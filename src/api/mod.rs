//! Stable public API surface for mmdflux.

pub mod config;
pub mod diagnostics;
pub mod errors;
pub mod format;
pub mod request;
pub mod traits;

pub use config::{
    AlgorithmId, ColorWhen, EngineAlgorithmCapabilities, EngineAlgorithmId, EngineId,
    GeometryLevel, LayoutConfig, PathSimplification, RenderConfig, RouteOwnership, TextColorMode,
};
pub use diagnostics::ParseDiagnostic;
pub use errors::RenderError;
pub use format::{CornerStyle, Curve, EdgePreset, OutputFormat, RoutingStyle};
pub use request::RenderRequest;
pub use traits::{DiagramFamily, DiagramModel, DiagramParser, DiagramRenderer};

pub(crate) fn normalize_enum_token(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('_', "-")
}
