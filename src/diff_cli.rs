//! `mmdflux diff`: compare two diagrams and print the snapshot diff.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use clap::{Args, ValueEnum};
use mmdflux::graph::GeometryLevel;
use mmdflux::mmds::Document;
use mmdflux::mmds::diff::diff_documents;
use mmdflux::mmds::diff::wire::{self, WireLayer, WireOptions};
use mmdflux::{RenderConfig, materialize_diagram};
use serde::Deserialize;
use serde_json::Value;

use crate::GeometryLevelArg;

/// Exit status for a failed comparison, as in diff(1).
pub(crate) const EXIT_TROUBLE: i32 = 2;

/// Compare two diagrams and print what changed.
///
/// Each input is Mermaid source or MMDS JSON, detected per input. Exit status
/// is 0 on success; with --exit-code it is 0 when nothing changed and 1 when
/// something did. A failed comparison exits with 2.
#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub(crate) struct DiffArgs {
    /// Before diagram file (`-` reads stdin)
    #[arg(required_unless_present = "pair", conflicts_with = "pair")]
    before: Option<PathBuf>,

    /// After diagram file (`-` reads stdin)
    #[arg(required_unless_present = "pair", conflicts_with = "pair")]
    after: Option<PathBuf>,

    /// JSON envelope `{"before": "<source>", "after": "<source>"}` carrying both
    /// inputs (`-` reads stdin)
    #[arg(long, value_name = "FILE")]
    pair: Option<PathBuf>,

    /// Output format
    #[arg(short = 'f', long, value_enum, default_value_t = DiffFormat::Json)]
    format: DiffFormat,

    /// Change layer to report
    #[arg(long, value_enum, default_value_t = DiffLayer::Model)]
    layer: DiffLayer,

    /// Geometry level used when materializing Mermaid inputs
    #[arg(long, value_enum, default_value_t = GeometryLevelArg::Layout)]
    geometry_level: GeometryLevelArg,

    /// Include each change's diagnostic evidence strings (not format-stable)
    #[arg(long)]
    evidence: bool,

    /// Exit with 1 when the diagrams differ and 0 when they don't
    #[arg(long)]
    exit_code: bool,

    /// Compare diagrams of different types instead of refusing
    #[arg(long)]
    force: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum DiffFormat {
    /// `mmdflux.diff.v1` JSON (default)
    Json,
    /// One line per change, after a count by category
    Summary,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum DiffLayer {
    /// Model changes only (default)
    Model,
    /// Geometry changes only
    Geometry,
    /// Model and geometry changes
    All,
}

impl From<DiffLayer> for WireLayer {
    fn from(layer: DiffLayer) -> Self {
        match layer {
            DiffLayer::Model => WireLayer::Model,
            DiffLayer::Geometry => WireLayer::Geometry,
            DiffLayer::All => WireLayer::All,
        }
    }
}

#[derive(Deserialize)]
struct PairEnvelope {
    before: String,
    after: String,
}

/// Run `mmdflux diff` and return the process exit status.
pub(crate) fn run(args: &DiffArgs) -> i32 {
    match compare(args) {
        Ok((output, changed)) => {
            println!("{output}");
            if args.exit_code && changed { 1 } else { 0 }
        }
        Err(message) => {
            eprintln!("Error: {message}");
            EXIT_TROUBLE
        }
    }
}

fn compare(args: &DiffArgs) -> Result<(String, bool), String> {
    let (before_source, after_source) = read_sources(args)?;
    let config = RenderConfig {
        geometry_level: GeometryLevel::from(args.geometry_level),
        ..RenderConfig::default()
    };
    let before = materialize("before", &before_source, &config)?;
    let after = materialize("after", &after_source, &config)?;

    if !args.force && before.metadata.diagram_type != after.metadata.diagram_type {
        return Err(format!(
            "cannot diff a {} diagram against a {} diagram; pass --force to compare anyway",
            before.metadata.diagram_type, after.metadata.diagram_type
        ));
    }

    let diff = diff_documents(&before, &after);
    let options = WireOptions::default()
        .with_layer(args.layer.into())
        .with_evidence(args.evidence);
    let wire = wire::to_json(&diff, &before, &after, &options);
    let changed = !wire["changes"]
        .as_array()
        .is_none_or(|changes| changes.is_empty());

    let output = match args.format {
        DiffFormat::Json => serde_json::to_string_pretty(&wire)
            .map_err(|error| format!("failed to serialize diff: {error}"))?,
        DiffFormat::Summary => summary(&wire),
    };
    Ok((output, changed))
}

fn read_sources(args: &DiffArgs) -> Result<(String, String), String> {
    if let Some(pair) = &args.pair {
        let raw = read_input("pair", pair)?;
        let envelope: PairEnvelope = serde_json::from_str(&raw).map_err(|error| {
            format!("pair: expected {{\"before\": ..., \"after\": ...}}: {error}")
        })?;
        return Ok((envelope.before, envelope.after));
    }

    let (Some(before), Some(after)) = (&args.before, &args.after) else {
        return Err("expected <BEFORE> and <AFTER>, or --pair".to_string());
    };
    if is_stdin(before) && is_stdin(after) {
        return Err("only one of <BEFORE> and <AFTER> can read stdin".to_string());
    }
    Ok((read_input("before", before)?, read_input("after", after)?))
}

fn is_stdin(path: &Path) -> bool {
    path.as_os_str() == "-"
}

fn read_input(side: &str, path: &Path) -> Result<String, String> {
    if is_stdin(path) {
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .map_err(|error| format!("{side}: failed to read stdin: {error}"))?;
        return Ok(buffer);
    }
    fs::read_to_string(path).map_err(|error| format!("{side}: {}: {error}", path.display()))
}

fn materialize(side: &str, source: &str, config: &RenderConfig) -> Result<Document, String> {
    materialize_diagram(source, config).map_err(|error| format!("{side}: {error}"))
}

/// Render the wire document as a count line followed by one line per change.
fn summary(wire: &Value) -> String {
    let summary = &wire["summary"];
    let changes = wire["changes"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut lines = vec![format!(
        "{} {}: {} added, {} removed, {} changed, {} moved",
        changes.len(),
        if changes.len() == 1 {
            "change"
        } else {
            "changes"
        },
        summary["added"],
        summary["removed"],
        summary["changed"],
        summary["moved"],
    )];
    lines.extend(changes.iter().map(summary_line));
    lines.join("\n")
}

fn summary_line(change: &Value) -> String {
    let marker = match change["category"].as_str() {
        Some("added") => "+",
        Some("removed") => "-",
        Some("moved") => ">",
        _ => "~",
    };
    let kind = change["kind"].as_str().unwrap_or_default();
    let subject = &change["subject"];
    let before = change.get("before");
    let after = change.get("after");

    let description = match subject["type"].as_str() {
        Some("edge") => {
            let id = subject["after_id"]
                .as_str()
                .or_else(|| subject["before_id"].as_str())
                .unwrap_or_default();
            match (before, after) {
                (Some(before), Some(after)) if before != after => {
                    format!("edge {id} {} -> {}", edge_text(before), edge_text(after))
                }
                (_, Some(value)) | (Some(value), None) => format!("edge {id} {}", edge_text(value)),
                (None, None) => format!("edge {id}"),
            }
        }
        Some(subject_type @ ("node" | "subgraph")) => {
            let id = subject["id"].as_str().unwrap_or_default();
            let name = if subject_type == "node" {
                "label"
            } else {
                "title"
            };
            // Show the field the change is about; otherwise name the entity.
            let field = match kind {
                "NodeLabelChanged" | "SubgraphTitleChanged" => Some(name),
                "NodeParentChanged" | "SubgraphParentChanged" => Some("parent"),
                "NodeShapeChanged" => Some("shape"),
                _ => None,
            };
            match (before, after, field) {
                (Some(before), Some(after), Some(field)) => format!(
                    "{subject_type} {id} {}: {} -> {}",
                    after[name], before[field], after[field]
                ),
                (_, Some(value), _) | (Some(value), None, _) => {
                    format!("{subject_type} {id} {}", value[name])
                }
                (None, None, _) => format!("{subject_type} {id}"),
            }
        }
        _ => {
            let mut text = "document".to_string();
            if let Some(namespace) = subject["extension_namespace"].as_str() {
                text.push(' ');
                text.push_str(namespace);
            }
            if let (Some(before), Some(after)) = (before, after) {
                text.push_str(&format!(
                    " {} -> {}",
                    inline_value(before),
                    inline_value(after)
                ));
            }
            text
        }
    };
    format!("{marker} {kind} {description}")
}

fn edge_text(value: &Value) -> String {
    let source = value["source"].as_str().unwrap_or_default();
    let target = value["target"].as_str().unwrap_or_default();
    match value["label"].as_str() {
        Some(label) => format!("{source} --> {target} {}", Value::from(label)),
        None => format!("{source} --> {target}"),
    }
}

/// A document change's single inline value, such as `"flowchart"` or `"LR"`.
fn inline_value(value: &Value) -> String {
    value
        .as_object()
        .and_then(|object| object.values().next())
        .map(Value::to_string)
        .unwrap_or_default()
}
