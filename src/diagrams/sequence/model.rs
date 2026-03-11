//! Sequence diagram model.
//!
//! The validated model used by the timeline layout engine. Produced by
//! compiling the raw parsed AST statements.

use crate::frontends::mermaid::sequence::ast::ParticipantKind;

/// A participant in the sequence diagram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Participant {
    /// Unique identifier.
    pub id: String,
    /// Display label (alias if provided, otherwise id).
    pub label: String,
    /// Whether this is a participant box or actor stick-figure.
    pub kind: ParticipantKind,
}

/// Arrow style for a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageStyle {
    /// Solid line with filled arrowhead.
    Solid,
    /// Dashed line with filled arrowhead.
    Dashed,
}

/// An event in the sequence (message or note).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SequenceEvent {
    /// A message between (or within) participants.
    Message {
        /// Index into `SequenceModel::participants`.
        from: usize,
        /// Index into `SequenceModel::participants`.
        to: usize,
        /// Arrow style.
        style: MessageStyle,
        /// Message text label.
        text: String,
        /// Optional autonumber prefix (1-indexed).
        number: Option<usize>,
    },
    /// A note over one participant.
    Note {
        /// Index into `SequenceModel::participants`.
        over: usize,
        /// Note text.
        text: String,
    },
}

/// The validated sequence diagram model.
///
/// Participants are in stable declaration order. Events reference participants
/// by index. This is the input to the timeline layout engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceModel {
    /// Participants in declaration order.
    pub participants: Vec<Participant>,
    /// Events in source order.
    pub events: Vec<SequenceEvent>,
    /// Whether autonumber is enabled.
    pub autonumber: bool,
}
