//! AST types for class diagrams.

#![allow(dead_code)]

/// Parsed class diagram model.
#[derive(Debug, Clone)]
pub struct ClassModel {
    /// Class declarations (in order of appearance).
    pub classes: Vec<ClassDecl>,
    /// Relationships between classes.
    pub relations: Vec<ClassRelation>,
    /// Optional Mermaid direction directive (`LR`, `RL`, `BT`, `TB`).
    pub direction: Option<String>,
    /// Namespace declarations (inner-first close order).
    pub namespaces: Vec<ClassNamespace>,
}

/// A class declaration.
#[derive(Debug, Clone)]
pub struct ClassDecl {
    /// Class name/identifier.
    pub name: String,
    /// Optional Mermaid display label (`class Id["Display Label"]`).
    pub display_label: Option<String>,
    /// Optional containing namespace ID.
    pub namespace: Option<String>,
    /// Optional class annotations/stereotypes (without `<<`/`>>`).
    pub annotations: Vec<String>,
    /// Body members (fields and methods), if any.
    pub members: Vec<String>,
}

/// A parsed Mermaid namespace declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassNamespace {
    /// Internal namespace identifier (stable for compiler/subgraph wiring).
    pub id: String,
    /// Display namespace name.
    pub name: String,
    /// Parent namespace ID when nested.
    pub parent: Option<String>,
}

/// A relationship between two classes.
#[derive(Debug, Clone)]
pub struct ClassRelation {
    /// Left-hand class name (always first in declaration order).
    pub from: String,
    /// Right-hand class name (always second in declaration order).
    pub to: String,
    /// Relationship type.
    pub relation_type: ClassRelationType,
    /// Optional label on the relationship.
    pub label: Option<String>,
    /// Optional cardinality/multiplicity label on the `from` endpoint.
    pub cardinality_from: Option<String>,
    /// Optional cardinality/multiplicity label on the `to` endpoint.
    pub cardinality_to: Option<String>,
    /// When true, the relationship marker belongs on the `from` (left) end
    /// rather than the `to` (right) end. Set for left-pointing operators
    /// like `<|--`, `*--`, `o--`.
    pub marker_start: bool,
    /// When true, the relationship marker belongs on the `to` (right) end.
    /// This is true for right-pointing and two-way relation operators.
    pub marker_end: bool,
}

/// Types of class relationships (MVP scope).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassRelationType {
    /// `--`  Plain association (no arrow).
    Association,
    /// `-->`  Directed association (with arrow).
    DirectedAssociation,
    /// `<|--`  Inheritance/generalization.
    Inheritance,
    /// `<|..` / `..|>`  Realization/implementation.
    Realization,
    /// `*--`  Composition.
    Composition,
    /// `o--`  Aggregation.
    Aggregation,
    /// `..`  Dependency (no arrow).
    Dependency,
    /// `..>`  Directed dependency (with arrow).
    DirectedDependency,
    /// `--()` / `()--`  Lollipop interface relation.
    Lollipop,
}
