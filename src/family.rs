/// Diagram family classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagramFamily {
    /// Node-edge graphs (flowchart, state, class, ER).
    Graph,
    /// Timeline-based (sequence, gantt, gitgraph).
    Timeline,
    /// Chart/visualization (pie, radar, xy).
    Chart,
    /// Tabular layout (packet, kanban).
    Table,
}
