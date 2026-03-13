//! Residual crate-local tests plus temporary rehome staging.

// Intentional long-lived crate-central coverage.
mod cross_pipeline;

// Transitional staging while owner-local suites are still being rehomed.
mod dagre_parity;
mod integration;
mod model_order;
mod routed_geometry;
mod support;
mod svg_render;
