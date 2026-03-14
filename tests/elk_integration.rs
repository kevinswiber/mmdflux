//! ELK layout engine integration tests.
//!
//! These tests require the `engine-elk` feature flag and an available
//! ELK subprocess runtime (`mmdflux-elk` on PATH or `MMDFLUX_ELK_CMD`).

#![cfg(feature = "engine-elk")]

use mmdflux::{EngineAlgorithmId, OutputFormat, RenderConfig, RenderError};

/// Helper: render via the FlowchartInstance with a specific engine.
fn render_with_engine(input: &str, engine: &str) -> Result<String, RenderError> {
    let engine_id = EngineAlgorithmId::parse(engine)?;
    let config = RenderConfig {
        layout_engine: Some(engine_id),
        ..Default::default()
    };
    mmdflux::render_diagram(input, OutputFormat::Text, &config)
}

#[test]
fn elk_render_returns_error_when_subprocess_missing() {
    // SAFETY: test runs single-threaded; no other thread reads this env var
    unsafe {
        std::env::set_var("MMDFLUX_ELK_CMD", "nonexistent-elk-binary-99999");
    }
    let result = render_with_engine("graph TD\nA-->B", "elk-layered");
    unsafe {
        std::env::remove_var("MMDFLUX_ELK_CMD");
    }

    assert!(
        result.is_err(),
        "should error when ELK subprocess is missing"
    );
    let err = result.unwrap_err();
    assert!(
        err.message.contains("not found")
            || err.message.contains("ELK")
            || err.message.contains("engine-elk"),
        "error should explain subprocess issue: {}",
        err.message
    );
}
