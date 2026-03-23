use std::fmt::Write;

use crate::architecture::boundaries::BoundaryGraph;
use crate::architecture::policy::ArchitecturePolicy;

/// Explain a specific edge between two boundaries.
pub(crate) fn explain_edge(
    graph: &BoundaryGraph,
    policy: &ArchitecturePolicy,
    source: &str,
    target: &str,
) -> String {
    let mut out = String::new();

    writeln!(out, "Edge: {source} -> {target}").unwrap();
    writeln!(out).unwrap();

    match graph.edge(source, target) {
        Some(edge) => {
            writeln!(out, "  Exists: yes").unwrap();
            writeln!(out, "  Provenance: {:?}", edge.provenance).unwrap();
            writeln!(out, "  Sample:").unwrap();
            writeln!(out, "    source: {}", edge.sample.source).unwrap();
            writeln!(out, "    symbol: {}", edge.sample.symbol).unwrap();
            writeln!(out, "    target: {}", edge.sample.target).unwrap();
            if let Some(loc) = &edge.sample.location {
                writeln!(
                    out,
                    "    location: {}:{}:{}",
                    loc.path, loc.line, loc.column
                )
                .unwrap();
            }
        }
        None => {
            writeln!(out, "  Exists: no (no dependency detected)").unwrap();
        }
    }

    // Show which rules govern this edge
    writeln!(out).unwrap();
    writeln!(out, "  Rules:").unwrap();
    let mut found_rule = false;
    for rule in &policy.rules {
        if rule_governs_edge(&rule.rule, source, target) {
            writeln!(out, "    - {} ({})", rule.id, rule_type_name(&rule.rule)).unwrap();
            found_rule = true;
        }
    }
    if !found_rule {
        writeln!(out, "    (none)").unwrap();
    }

    // Show any exceptions for this edge
    let matching_exceptions: Vec<_> = policy
        .exceptions
        .iter()
        .filter(|exc| exc.source == source && exc.target == target)
        .collect();
    if !matching_exceptions.is_empty() {
        writeln!(out).unwrap();
        writeln!(out, "  Exceptions:").unwrap();
        for exc in matching_exceptions {
            writeln!(
                out,
                "    - {} ({}, owner: {})",
                exc.id, exc.rule_id, exc.owner
            )
            .unwrap();
            if !exc.reason.is_empty() {
                writeln!(out, "      reason: {}", exc.reason).unwrap();
            }
        }
    }

    out
}

/// Explain a specific boundary: what it depends on and what depends on it.
pub(crate) fn explain_boundary(
    graph: &BoundaryGraph,
    policy: &ArchitecturePolicy,
    name: &str,
) -> String {
    let mut out = String::new();

    writeln!(out, "Boundary: {name}").unwrap();
    writeln!(out).unwrap();

    if !graph.boundaries.contains(name) {
        writeln!(out, "  Not found in the boundary graph.").unwrap();
        return out;
    }

    // Show tags if present
    if let Some(spec) = policy.modules.get(name)
        && !spec.tags.is_empty()
    {
        writeln!(out, "  Tags:").unwrap();
        for (key, value) in &spec.tags {
            writeln!(out, "    {key}: {value}").unwrap();
        }
        writeln!(out).unwrap();
    }

    // Outgoing edges (what this boundary depends on)
    let outgoing: Vec<_> = graph
        .edges
        .iter()
        .filter(|((source, _), _)| source == name)
        .collect();
    writeln!(out, "  Depends on ({}):", outgoing.len()).unwrap();
    for ((_, target), edge) in &outgoing {
        writeln!(out, "    -> {target} ({:?})", edge.provenance).unwrap();
    }

    // Incoming edges (what depends on this boundary)
    let incoming: Vec<_> = graph
        .edges
        .iter()
        .filter(|((_, target), _)| target == name)
        .collect();
    writeln!(out).unwrap();
    writeln!(out, "  Depended on by ({}):", incoming.len()).unwrap();
    for ((source, _), edge) in &incoming {
        writeln!(out, "    <- {source} ({:?})", edge.provenance).unwrap();
    }

    // Show which rules mention this boundary
    let governing: Vec<_> = policy
        .rules
        .iter()
        .filter(|c| rule_mentions_boundary(&c.rule, name))
        .collect();
    if !governing.is_empty() {
        writeln!(out).unwrap();
        writeln!(out, "  Rules:").unwrap();
        for rule in governing {
            writeln!(out, "    - {} ({})", rule.id, rule_type_name(&rule.rule)).unwrap();
        }
    }

    out
}

fn rule_governs_edge(
    rule: &crate::architecture::policy::RuleKind,
    source: &str,
    target: &str,
) -> bool {
    use crate::architecture::policy::RuleKind;
    match rule {
        RuleKind::Allow(a) => a.source == source,
        RuleKind::Layers(l) => {
            l.order.iter().any(|b| b == source) && l.order.iter().any(|b| b == target)
        }
        RuleKind::Protected(p) => p.targets.iter().any(|t| t == target),
        RuleKind::Independence(i) => {
            i.members.iter().any(|m| m == source) && i.members.iter().any(|m| m == target)
        }
        RuleKind::Acyclic(a) => {
            a.members.iter().any(|m| m == source) && a.members.iter().any(|m| m == target)
        }
    }
}

