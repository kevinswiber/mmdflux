//! Pie-diagram renderer.

use crate::mermaid::pie::Pie;

pub fn render(pie: &Pie) -> String {
    let mut lines = vec![String::from("[Pie Chart]")];
    let mut header = String::from("pie");

    if pie.show_data {
        header.push_str(" showData");
    }

    if let Some(title) = &pie.title {
        header.push_str(" title ");
        header.push_str(title);
    }

    lines.push(header);
    for section in &pie.sections {
        lines.push(format!(
            "\"{}\": {}",
            section.label,
            format_value(section.value)
        ));
    }

    lines.join("\n")
}

fn format_value(value: f64) -> String {
    if value.fract().abs() < f64::EPSILON {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use crate::mermaid::pie::{Pie, PieSection};

    #[test]
    fn render_uses_typed_pie_sections() {
        let pie = Pie {
            show_data: true,
            title: Some("Fruit".to_string()),
            sections: vec![
                PieSection {
                    label: "Apples".to_string(),
                    value: 60.0,
                },
                PieSection {
                    label: "Oranges".to_string(),
                    value: 40.0,
                },
            ],
        };

        let output = super::render(&pie);
        assert!(output.contains("pie showData title Fruit"));
        assert!(output.contains("\"Apples\": 60"));
        assert!(output.contains("\"Oranges\": 40"));
    }
}
