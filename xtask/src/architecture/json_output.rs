use std::io::{self, Write};
use std::path::Path;

use serde_json::{Value, json};

use super::boundaries::BoundaryViolation;

pub(crate) fn violation_to_cargo_diagnostic_json(
    violation: &BoundaryViolation,
    repo_root: &Path,
) -> Value {
    let message = format!(
        "forbidden dependency from `{}` to `{}`",
        violation.source_boundary, violation.target_boundary
    );

    let spans = if let Some(file) = &violation.file {
        let line = violation.line.unwrap_or(1);
        let col_start = violation.column.unwrap_or(1);
        let col_end = violation
            .underline_offset
            .zip(violation.underline_len)
            .map(|(offset, len)| offset + len + 1)
            .unwrap_or(col_start + 1);

        let text = violation
            .line_text
            .as_ref()
            .map(|t| {
                json!([{
                    "text": t,
                    "highlight_start": col_start,
                    "highlight_end": col_end,
                }])
            })
            .unwrap_or(json!([]));

        json!([{
            "file_name": file,
            "line_start": line,
            "line_end": line,
            "column_start": col_start,
            "column_end": col_end,
            "byte_start": 0,
            "byte_end": 0,
            "is_primary": true,
            "text": text,
            "label": format!("forbidden dependency on `{}`", violation.target_boundary),
            "suggested_replacement": null,
            "suggestion_applicability": null,
            "expansion": null,
        }])
    } else {
        json!([])
    };

    let manifest_path = repo_root.join("Cargo.toml");
    let src_path = repo_root.join("src/lib.rs");

    json!({
        "reason": "compiler-message",
        "package_id": "mmdflux",
        "manifest_path": manifest_path.display().to_string(),
        "target": {
            "kind": ["lib"],
            "crate_types": ["lib"],
            "name": "mmdflux",
            "src_path": src_path.display().to_string(),
            "edition": "2021",
            "doc": true,
            "doctest": false,
            "test": true,
        },
        "message": {
            "level": "error",
            "message": message,
            "code": {
                "code": "boundaries::forbidden_dependency",
                "explanation": null,
            },
            "spans": spans,
            "children": [{
                "level": "note",
                "message": format!("imported symbol: `{}`", violation.symbol),
                "code": null,
                "spans": [],
                "children": [],
                "rendered": null,
            }],
            "rendered": format!(
                "error[boundaries]: {message}\n  --> imported symbol: `{}`",
                violation.symbol
            ),
        },
    })
}

pub(crate) fn emit_violations_json(
    violations: &[BoundaryViolation],
    repo_root: &Path,
) -> io::Result<()> {
    for violation in violations {
        let json = violation_to_cargo_diagnostic_json(violation, repo_root);
        println!("{json}");
        io::stdout().flush()?;
    }
    Ok(())
}

pub(crate) fn build_finished_json(success: bool) -> Value {
    json!({
        "reason": "build-finished",
        "success": success,
    })
}

pub(crate) fn emit_build_finished(success: bool) -> io::Result<()> {
    let json = build_finished_json(success);
    println!("{json}");
    io::stdout().flush()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn repo_root() -> PathBuf {
        PathBuf::from("/tmp/test-repo")
    }

    #[test]
    fn violation_produces_cargo_compatible_diagnostic_json() {
        let violation = BoundaryViolation {
            source_boundary: "diagrams".to_string(),
            target_boundary: "engines".to_string(),
            symbol: "crate::EngineAlgorithmId".to_string(),
            file: Some("src/diagrams/flowchart/compiler.rs".to_string()),
            line: Some(5),
            column: Some(5),
            line_text: Some("use crate::EngineAlgorithmId;".to_string()),
            underline_offset: Some(4),
            underline_len: Some(24),
        };

        let json = violation_to_cargo_diagnostic_json(&violation, &repo_root());

        assert_eq!(json["reason"], "compiler-message");
        let message = &json["message"];
        assert_eq!(message["level"], "error");
        assert_eq!(message["code"]["code"], "boundaries::forbidden_dependency");
        assert!(message["message"].as_str().unwrap().contains("diagrams"));
        assert!(message["message"].as_str().unwrap().contains("engines"));

        let span = &message["spans"][0];
        assert_eq!(span["file_name"], "src/diagrams/flowchart/compiler.rs");
        assert_eq!(span["line_start"], 5);
        assert_eq!(span["line_end"], 5);
        assert_eq!(span["column_start"], 5);
        assert!(span["is_primary"].as_bool().unwrap());
        assert_eq!(span["label"], "forbidden dependency on `engines`");
    }

    #[test]
    fn violation_without_location_produces_valid_diagnostic() {
        let violation = BoundaryViolation {
            source_boundary: "diagrams".to_string(),
            target_boundary: "engines".to_string(),
            symbol: "crate::EngineAlgorithmId".to_string(),
            file: None,
            line: None,
            column: None,
            line_text: None,
            underline_offset: None,
            underline_len: None,
        };

        let json = violation_to_cargo_diagnostic_json(&violation, &repo_root());

        assert_eq!(json["reason"], "compiler-message");
        assert_eq!(json["message"]["level"], "error");
        assert_eq!(json["message"]["spans"], json!([]));
    }

    #[test]
    fn violation_children_contain_symbol_note() {
        let violation = BoundaryViolation {
            source_boundary: "diagrams".to_string(),
            target_boundary: "engines".to_string(),
            symbol: "crate::EngineAlgorithmId".to_string(),
            file: Some("src/foo.rs".to_string()),
            line: Some(1),
            column: Some(1),
            line_text: None,
            underline_offset: None,
            underline_len: None,
        };

        let json = violation_to_cargo_diagnostic_json(&violation, &repo_root());
        let children = &json["message"]["children"];

        assert_eq!(children[0]["level"], "note");
        assert!(
            children[0]["message"]
                .as_str()
                .unwrap()
                .contains("crate::EngineAlgorithmId")
        );
    }

    #[test]
    fn build_finished_json_contains_required_fields() {
        let json = build_finished_json(true);
        assert_eq!(json["reason"], "build-finished");
        assert_eq!(json["success"], true);

        let json = build_finished_json(false);
        assert_eq!(json["success"], false);
    }
}
