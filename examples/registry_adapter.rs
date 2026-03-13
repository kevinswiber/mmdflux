use mmdflux::RenderConfig;
use mmdflux::builtins::default_registry;
use mmdflux::prepared::PreparedDiagram;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let input = "graph TD\nA[Draft] --> B[Published]";
    let registry = default_registry();
    let resolved = registry
        .resolve(input)
        .expect("built-in registry should detect the flowchart input");
    let instance = registry
        .create(resolved.diagram_id())
        .expect("resolved diagram should have a constructible instance");
    let prepared = instance.parse(input)?.prepare(&RenderConfig::default())?;

    match prepared {
        PreparedDiagram::Graph(graph) => {
            println!(
                "prepared {} diagram with {} nodes",
                graph.diagram_type,
                graph.diagram.nodes.len()
            );
        }
        other => panic!("expected graph payload, got {other:?}"),
    }

    Ok(())
}
