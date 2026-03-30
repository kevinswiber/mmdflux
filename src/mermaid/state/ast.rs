//! AST types for state diagrams.

#![allow(dead_code)]

/// Parsed state diagram model.
#[derive(Debug, Clone)]
pub struct StateModel {
    /// Optional root-level direction directive (`TB`, `BT`, `LR`, `RL`).
    pub direction: Option<String>,
    /// Top-level statements in source order.
    pub statements: Vec<StateStatement>,
}

/// A statement inside a state diagram or composite state body.
#[derive(Debug, Clone)]
pub enum StateStatement {
    /// A state declaration (may include children for composite states).
    State(StateDecl),
    /// A transition between two states.
    Transition(StateTransition),
    /// A direction override inside a composite state.
    Direction(String),
}

/// A state declaration.
#[derive(Debug, Clone)]
pub struct StateDecl {
    /// State identifier (e.g. `Moving`, `s1`).
    pub id: String,
    /// Optional description text from `StateId : description`.
    pub description: Option<String>,
    /// Optional alias from `state "Long description" as StateId`.
    pub alias: Option<String>,
    /// Optional stereotype (`<<fork>>`, `<<join>>`, `<<choice>>`).
    pub stereotype: Option<StateStereotype>,
    /// Children for composite `state Id { ... }` blocks.
    pub children: Vec<StateStatement>,
}

/// Stereotypes for special state node shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateStereotype {
    Fork,
    Join,
    Choice,
}

/// A transition between two states.
#[derive(Debug, Clone)]
pub struct StateTransition {
    /// Source state id (may be `"[*]"` for start/end pseudo-state).
    pub from: String,
    /// Target state id (may be `"[*]"` for start/end pseudo-state).
    pub to: String,
    /// Optional label text from `A --> B : label`.
    pub label: Option<String>,
}
