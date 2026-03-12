//! Shared runtime facade for CLI, WASM, and library consumers.
//!
//! This module provides the single orchestration contract that all adapters
//! delegate to, plus serde-friendly config input types for JSON consumers.

pub mod config_input;
pub mod facade;
#[cfg(test)]
pub(crate) mod test_support_tests;
