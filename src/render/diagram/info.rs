//! Info-diagram renderer.

pub fn render() -> String {
    format!(
        "mmdflux v{}\n\
         Mermaid flowchart to text/SVG renderer",
        env!("CARGO_PKG_VERSION")
    )
}
