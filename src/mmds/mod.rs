//! MMDS interchange contract and output-generation namespace.
//!
//! Input ingestion belongs to [`crate::frontends::mmds`]. This module owns the
//! typed MMDS envelope, profile vocabulary, Mermaid regeneration helpers, and
//! graph-family serialization to MMDS JSON.

mod contract;
mod mermaid;
mod output;

pub use contract::{
    MmdsParseError, MmdsProfileNegotiation, evaluate_mmds_profiles,
    evaluate_mmds_profiles_for_output, parse_mmds_input,
};
pub use mermaid::{
    MmdsGenerationError, generate_mermaid_from_mmds, generate_mermaid_from_mmds_str,
};
pub use output::*;
