//! MMDS JSON parsing with profile negotiation and structural validation.

use super::detect::resolve_logical_diagram_id;
use crate::errors::RenderError;
use crate::mmds::{
    MmdsOutput, MmdsParseError, MmdsProfileNegotiation, evaluate_mmds_profiles_for_output,
    parse_mmds_input,
};

/// Parse MMDS input, returning the payload and profile negotiation result.
pub fn parse_with_profiles(
    input: &str,
) -> Result<(MmdsOutput, MmdsProfileNegotiation), MmdsParseError> {
    let payload = parse_mmds_input(input)?;
    let negotiation = evaluate_mmds_profiles_for_output(&payload);
    Ok((payload, negotiation))
}

/// Validate MMDS input by parsing and resolving its logical diagram type.
pub fn validate_input(input: &str) -> Result<(), RenderError> {
    let output = parse_mmds_input(input).map_err(|error| RenderError {
        message: format!("parse error: {error}"),
    })?;
    resolve_logical_diagram_id(&output)?;
    Ok(())
}
