//! MMDS interchange contract and output-generation namespace.
//!
//! This module owns the typed MMDS envelope, profile vocabulary, Mermaid
//! regeneration helpers, validation, replay helpers, and hydration to
//! `Diagram` for adapter workflows.

pub(crate) mod detect;
pub(crate) mod hydrate;
mod mermaid;
mod output;
pub(crate) mod parse;
pub(crate) mod replay;

use std::error::Error;
use std::fmt;

pub use detect::{
    SUPPORTED_OUTPUT_FORMATS, detect_diagram_type, is_mmds_input, resolve_logical_diagram_id,
    supports_format,
};
pub use hydrate::{MmdsHydrationError, from_mmds_output, from_mmds_str};
pub use mermaid::{
    MmdsGenerationError, generate_mermaid_from_mmds, generate_mermaid_from_mmds_str,
};
// Internal serialization (runtime plumbing, not part of public contract).
pub(crate) use output::to_mmds_json_typed_with_routing;
// Profile vocabulary constants.
pub use output::{
    MMDS_CORE_PROFILE, MMDS_NODE_STYLE_EXTENSION_NAMESPACE, MMDS_NODE_STYLE_PROFILE,
    MMDS_SVG_PROFILE, MMDS_TEXT_EXTENSION_NAMESPACE, MMDS_TEXT_PROFILE, SUPPORTED_MMDS_PROFILES,
};
// Schema types (public adapter contract).
pub use output::{
    MmdsBounds, MmdsDefaults, MmdsEdge, MmdsEdgeDefaults, MmdsMetadata, MmdsNode, MmdsNodeDefaults,
    MmdsOutput, MmdsPort, MmdsPosition, MmdsSize, MmdsSubgraph,
};
pub use parse::{parse_with_profiles, validate_input};
pub use replay::{render_input, render_output};
use serde_json::{Map, Value};

#[cfg(test)]
mod regression_tests;

/// Parse-time error for MMDS input.
#[derive(Debug, Clone)]
pub struct MmdsParseError {
    message: String,
}

impl MmdsParseError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for MmdsParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for MmdsParseError {}

/// Result of profile capability evaluation for a parsed MMDS payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MmdsProfileNegotiation {
    /// Profiles recognized by the current runtime.
    pub supported: Vec<String>,
    /// Profiles declared by payload but unknown to this runtime.
    pub unknown: Vec<String>,
}

/// Parse MMDS JSON input into the typed output envelope.
///
/// Unlike a plain deserialize, this expands omitted node/edge fields using
/// the top-level `defaults` block before constructing `MmdsOutput`.
pub fn parse_mmds_input(input: &str) -> Result<MmdsOutput, MmdsParseError> {
    let mut value: Value = serde_json::from_str(input)
        .map_err(|err| MmdsParseError::new(format!("MMDS parse error: {err}")))?;

    expand_defaults_in_value(&mut value)?;

    serde_json::from_value::<MmdsOutput>(value)
        .map_err(|err| MmdsParseError::new(format!("MMDS parse error: {err}")))
}

/// Evaluate declared profiles against runtime-known profile vocabulary.
///
/// This helper is advisory. Hydration remains permissive with unknown profiles.
pub fn evaluate_mmds_profiles(input: &str) -> Result<MmdsProfileNegotiation, MmdsParseError> {
    let output = parse_mmds_input(input)?;
    Ok(evaluate_mmds_profiles_for_output(&output))
}

/// Evaluate declared profiles for an already-parsed MMDS payload.
pub fn evaluate_mmds_profiles_for_output(output: &MmdsOutput) -> MmdsProfileNegotiation {
    let mut supported = Vec::new();
    let mut unknown = Vec::new();
    let mut seen_supported = std::collections::HashSet::new();
    let mut seen_unknown = std::collections::HashSet::new();

    for profile in &output.profiles {
        if SUPPORTED_MMDS_PROFILES.contains(&profile.as_str()) {
            if seen_supported.insert(profile.clone()) {
                supported.push(profile.clone());
            }
            continue;
        }

        if seen_unknown.insert(profile.clone()) {
            unknown.push(profile.clone());
        }
    }

    MmdsProfileNegotiation { supported, unknown }
}

fn expand_defaults_in_value(value: &mut Value) -> Result<(), MmdsParseError> {
    let root = value.as_object_mut().ok_or_else(|| {
        MmdsParseError::new("MMDS parse error: top-level JSON value must be an object")
    })?;

    let node_shape = default_value(
        root,
        &["defaults", "node", "shape"],
        Value::String("rectangle".to_string()),
    );
    let edge_stroke = default_value(
        root,
        &["defaults", "edge", "stroke"],
        Value::String("solid".to_string()),
    );
    let edge_arrow_start = default_value(
        root,
        &["defaults", "edge", "arrow_start"],
        Value::String("none".to_string()),
    );
    let edge_arrow_end = default_value(
        root,
        &["defaults", "edge", "arrow_end"],
        Value::String("normal".to_string()),
    );
    let edge_minlen = default_value(root, &["defaults", "edge", "minlen"], Value::from(1));

    if let Some(nodes) = root.get_mut("nodes").and_then(Value::as_array_mut) {
        for node in nodes {
            if let Some(node_obj) = node.as_object_mut() {
                insert_default(node_obj, "shape", &node_shape);
            }
        }
    }

    if let Some(edges) = root.get_mut("edges").and_then(Value::as_array_mut) {
        for edge in edges {
            if let Some(edge_obj) = edge.as_object_mut() {
                insert_default(edge_obj, "stroke", &edge_stroke);
                insert_default(edge_obj, "arrow_start", &edge_arrow_start);
                insert_default(edge_obj, "arrow_end", &edge_arrow_end);
                insert_default(edge_obj, "minlen", &edge_minlen);
            }
        }
    }

    Ok(())
}

fn default_value(root: &Map<String, Value>, path: &[&str], fallback: Value) -> Value {
    traverse_value(root, path).cloned().unwrap_or(fallback)
}

fn insert_default(object: &mut Map<String, Value>, key: &str, default: &Value) {
    object
        .entry(key.to_string())
        .or_insert_with(|| default.clone());
}

fn traverse_value<'a>(root: &'a Map<String, Value>, path: &[&str]) -> Option<&'a Value> {
    let (first, rest) = path.split_first()?;
    let mut current = root.get(*first)?;
    for key in rest {
        current = current.get(*key)?;
    }
    Some(current)
}
