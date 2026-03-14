use mmdflux::{OutputFormat, RenderConfig, render_diagram};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = "graph TD\nA[Collect] --> B[Render]";
    let output = render_diagram(input, OutputFormat::Text, &RenderConfig::default())?;

    println!("{output}");
    Ok(())
}
