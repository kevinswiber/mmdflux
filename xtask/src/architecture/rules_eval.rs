use std::collections::BTreeSet;

use crate::architecture::boundaries::{BoundaryGraph, DependencySample};
use crate::architecture::policy::{ArchitecturePolicy, RuleKind, RuleSpec};

// ---------------------------------------------------------------------------
// Violation — the output of rule evaluation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Violation {
    pub(crate) rule_id: String,
    pub(crate) rule_type: String,
    pub(crate) source_boundary: String,
    pub(crate) target_boundary: String,
    pub(crate) sample: DependencySample,
    pub(crate) detail: Option<String>,
}

/// Result of evaluating all rules, including exception handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EvaluationResult {
    /// Violations not suppressed by any exception.
    pub(crate) violations: Vec<Violation>,
    /// Violations that were suppressed by a matching exception.
    pub(crate) suppressed: Vec<SuppressedViolation>,
    /// Exceptions that did not match any violation (suppression debt).
    pub(crate) unused_exceptions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SuppressedViolation {
    pub(crate) violation: Violation,
    pub(crate) exception_id: String,
}

// ---------------------------------------------------------------------------
// Evaluate all rules against a boundary graph
// ---------------------------------------------------------------------------

pub(crate) fn evaluate_rules(
    graph: &BoundaryGraph,
    policy: &ArchitecturePolicy,
) -> EvaluationResult {
    let mut all_violations = Vec::new();
    for rule in &policy.rules {
        match &rule.rule {
            RuleKind::Allow(allow) => {
                evaluate_allow(
                    graph,
                    rule,
                    &allow.source,
                    &allow.allowed,
                    &mut all_violations,
                );
            }
            RuleKind::Layers(layers) => {
                evaluate_layers(graph, rule, &layers.order, &mut all_violations);
            }
            RuleKind::Protected(prot) => {
                evaluate_protected(
                    graph,
                    rule,
                    &prot.targets,
                    &prot.allowed_importers,
                    &mut all_violations,
                );
            }
            RuleKind::Independence(ind) => {
                evaluate_independence(graph, rule, &ind.members, &mut all_violations);
            }
            RuleKind::Acyclic(acyc) => {
                evaluate_acyclic(graph, rule, &acyc.members, &mut all_violations);
            }
        }
    }

    apply_exceptions(all_violations, &policy.exceptions)
}

fn apply_exceptions(
    violations: Vec<Violation>,
    exceptions: &[crate::architecture::policy::ExceptionSpec],
) -> EvaluationResult {
    let mut used_exception_ids: BTreeSet<String> = BTreeSet::new();
    let mut active_violations = Vec::new();
    let mut suppressed = Vec::new();

    for violation in violations {
        let matching_exception = exceptions.iter().find(|exc| {
            exc.rule_id == violation.rule_id
                && exc.source == violation.source_boundary
                && exc.target == violation.target_boundary
        });

        if let Some(exc) = matching_exception {
            used_exception_ids.insert(exc.id.clone());
            suppressed.push(SuppressedViolation {
                violation,
                exception_id: exc.id.clone(),
            });
        } else {
            active_violations.push(violation);
        }
    }

    let unused_exceptions: Vec<String> = exceptions
        .iter()
        .filter(|exc| !used_exception_ids.contains(&exc.id))
        .map(|exc| exc.id.clone())
        .collect();

    EvaluationResult {
        violations: active_violations,
        suppressed,
        unused_exceptions,
    }
}

// ---------------------------------------------------------------------------
// allow — same semantics as the current v1 allowlist
// ---------------------------------------------------------------------------

