//! Layout engine adapters and registries.
//!
//! Engines are organized by diagram family first. Within a family, the
//! hierarchy is:
//! - engine adapters, such as Flux or Mermaid compatibility layers
//! - shared algorithm kernels, such as the layered algorithm
//!
//! This keeps engine policy separate from reusable layout machinery.

pub mod graph;
