//! Sequence diagram AST types.
//!
//! These represent the raw parsed syntax before validation/compilation
//! into the `Sequence` model used by the layout engine.
pub use crate::timeline::sequence::model::ParticipantKind;

/// Arrow type for messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrowType {
    /// Solid line with filled arrowhead (`->>`).
    Solid,
    /// Dashed line with filled arrowhead (`-->>`).
    Dashed,
}

/// A parsed sequence diagram statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SequenceStatement {
    /// `participant A` or `participant A as Alice`.
    Participant {
        kind: ParticipantKind,
        id: String,
        alias: Option<String>,
    },
    /// `A->>B: hello` or `A-->>B: hello`.
    Message {
        from: String,
        to: String,
        arrow: ArrowType,
        text: String,
    },
    /// `Note over A: text`.
    Note { over: String, text: String },
    /// `autonumber`.
    Autonumber,
}
