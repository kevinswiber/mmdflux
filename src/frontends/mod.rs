//! Source-format frontends.
//!
//! Frontends own input detection and ingestion before runtime dispatch resolves
//! a logical diagram type and family pipeline.

pub mod mermaid;
pub mod mmds;

/// Input source format detected from raw input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFrontend {
    Mermaid,
    Mmds,
}

/// Detect the source-format frontend from raw input.
#[must_use]
pub fn detect_input_frontend(input: &str) -> Option<InputFrontend> {
    if mmds::is_mmds_input(input) {
        Some(InputFrontend::Mmds)
    } else if mermaid::detect_diagram_type(input).is_some() {
        Some(InputFrontend::Mermaid)
    } else {
        None
    }
}