fn rule_mentions_boundary(rule: &crate::architecture::policy::RuleKind, name: &str) -> bool {
    use crate::architecture::policy::RuleKind;
    match rule {
        RuleKind::Allow(a) => a.source == name || a.allowed.iter().any(|d| d == name),
        RuleKind::Layers(l) => l.order.iter().any(|b| b == name),
        RuleKind::Protected(p) => {
            p.targets.iter().any(|t| t == name) || p.allowed_importers.iter().any(|i| i == name)
        }
        RuleKind::Independence(i) => i.members.iter().any(|m| m == name),
        RuleKind::Acyclic(a) => a.members.iter().any(|m| m == name),
    }
}

fn rule_type_name(rule: &crate::architecture::policy::RuleKind) -> &'static str {
    use crate::architecture::policy::RuleKind;
    match rule {
        RuleKind::Allow(_) => "allow",
        RuleKind::Layers(_) => "layers",
        RuleKind::Protected(_) => "protected",
        RuleKind::Independence(_) => "independence",
        RuleKind::Acyclic(_) => "acyclic",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;
    use crate::architecture::boundaries::{DependencySample, EdgeProvenance};
    use crate::architecture::policy::{AllowRule, ExceptionSpec, ModuleSpec, RuleKind, RuleSpec};

    fn test_graph() -> BoundaryGraph {
        let boundaries = BTreeSet::from([
            "graph".to_string(),
            "errors".to_string(),
            "render".to_string(),
        ]);
        let mut graph = BoundaryGraph::new(boundaries);
        graph.insert_edge(
            "graph".to_string(),
            "errors".to_string(),
            DependencySample {
                source: "crate::graph".to_string(),
                symbol: "crate::errors::RenderError".to_string(),
                target: "crate::errors".to_string(),
                location: None,
            },
            EdgeProvenance::ModuleScope,
        );
        graph.insert_edge(
            "render".to_string(),
            "graph".to_string(),
            DependencySample {
                source: "crate::render".to_string(),
                symbol: "crate::graph::Node".to_string(),
                target: "crate::graph".to_string(),
                location: None,
            },
            EdgeProvenance::QualifiedPath,
        );
        graph
    }

    fn test_policy() -> ArchitecturePolicy {
        ArchitecturePolicy {
            version: 1,
            modules: BTreeMap::from([
                ("graph".to_string(), ModuleSpec::default()),
                ("errors".to_string(), ModuleSpec::default()),
                ("render".to_string(), ModuleSpec::default()),
            ]),
            rules: vec![RuleSpec {
                id: "allow-graph".to_string(),
                rule: RuleKind::Allow(AllowRule {
                    source: "graph".to_string(),
                    allowed: vec!["errors".to_string()],
                }),
            }],
            exceptions: vec![],
        }
    }

    #[test]
    fn explain_edge_existing() {
        let output = explain_edge(&test_graph(), &test_policy(), "graph", "errors");
        assert!(output.contains("Exists: yes"), "got:\n{output}");
        assert!(output.contains("ModuleScope"), "got:\n{output}");
        assert!(output.contains("allow-graph"), "got:\n{output}");
    }

    #[test]
    fn explain_edge_nonexistent() {
        let output = explain_edge(&test_graph(), &test_policy(), "errors", "render");
        assert!(output.contains("Exists: no"), "got:\n{output}");
    }

    #[test]
    fn explain_edge_shows_exceptions() {
        let mut policy = test_policy();
        policy.exceptions.push(ExceptionSpec {
            id: "legacy-coupling".to_string(),
            rule_id: "allow-graph".to_string(),
            source: "graph".to_string(),
            target: "errors".to_string(),
            reason: "historical".to_string(),
            owner: "kevin".to_string(),
        });
        let output = explain_edge(&test_graph(), &policy, "graph", "errors");
        assert!(output.contains("legacy-coupling"), "got:\n{output}");
        assert!(output.contains("historical"), "got:\n{output}");
    }

    #[test]
    fn explain_boundary_shows_edges() {
        let output = explain_boundary(&test_graph(), &test_policy(), "graph");
        assert!(output.contains("Depends on (1):"), "got:\n{output}");
        assert!(output.contains("-> errors"), "got:\n{output}");
        assert!(output.contains("Depended on by (1):"), "got:\n{output}");
        assert!(output.contains("<- render"), "got:\n{output}");
    }

    #[test]
    fn explain_boundary_shows_rules() {
        let output = explain_boundary(&test_graph(), &test_policy(), "graph");
        assert!(output.contains("allow-graph"), "got:\n{output}");
    }

    #[test]
    fn explain_boundary_not_found() {
        let output = explain_boundary(&test_graph(), &test_policy(), "nonexistent");
        assert!(output.contains("Not found"), "got:\n{output}");
    }
}