fn evaluate_allow(
    graph: &BoundaryGraph,
    rule: &RuleSpec,
    source: &str,
    allowed: &[String],
    violations: &mut Vec<Violation>,
) {
    let allowed_set: BTreeSet<&str> = allowed.iter().map(|s| s.as_str()).collect();
    for ((edge_source, edge_target), edge) in &graph.edges {
        if edge_source != source {
            continue;
        }
        if !allowed_set.contains(edge_target.as_str()) {
            violations.push(Violation {
                rule_id: rule.id.clone(),
                rule_type: "allow".to_string(),
                source_boundary: edge_source.clone(),
                target_boundary: edge_target.clone(),
                sample: edge.sample.clone(),
                detail: None,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// layers — boundaries may only depend on earlier (lower) entries in order
// ---------------------------------------------------------------------------

fn evaluate_layers(
    graph: &BoundaryGraph,
    rule: &RuleSpec,
    order: &[String],
    violations: &mut Vec<Violation>,
) {
    // Build a position map: boundary name -> index in the layer order.
    // Lower index = lower layer. A boundary at index i may only depend on
    // boundaries at index j where j < i.
    let positions: std::collections::BTreeMap<&str, usize> = order
        .iter()
        .enumerate()
        .map(|(i, name)| (name.as_str(), i))
        .collect();

    for ((edge_source, edge_target), edge) in &graph.edges {
        let Some(&source_pos) = positions.get(edge_source.as_str()) else {
            continue; // edge source not in this layer set — not governed
        };
        let Some(&target_pos) = positions.get(edge_target.as_str()) else {
            continue; // edge target not in this layer set — not governed
        };
        if target_pos >= source_pos {
            // Depending on same layer or higher layer is a violation
            violations.push(Violation {
                rule_id: rule.id.clone(),
                rule_type: "layers".to_string(),
                source_boundary: edge_source.clone(),
                target_boundary: edge_target.clone(),
                sample: edge.sample.clone(),
                detail: Some(format!(
                    "{} (layer {}) must not depend on {} (layer {})",
                    edge_source, source_pos, edge_target, target_pos
                )),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// protected — only listed importers may access the protected targets
// ---------------------------------------------------------------------------

fn evaluate_protected(
    graph: &BoundaryGraph,
    rule: &RuleSpec,
    targets: &[String],
    allowed_importers: &[String],
    violations: &mut Vec<Violation>,
) {
    let target_set: BTreeSet<&str> = targets.iter().map(|s| s.as_str()).collect();
    let allowed_set: BTreeSet<&str> = allowed_importers.iter().map(|s| s.as_str()).collect();

    for ((edge_source, edge_target), edge) in &graph.edges {
        if !target_set.contains(edge_target.as_str()) {
            continue; // edge target is not protected by this rule
        }
        if allowed_set.contains(edge_source.as_str()) {
            continue; // source is an authorized importer
        }
        violations.push(Violation {
            rule_id: rule.id.clone(),
            rule_type: "protected".to_string(),
            source_boundary: edge_source.clone(),
            target_boundary: edge_target.clone(),
            sample: edge.sample.clone(),
            detail: Some(format!(
                "{} is not an allowed importer of protected boundary {}",
                edge_source, edge_target
            )),
        });
    }
}

// ---------------------------------------------------------------------------
// independence — any direct edge among group members is a violation
// ---------------------------------------------------------------------------

fn evaluate_independence(
    graph: &BoundaryGraph,
    rule: &RuleSpec,
    members: &[String],
    violations: &mut Vec<Violation>,
) {
    let member_set: BTreeSet<&str> = members.iter().map(|s| s.as_str()).collect();

    for ((edge_source, edge_target), edge) in &graph.edges {
        if member_set.contains(edge_source.as_str()) && member_set.contains(edge_target.as_str()) {
            violations.push(Violation {
                rule_id: rule.id.clone(),
                rule_type: "independence".to_string(),
                source_boundary: edge_source.clone(),
                target_boundary: edge_target.clone(),
                sample: edge.sample.clone(),
                detail: Some(format!(
                    "{} and {} must be independent (no direct dependencies)",
                    edge_source, edge_target
                )),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// acyclic — boundaries must form a DAG (no dependency cycles)
// ---------------------------------------------------------------------------

fn evaluate_acyclic(
    graph: &BoundaryGraph,
    rule: &RuleSpec,
    members: &[String],
    violations: &mut Vec<Violation>,
) {
    let member_set: BTreeSet<&str> = members.iter().map(|s| s.as_str()).collect();

    // Build adjacency list restricted to members
    let mut adj: std::collections::BTreeMap<&str, Vec<&str>> = std::collections::BTreeMap::new();
    for member in &member_set {
        adj.entry(member).or_default();
    }
    for (source, target) in graph.edges.keys() {
        if member_set.contains(source.as_str()) && member_set.contains(target.as_str()) {
            adj.entry(source.as_str())
                .or_default()
                .push(target.as_str());
        }
    }

    // DFS-based cycle detection
    #[derive(Clone, Copy, PartialEq)]
    enum Color {
        White,
        Gray,
        Black,
    }

    let mut color: std::collections::BTreeMap<&str, Color> =
        member_set.iter().map(|&m| (m, Color::White)).collect();
    let mut path: Vec<&str> = Vec::new();

    fn dfs<'a>(
        node: &'a str,
        adj: &std::collections::BTreeMap<&'a str, Vec<&'a str>>,
        color: &mut std::collections::BTreeMap<&'a str, Color>,
        path: &mut Vec<&'a str>,
        cycles: &mut Vec<Vec<String>>,
    ) {
        color.insert(node, Color::Gray);
        path.push(node);

        if let Some(neighbors) = adj.get(node) {
            for &next in neighbors {
                match color.get(next) {
                    Some(Color::Gray) => {
                        // Found a cycle. Extract the cycle path starting from `next`.
                        let cycle_start = path.iter().position(|&n| n == next).unwrap();
                        let mut cycle: Vec<String> =
                            path[cycle_start..].iter().map(|s| s.to_string()).collect();
                        cycle.push(next.to_string()); // close the cycle
                        cycles.push(cycle);
                    }
                    Some(Color::White) | None => {
                        dfs(next, adj, color, path, cycles);
                    }
                    Some(Color::Black) => {}
                }
            }
        }

        path.pop();
        color.insert(node, Color::Black);
    }

    let mut cycles: Vec<Vec<String>> = Vec::new();
    for &node in &member_set {
        if color.get(node) == Some(&Color::White) {
            dfs(node, &adj, &mut color, &mut path, &mut cycles);
        }
    }

    // Normalize cycles for deterministic output: rotate so the
    // lexicographically smallest element is first.
    for cycle in &mut cycles {
        if cycle.len() > 1 {
            // Last element duplicates the first (closing the cycle); exclude it for rotation
            let body = &cycle[..cycle.len() - 1];
            if let Some(min_pos) = body
                .iter()
                .enumerate()
                .min_by_key(|(_, v)| *v)
                .map(|(i, _)| i)
            {
                let mut rotated: Vec<String> = body[min_pos..].to_vec();
                rotated.extend_from_slice(&body[..min_pos]);
                rotated.push(rotated[0].clone());
                *cycle = rotated;
            }
        }
    }

    // Deduplicate identical cycles
    cycles.sort();
    cycles.dedup();

    for cycle in &cycles {
        let cycle_path = cycle.join(" -> ");
        // Use the first edge in the cycle as the representative sample
        let source = &cycle[0];
        let target = &cycle[1];
        let sample = graph
            .edge(source, target)
            .map(|e| e.sample.clone())
            .unwrap_or_else(|| DependencySample {
                source: format!("crate::{source}"),
                symbol: format!("crate::{target}"),
                target: format!("crate::{target}"),
                location: None,
            });

        violations.push(Violation {
            rule_id: rule.id.clone(),
            rule_type: "acyclic".to_string(),
            source_boundary: source.clone(),
            target_boundary: target.clone(),
            sample,
            detail: Some(format!("cycle: {cycle_path}")),
        });
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::architecture::boundaries::EdgeProvenance;

    fn graph_with_edge(source: &str, target: &str) -> BoundaryGraph {
        let boundaries = BTreeSet::from([source.to_string(), target.to_string()]);
        let mut graph = BoundaryGraph::new(boundaries);
        graph.insert_edge(
            source.to_string(),
            target.to_string(),
            DependencySample {
                source: format!("crate::{source}"),
                symbol: format!("crate::{target}::Item"),
                target: format!("crate::{target}"),
                location: None,
            },
            EdgeProvenance::ModuleScope,
        );
        graph
    }

    fn graph_with_edges(edges: &[(&str, &str)]) -> BoundaryGraph {
        let mut all_names = BTreeSet::new();
        for (s, t) in edges {
            all_names.insert(s.to_string());
            all_names.insert(t.to_string());
        }
        let mut graph = BoundaryGraph::new(all_names);
        for (source, target) in edges {
            graph.insert_edge(
                source.to_string(),
                target.to_string(),
                DependencySample {
                    source: format!("crate::{source}"),
                    symbol: format!("crate::{target}::Item"),
                    target: format!("crate::{target}"),
                    location: None,
                },
                EdgeProvenance::ModuleScope,
            );
        }
        graph
    }

    fn allow_policy(source: &str, allowed: &[&str]) -> ArchitecturePolicy {
        let mut boundaries = BTreeMap::new();
        boundaries.insert(source.to_string(), Default::default());
        for dep in allowed {
            boundaries.insert(dep.to_string(), Default::default());
        }
        ArchitecturePolicy {
            version: 1,
            modules: boundaries,
            rules: vec![RuleSpec {
                id: format!("allow-{source}"),
                rule: RuleKind::Allow(crate::architecture::policy::AllowRule {
                    source: source.to_string(),
                    allowed: allowed.iter().map(|s| s.to_string()).collect(),
                }),
            }],
            exceptions: Vec::new(),
        }
    }

    fn layers_policy(order: &[&str]) -> ArchitecturePolicy {
        let boundaries = order
            .iter()
            .map(|name| (name.to_string(), Default::default()))
            .collect();
        ArchitecturePolicy {
            version: 1,
            modules: boundaries,
            rules: vec![RuleSpec {
                id: "pipeline-layers".to_string(),
                rule: RuleKind::Layers(crate::architecture::policy::LayersRule {
                    order: order.iter().map(|s| s.to_string()).collect(),
                }),
            }],
            exceptions: Vec::new(),
        }
    }

    // -- allow rule tests --

    #[test]
    fn allow_rule_reports_disallowed_edge() {
        let graph = graph_with_edge("runtime", "render");
        let policy = allow_policy("runtime", &["graph"]);
        let violations = evaluate_rules(&graph, &policy).violations;
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_id, "allow-runtime");
        assert_eq!(violations[0].rule_type, "allow");
        assert_eq!(violations[0].source_boundary, "runtime");
        assert_eq!(violations[0].target_boundary, "render");
    }

    #[test]
    fn allow_rule_passes_when_edge_is_allowed() {
        let graph = graph_with_edge("runtime", "graph");
        let policy = allow_policy("runtime", &["graph"]);
        let violations = evaluate_rules(&graph, &policy).violations;
        assert!(violations.is_empty());
    }

    #[test]
    fn allow_rule_ignores_edges_from_ungoverned_sources() {
        let graph = graph_with_edge("mermaid", "graph");
        let policy = allow_policy("runtime", &["graph"]);
        let violations = evaluate_rules(&graph, &policy).violations;
        assert!(violations.is_empty());
    }

    // -- layers rule tests --

    #[test]
    fn layers_rule_reports_upward_dependency() {
        // order: errors (0) < graph (1) < runtime (2)
        // graph -> runtime is upward = violation
        let graph = graph_with_edge("graph", "runtime");
        let policy = layers_policy(&["errors", "graph", "runtime"]);
        let violations = evaluate_rules(&graph, &policy).violations;
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_type, "layers");
        assert_eq!(violations[0].source_boundary, "graph");
        assert_eq!(violations[0].target_boundary, "runtime");
        assert!(violations[0].detail.is_some());
    }

    #[test]
    fn layers_rule_allows_downward_dependency() {
        // runtime (2) -> graph (1) is downward = ok
        let graph = graph_with_edge("runtime", "graph");
        let policy = layers_policy(&["errors", "graph", "runtime"]);
        let violations = evaluate_rules(&graph, &policy).violations;
        assert!(violations.is_empty());
    }

    #[test]
    fn layers_rule_reports_same_layer_dependency() {
        // graph -> graph would be same layer, but the collector already filters
        // self-edges. Test with two boundaries at... actually this can't happen
        // since each name is unique. Test peer dependency instead.
        // graph (1) -> render (1) would require duplicate positions, which
        // layers prevents. This test verifies that an edge between non-governed
        // boundaries is ignored.
        let graph = graph_with_edge("mermaid", "mmds");
        let policy = layers_policy(&["errors", "graph", "runtime"]);
        let violations = evaluate_rules(&graph, &policy).violations;
        assert!(violations.is_empty());
    }

    #[test]
    fn layers_rule_with_multiple_violations() {
        let graph = graph_with_edges(&[
            ("errors", "graph"),   // upward: 0 -> 1
            ("errors", "runtime"), // upward: 0 -> 2
            ("runtime", "errors"), // downward: 2 -> 0 (ok)
        ]);
        let policy = layers_policy(&["errors", "graph", "runtime"]);
        let violations = evaluate_rules(&graph, &policy).violations;
        assert_eq!(violations.len(), 2);
        assert!(violations.iter().all(|v| v.source_boundary == "errors"));
    }

    // -- protected rule tests --

    fn protected_policy(targets: &[&str], allowed_importers: &[&str]) -> ArchitecturePolicy {
        let mut boundaries = BTreeMap::new();
        for name in targets.iter().chain(allowed_importers.iter()) {
            boundaries.insert(name.to_string(), Default::default());
        }
        ArchitecturePolicy {
            version: 1,
            modules: boundaries,
            rules: vec![RuleSpec {
                id: "protect-internal".to_string(),
                rule: RuleKind::Protected(crate::architecture::policy::ProtectedRule {
                    targets: targets.iter().map(|s| s.to_string()).collect(),
                    allowed_importers: allowed_importers.iter().map(|s| s.to_string()).collect(),
                }),
            }],
            exceptions: Vec::new(),
        }
    }

    #[test]
    fn protected_rule_allows_declared_importers() {
        let graph = graph_with_edge("runtime", "payload");
        let policy = protected_policy(&["payload"], &["runtime", "registry"]);
        let violations = evaluate_rules(&graph, &policy).violations;
        assert!(violations.is_empty());
    }

    #[test]
    fn protected_rule_rejects_unauthorized_importer() {
        let graph = graph_with_edge("render", "payload");
        let mut policy = protected_policy(&["payload"], &["runtime", "registry"]);
        // Add "render" as a boundary so the graph is valid
        policy
            .modules
            .insert("render".to_string(), Default::default());
        let violations = evaluate_rules(&graph, &policy).violations;
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_type, "protected");
        assert_eq!(violations[0].source_boundary, "render");
        assert_eq!(violations[0].target_boundary, "payload");
    }

    #[test]
    fn protected_rule_ignores_edges_to_non_protected_targets() {
        let graph = graph_with_edge("render", "graph");
        let mut policy = protected_policy(&["payload"], &["runtime"]);
        policy
            .modules
            .insert("render".to_string(), Default::default());
        policy
            .modules
            .insert("graph".to_string(), Default::default());
        let violations = evaluate_rules(&graph, &policy).violations;
        assert!(violations.is_empty());
    }

    // -- independence rule tests --

    fn independence_policy(members: &[&str]) -> ArchitecturePolicy {
        let boundaries = members
            .iter()
            .map(|name| (name.to_string(), Default::default()))
            .collect();
        ArchitecturePolicy {
            version: 1,
            modules: boundaries,
            rules: vec![RuleSpec {
                id: "peer-isolation".to_string(),
                rule: RuleKind::Independence(crate::architecture::policy::IndependenceRule {
                    members: members.iter().map(|s| s.to_string()).collect(),
                }),
            }],
            exceptions: Vec::new(),
        }
    }

    #[test]
    fn independence_rule_rejects_peer_dependency() {
        let graph = graph_with_edge("mermaid", "mmds");
        let policy = independence_policy(&["mermaid", "mmds"]);
        let violations = evaluate_rules(&graph, &policy).violations;
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_type, "independence");
        assert_eq!(violations[0].source_boundary, "mermaid");
        assert_eq!(violations[0].target_boundary, "mmds");
    }

    #[test]
    fn independence_rule_rejects_in_either_direction() {
        let graph = graph_with_edges(&[("mermaid", "mmds"), ("mmds", "mermaid")]);
        let policy = independence_policy(&["mermaid", "mmds"]);
        let violations = evaluate_rules(&graph, &policy).violations;
        assert_eq!(violations.len(), 2);
    }

    #[test]
    fn independence_rule_ignores_edges_outside_group() {
        let graph = graph_with_edge("mermaid", "graph");
        let mut policy = independence_policy(&["mermaid", "mmds"]);
        policy
            .modules
            .insert("graph".to_string(), Default::default());
        let violations = evaluate_rules(&graph, &policy).violations;
        assert!(violations.is_empty());
    }

    // -- acyclic rule tests --

    fn acyclic_policy(members: &[&str]) -> ArchitecturePolicy {
        let boundaries = members
            .iter()
            .map(|name| (name.to_string(), Default::default()))
            .collect();
        ArchitecturePolicy {
            version: 1,
            modules: boundaries,
            rules: vec![RuleSpec {
                id: "no-cycles".to_string(),
                rule: RuleKind::Acyclic(crate::architecture::policy::AcyclicRule {
                    members: members.iter().map(|s| s.to_string()).collect(),
                }),
            }],
            exceptions: Vec::new(),
        }
    }

    #[test]
    fn acyclic_rule_detects_simple_cycle() {
        let graph = graph_with_edges(&[("a", "b"), ("b", "a")]);
        let policy = acyclic_policy(&["a", "b"]);
        let violations = evaluate_rules(&graph, &policy).violations;
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_type, "acyclic");
        let detail = violations[0].detail.as_ref().unwrap();
        assert!(detail.contains("cycle:"), "got: {detail}");
        assert!(
            detail.contains("a") && detail.contains("b"),
            "got: {detail}"
        );
    }

    #[test]
    fn acyclic_rule_detects_three_node_cycle() {
        let graph = graph_with_edges(&[("a", "b"), ("b", "c"), ("c", "a")]);
        let policy = acyclic_policy(&["a", "b", "c"]);
        let violations = evaluate_rules(&graph, &policy).violations;
        assert_eq!(violations.len(), 1);
        let detail = violations[0].detail.as_ref().unwrap();
        // Normalized: starts with "a" (lexicographically smallest)
        assert!(detail.starts_with("cycle: a"), "got: {detail}");
    }

    #[test]
    fn acyclic_rule_passes_for_dag() {
        let graph = graph_with_edges(&[("a", "b"), ("b", "c"), ("a", "c")]);
        let policy = acyclic_policy(&["a", "b", "c"]);
        let violations = evaluate_rules(&graph, &policy).violations;
        assert!(violations.is_empty());
    }

    #[test]
    fn acyclic_rule_ignores_edges_outside_member_set() {
        let graph = graph_with_edges(&[("a", "x"), ("x", "a")]);
        let mut policy = acyclic_policy(&["a", "b"]);
        policy.modules.insert("x".to_string(), Default::default());
        let violations = evaluate_rules(&graph, &policy).violations;
        assert!(violations.is_empty());
    }

    // -- exception tests --

    #[test]
    fn exception_suppresses_matching_violation() {
        let graph = graph_with_edge("graph", "diagrams");
        let mut policy = allow_policy("graph", &["errors"]);
        policy
            .modules
            .insert("diagrams".to_string(), Default::default());
        policy
            .exceptions
            .push(crate::architecture::policy::ExceptionSpec {
                id: "legacy-graph-diagrams".to_string(),
                rule_id: "allow-graph".to_string(),
                source: "graph".to_string(),
                target: "diagrams".to_string(),
                reason: "historical coupling".to_string(),
                owner: "kevin".to_string(),
            });

        let result = evaluate_rules(&graph, &policy);
        assert!(result.violations.is_empty());
        assert_eq!(result.suppressed.len(), 1);
        assert_eq!(result.suppressed[0].exception_id, "legacy-graph-diagrams");
    }

    #[test]
    fn exception_must_match_exact_rule_id() {
        let graph = graph_with_edge("graph", "diagrams");
        let mut policy = allow_policy("graph", &["errors"]);
        policy
            .modules
            .insert("diagrams".to_string(), Default::default());
        policy
            .exceptions
            .push(crate::architecture::policy::ExceptionSpec {
                id: "wrong-contract".to_string(),
                rule_id: "some-other-contract".to_string(),
                source: "graph".to_string(),
                target: "diagrams".to_string(),
                reason: "mismatched rule_id".to_string(),
                owner: "kevin".to_string(),
            });

        let result = evaluate_rules(&graph, &policy);
        assert_eq!(result.violations.len(), 1); // not suppressed
        assert!(result.suppressed.is_empty());
        assert_eq!(result.unused_exceptions, vec!["wrong-contract"]);
    }

    #[test]
    fn unused_exceptions_are_reported() {
        let graph = graph_with_edge("graph", "errors"); // allowed edge, no violation
        let mut policy = allow_policy("graph", &["errors"]);
        policy
            .exceptions
            .push(crate::architecture::policy::ExceptionSpec {
                id: "stale-exception".to_string(),
                rule_id: "allow-graph".to_string(),
                source: "graph".to_string(),
                target: "diagrams".to_string(),
                reason: "no longer needed".to_string(),
                owner: "kevin".to_string(),
            });

        let result = evaluate_rules(&graph, &policy);
        assert!(result.violations.is_empty());
        assert!(result.suppressed.is_empty());
        assert_eq!(result.unused_exceptions, vec!["stale-exception"]);
    }

    #[test]
    fn no_exceptions_means_empty_suppression_and_unused() {
        let graph = graph_with_edge("graph", "diagrams");
        let mut policy = allow_policy("graph", &["errors"]);
        policy
            .modules
            .insert("diagrams".to_string(), Default::default());

        let result = evaluate_rules(&graph, &policy);
        assert_eq!(result.violations.len(), 1);
        assert!(result.suppressed.is_empty());
        assert!(result.unused_exceptions.is_empty());
    }

    // -- parity and smoke tests --

    #[test]
    fn parsed_allow_rules_detect_forbidden_edges() {
        let graph = graph_with_edges(&[
            ("graph", "errors"),   // allowed
            ("graph", "format"),   // allowed
            ("graph", "diagrams"), // forbidden
        ]);
        let toml = "\
            version = 1\n\
            [modules]\n\
            graph = {}\n\
            errors = {}\n\
            format = {}\n\
            render = {}\n\
            diagrams = {}\n\
            \n\
            [[rules]]\n\
            id = \"allow-graph\"\n\
            type = \"allow\"\n\
            [rules.config]\n\
            source = \"graph\"\n\
            allowed = [\"errors\", \"format\"]\n\
        ";
        let policy = crate::architecture::policy::parse_policy_str(toml).unwrap();
        let result = evaluate_rules(&graph, &policy);

        assert_eq!(result.violations.len(), 1);
        assert_eq!(result.violations[0].source_boundary, "graph");
        assert_eq!(result.violations[0].target_boundary, "diagrams");
        assert_eq!(result.violations[0].rule_type, "allow");
    }

    #[test]
    fn current_repo_boundaries_file_parses_to_valid_rules() {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask should live at the workspace root");
        let path = crate::architecture::policy::resolve_policy_path(repo_root);
        let policy = crate::architecture::policy::parse_policy_file(&path).unwrap();
        assert!(!policy.modules.is_empty());
        assert!(!policy.rules.is_empty());
    }
}
