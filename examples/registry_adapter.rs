use mmdflux::builtins::default_registry;
use mmdflux::payload::Diagram;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let input = "graph TD\nA[Draft] --> B[Published]";
    let registry = default_registry();
    let resolved = registry
        .resolve(input)
        .expect("built-in registry should detect the flowchart input");
    let instance = registry
        .create(resolved.diagram_id())
        .expect("resolved diagram should have a constructible instance");
    let payload = instance.parse(input)?.into_payload()?;

    match payload {
        Diagram::Flowchart(graph) => {
            println!(
                "built flowchart graph payload with {} nodes",
                graph.nodes.len()
            );
        }
        other => panic!("expected flowchart payload, got {other:?}"),
    }

    Ok(())
}
