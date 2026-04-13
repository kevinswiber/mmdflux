use std::collections::{BTreeMap, BTreeSet};

use crate::architecture::boundaries::{BoundaryGraph, DependencySample};
use crate::architecture::policy::{self, ArchitecturePolicy, RuleKind, RuleSpec};

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

#[cfg(test)]
pub(crate) fn evaluate_rules(
    graph: &BoundaryGraph,
    policy: &ArchitecturePolicy,
) -> EvaluationResult {
    evaluate_rules_with_module_graph(graph, None, policy)
}

pub(crate) fn evaluate_rules_with_module_graph(
    graph: &BoundaryGraph,
    module_graph: Option<&BoundaryGraph>,
    policy: &ArchitecturePolicy,
) -> EvaluationResult {
    let mut all_violations = Vec::new();
    for rule in &policy.rules {
        match &rule.rule {
            RuleKind::Allow(allow) => {
                evaluate_allow(
                    graph,
                    module_graph,
                    rule,
                    &allow.source,
                    &allow.allowed,
                    &mut all_violations,
                );
            }
            RuleKind::Layers(layers) => {
                evaluate_layers(
                    graph,
                    module_graph,
                    rule,
                    &layers.order,
                    &mut all_violations,
                );
            }
            RuleKind::Protected(prot) => {
                evaluate_protected(
                    graph,
                    module_graph,
                    rule,
                    &prot.targets,
                    &prot.allowed_importers,
                    &mut all_violations,
                );
            }
            RuleKind::Independence(ind) => {
                evaluate_independence(graph, module_graph, rule, &ind.members, &mut all_violations);
            }
            RuleKind::Acyclic(acyc) => {
                evaluate_acyclic(
                    graph,
                    module_graph,
                    rule,
                    &acyc.members,
                    acyc.skip_parent_child,
                    &mut all_violations,
                );
            }
        }
    }

    apply_exceptions(all_violations, &policy.exceptions)
}

fn uses_module_path_selector(selector: &str) -> bool {
    selector.contains("::") || selector == policy::WILDCARD_ALL
}

fn list_uses_module_path_selectors(selectors: &[String]) -> bool {
    selectors
        .iter()
        .any(|selector| uses_module_path_selector(selector))
}

fn selector_matches(selector: &str, actual_path: &str) -> bool {
    actual_path == selector
        || actual_path
            .strip_prefix(selector)
            .is_some_and(|suffix| suffix.starts_with("::"))
}

fn longest_matching_selector(actual_path: &str, selectors: &[String]) -> Option<String> {
    selectors
        .iter()
        .filter(|selector| selector_matches(selector, actual_path))
        .max_by_key(|selector| selector.len())
        .cloned()
}

/// Returns true if one selector is a direct ancestor of the other
/// (e.g. `"foo"` and `"foo::bar"`, or `"a::b"` and `"a::b::c"`).
fn is_parent_child(a: &str, b: &str) -> bool {
    b.starts_with(a) && b[a.len()..].starts_with("::")
        || a.starts_with(b) && a[b.len()..].starts_with("::")
}

fn select_graph<'a>(
    graph: &'a BoundaryGraph,
    module_graph: Option<&'a BoundaryGraph>,
    uses_module_paths: bool,
) -> Option<&'a BoundaryGraph> {
    if uses_module_paths {
        // Prefer the module graph when available; fall back to the boundary
        // graph so that the "*" wildcard still works at boundary granularity
        // when no module-path selectors triggered a full module scan.
        module_graph.or(Some(graph))
    } else {
        Some(graph)
    }
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
    module_graph: Option<&BoundaryGraph>,
    rule: &RuleSpec,
    source: &str,
    allowed: &[String],
    violations: &mut Vec<Violation>,
) {
    let uses_module_paths =
        uses_module_path_selector(source) || list_uses_module_path_selectors(allowed);
    let Some(graph) = select_graph(graph, module_graph, uses_module_paths) else {
        return;
    };
    let allowed_set: BTreeSet<&str> = allowed.iter().map(|s| s.as_str()).collect();
    for ((edge_source, edge_target), edge) in &graph.edges {
        if (!uses_module_paths && edge_source != source)
            || (uses_module_paths && !selector_matches(source, edge_source))
        {
            continue;
        }
        let is_allowed = if uses_module_paths {
            allowed
                .iter()
                .any(|selector| selector_matches(selector, edge_target))
        } else {
            allowed_set.contains(edge_target.as_str())
        };
        if !is_allowed {
            violations.push(Violation {
                rule_id: rule.id.clone(),
                rule_type: "allow".to_string(),
                source_boundary: if uses_module_paths {
                    source.to_string()
                } else {
                    edge_source.clone()
                },
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
    module_graph: Option<&BoundaryGraph>,
    rule: &RuleSpec,
    order: &[String],
    violations: &mut Vec<Violation>,
) {
    let uses_module_paths = list_uses_module_path_selectors(order);
    let Some(graph) = select_graph(graph, module_graph, uses_module_paths) else {
        return;
    };

    // Build a position map: boundary name -> index in the layer order.
    // Lower index = lower layer. A boundary at index i may only depend on
    // boundaries at index j where j < i.
    let positions: std::collections::BTreeMap<&str, usize> = order
        .iter()
        .enumerate()
        .map(|(i, name)| (name.as_str(), i))
        .collect();

    for ((edge_source, edge_target), edge) in &graph.edges {
        let source_selector = if uses_module_paths {
            longest_matching_selector(edge_source, order)
        } else {
            Some(edge_source.clone())
        };
        let Some(source_selector) = source_selector else {
            continue; // edge source not in this layer set — not governed
        };
        let target_selector = if uses_module_paths {
            longest_matching_selector(edge_target, order)
        } else {
            Some(edge_target.clone())
        };
        let Some(target_selector) = target_selector else {
            continue; // edge target not in this layer set — not governed
        };
        if source_selector == target_selector {
            continue;
        }
        let Some(&source_pos) = positions.get(source_selector.as_str()) else {
            continue;
        };
        let Some(&target_pos) = positions.get(target_selector.as_str()) else {
            continue;
        };
        if target_pos >= source_pos {
            // Depending on same layer or higher layer is a violation
            violations.push(Violation {
                rule_id: rule.id.clone(),
                rule_type: "layers".to_string(),
                source_boundary: source_selector.clone(),
                target_boundary: target_selector.clone(),
                sample: edge.sample.clone(),
                detail: Some(format!(
                    "{} (layer {}) must not depend on {} (layer {})",
                    source_selector, source_pos, target_selector, target_pos
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
    module_graph: Option<&BoundaryGraph>,
    rule: &RuleSpec,
    targets: &[String],
    allowed_importers: &[String],
    violations: &mut Vec<Violation>,
) {
    let uses_module_paths = list_uses_module_path_selectors(targets)
        || list_uses_module_path_selectors(allowed_importers);
    let Some(graph) = select_graph(graph, module_graph, uses_module_paths) else {
        return;
    };
    let target_set: BTreeSet<&str> = targets.iter().map(|s| s.as_str()).collect();
    let allowed_set: BTreeSet<&str> = allowed_importers.iter().map(|s| s.as_str()).collect();

    for ((edge_source, edge_target), edge) in &graph.edges {
        let target_selector = if uses_module_paths {
            longest_matching_selector(edge_target, targets)
        } else if target_set.contains(edge_target.as_str()) {
            Some(edge_target.clone())
        } else {
            None
        };
        let Some(target_selector) = target_selector else {
            continue; // edge target is not protected by this rule
        };
        let is_allowed = if uses_module_paths {
            allowed_importers
                .iter()
                .any(|selector| selector_matches(selector, edge_source))
        } else {
            allowed_set.contains(edge_source.as_str())
        };
        if is_allowed {
            continue; // source is an authorized importer
        }
        violations.push(Violation {
            rule_id: rule.id.clone(),
            rule_type: "protected".to_string(),
            source_boundary: edge_source.clone(),
            target_boundary: target_selector.clone(),
            sample: edge.sample.clone(),
            detail: Some(format!(
                "{} is not an allowed importer of protected boundary {}",
                edge_source, target_selector
            )),
        });
    }
}

// ---------------------------------------------------------------------------
// independence — any direct edge among group members is a violation
// ---------------------------------------------------------------------------

fn evaluate_independence(
    graph: &BoundaryGraph,
    module_graph: Option<&BoundaryGraph>,
    rule: &RuleSpec,
    members: &[String],
    violations: &mut Vec<Violation>,
) {
    let uses_module_paths = list_uses_module_path_selectors(members);
    let Some(graph) = select_graph(graph, module_graph, uses_module_paths) else {
        return;
    };
    let member_set: BTreeSet<&str> = members.iter().map(|s| s.as_str()).collect();

    for ((edge_source, edge_target), edge) in &graph.edges {
        let source_selector = if uses_module_paths {
            longest_matching_selector(edge_source, members)
        } else if member_set.contains(edge_source.as_str()) {
            Some(edge_source.clone())
        } else {
            None
        };
        let Some(source_selector) = source_selector else {
            continue;
        };
        let target_selector = if uses_module_paths {
            longest_matching_selector(edge_target, members)
        } else if member_set.contains(edge_target.as_str()) {
            Some(edge_target.clone())
        } else {
            None
        };
        let Some(target_selector) = target_selector else {
            continue;
        };
        if source_selector == target_selector {
            continue;
        }
        if member_set.contains(source_selector.as_str())
            && member_set.contains(target_selector.as_str())
        {
            violations.push(Violation {
                rule_id: rule.id.clone(),
                rule_type: "independence".to_string(),
                source_boundary: source_selector.clone(),
                target_boundary: target_selector.clone(),
                sample: edge.sample.clone(),
                detail: Some(format!(
                    "{} and {} must be independent (no direct dependencies)",
                    source_selector, target_selector
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
    module_graph: Option<&BoundaryGraph>,
    rule: &RuleSpec,
    members: &[String],
    skip_parent_child: bool,
    violations: &mut Vec<Violation>,
) {
    let uses_module_paths = list_uses_module_path_selectors(members);
    let Some(graph) = select_graph(graph, module_graph, uses_module_paths) else {
        return;
    };

    // Expand the "*" wildcard to every node in the graph.
    let expanded: Vec<String>;
    let members = if members.iter().any(|m| m == policy::WILDCARD_ALL) {
        expanded = graph.boundaries.iter().cloned().collect();
        &expanded[..]
    } else {
        members
    };

    let member_set: BTreeSet<String> = members.iter().cloned().collect();

    let mut adj: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut samples: BTreeMap<(String, String), DependencySample> = BTreeMap::new();
    for member in members {
        adj.entry(member.clone()).or_default();
    }
    for ((source, target), edge) in &graph.edges {
        let source_selector = if uses_module_paths {
            longest_matching_selector(source, members)
        } else if member_set.contains(source) {
            Some(source.clone())
        } else {
            None
        };
        let Some(source_selector) = source_selector else {
            continue;
        };
        let target_selector = if uses_module_paths {
            longest_matching_selector(target, members)
        } else if member_set.contains(target) {
            Some(target.clone())
        } else {
            None
        };
        let Some(target_selector) = target_selector else {
            continue;
        };
        if source_selector == target_selector {
            continue;
        }
        if skip_parent_child && is_parent_child(&source_selector, &target_selector) {
            continue;
        }
        adj.entry(source_selector.clone())
            .or_default()
            .push(target_selector.clone());
        samples
            .entry((source_selector, target_selector))
            .or_insert_with(|| edge.sample.clone());
    }

    // DFS-based cycle detection
    #[derive(Clone, Copy, PartialEq)]
    enum Color {
        White,
        Gray,
        Black,
    }

    let mut color: BTreeMap<String, Color> = member_set
        .iter()
        .cloned()
        .map(|m| (m, Color::White))
        .collect();
    let mut path: Vec<String> = Vec::new();

    fn dfs(
        node: String,
        adj: &BTreeMap<String, Vec<String>>,
        color: &mut BTreeMap<String, Color>,
        path: &mut Vec<String>,
        cycles: &mut Vec<Vec<String>>,
    ) {
        color.insert(node.clone(), Color::Gray);
        path.push(node.clone());

        if let Some(neighbors) = adj.get(&node) {
            for next in neighbors {
                match color.get(next) {
                    Some(Color::Gray) => {
                        // Found a cycle. Extract the cycle path starting from `next`.
                        let cycle_start = path.iter().position(|n| n == next).unwrap();
                        let mut cycle: Vec<String> =
                            path[cycle_start..].iter().map(|s| s.to_string()).collect();
                        cycle.push(next.to_string()); // close the cycle
                        cycles.push(cycle);
                    }
                    Some(Color::White) | None => {
                        dfs(next.clone(), adj, color, path, cycles);
                    }
                    Some(Color::Black) => {}
                }
            }
        }

        path.pop();
        color.insert(node, Color::Black);
    }

    let mut cycles: Vec<Vec<String>> = Vec::new();
    for node in &member_set {
        if color.get(node) == Some(&Color::White) {
            dfs(node.clone(), &adj, &mut color, &mut path, &mut cycles);
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
        let sample = samples
            .get(&(source.clone(), target.clone()))
            .cloned()
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

    fn empty_graph() -> BoundaryGraph {
        BoundaryGraph::new(BTreeSet::new())
    }

    fn allow_policy(source: &str, allowed: &[&str]) -> ArchitecturePolicy {
        let mut boundaries = BTreeMap::new();
        boundaries.insert(
            source.split("::").next().unwrap_or(source).to_string(),
            Default::default(),
        );
        for dep in allowed {
            boundaries.insert(
                dep.split("::").next().unwrap_or(dep).to_string(),
                Default::default(),
            );
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
        let mut boundaries = BTreeMap::new();
        for name in order {
            let root = name.split("::").next().unwrap_or(name);
            boundaries.entry(root.to_string()).or_default();
        }
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

    #[test]
    fn allow_rule_reports_disallowed_edge_for_module_paths() {
        let graph = graph_with_edge("render::graph::emit", "render::timeline::layout");
        let policy = allow_policy("render::graph", &["render::text", "render::svg"]);

        let violations =
            evaluate_rules_with_module_graph(&empty_graph(), Some(&graph), &policy).violations;

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].source_boundary, "render::graph");
        assert_eq!(violations[0].target_boundary, "render::timeline::layout");
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

    #[test]
    fn layers_rule_reports_upward_dependency_for_module_paths() {
        let graph = graph_with_edge("render::text::canvas", "render::graph");
        let policy = layers_policy(&[
            "render::text",
            "render::svg",
            "render::graph",
            "render::timeline",
        ]);

        let violations =
            evaluate_rules_with_module_graph(&empty_graph(), Some(&graph), &policy).violations;

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_type, "layers");
        assert_eq!(violations[0].source_boundary, "render::text");
        assert_eq!(violations[0].target_boundary, "render::graph");
    }

    #[test]
    fn layers_rule_allows_downward_dependency_for_module_paths() {
        let graph = graph_with_edge("render::graph::text", "render::text::canvas");
        let policy = layers_policy(&[
            "render::text",
            "render::svg",
            "render::graph",
            "render::timeline",
        ]);

        let violations =
            evaluate_rules_with_module_graph(&empty_graph(), Some(&graph), &policy).violations;

        assert!(violations.is_empty());
    }

    #[test]
    fn layers_rule_ignores_edges_outside_the_declared_module_set_for_module_paths() {
        let graph = graph_with_edge("render::text::canvas", "graph::measure");
        let policy = layers_policy(&[
            "render::text",
            "render::svg",
            "render::graph",
            "render::timeline",
        ]);

        let violations =
            evaluate_rules_with_module_graph(&empty_graph(), Some(&graph), &policy).violations;

        assert!(violations.is_empty());
    }

    // -- protected rule tests --

    fn protected_policy(targets: &[&str], allowed_importers: &[&str]) -> ArchitecturePolicy {
        let mut boundaries = BTreeMap::new();
        for name in targets.iter().chain(allowed_importers.iter()) {
            boundaries.insert(
                name.split("::").next().unwrap_or(name).to_string(),
                Default::default(),
            );
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

    #[test]
    fn protected_rule_rejects_unauthorized_importer_for_module_paths() {
        let graph = graph_with_edge("render::timeline::draw", "render::text::layout");
        let policy = protected_policy(&["render::text"], &["render::graph", "render::text"]);

        let violations =
            evaluate_rules_with_module_graph(&empty_graph(), Some(&graph), &policy).violations;

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].source_boundary, "render::timeline::draw");
        assert_eq!(violations[0].target_boundary, "render::text");
    }

    // -- independence rule tests --

    fn independence_policy(members: &[&str]) -> ArchitecturePolicy {
        let mut boundaries = BTreeMap::new();
        for name in members {
            boundaries.insert(
                name.split("::").next().unwrap_or(name).to_string(),
                Default::default(),
            );
        }
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

    #[test]
    fn independence_rule_rejects_peer_dependency_for_module_paths() {
        let graph = graph_with_edge("render::graph::emit", "render::timeline::layout");
        let policy = independence_policy(&["render::graph", "render::timeline"]);

        let violations =
            evaluate_rules_with_module_graph(&empty_graph(), Some(&graph), &policy).violations;

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].source_boundary, "render::graph");
        assert_eq!(violations[0].target_boundary, "render::timeline");
    }

    // -- acyclic rule tests --

    fn acyclic_policy(members: &[&str]) -> ArchitecturePolicy {
        let mut boundaries = BTreeMap::new();
        for name in members {
            boundaries.insert(
                name.split("::").next().unwrap_or(name).to_string(),
                Default::default(),
            );
        }
        ArchitecturePolicy {
            version: 1,
            modules: boundaries,
            rules: vec![RuleSpec {
                id: "no-cycles".to_string(),
                rule: RuleKind::Acyclic(crate::architecture::policy::AcyclicRule {
                    members: members.iter().map(|s| s.to_string()).collect(),
                    skip_parent_child: false,
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

    #[test]
    fn acyclic_rule_detects_cycle_for_module_paths() {
        let graph = graph_with_edges(&[
            ("render::graph::emit", "render::timeline::layout"),
            ("render::timeline::draw", "render::graph::helpers"),
        ]);
        let policy = acyclic_policy(&["render::graph", "render::timeline"]);

        let violations =
            evaluate_rules_with_module_graph(&empty_graph(), Some(&graph), &policy).violations;

        assert_eq!(violations.len(), 1);
        let detail = violations[0].detail.as_ref().unwrap();
        assert!(detail.contains("render::graph"));
        assert!(detail.contains("render::timeline"));
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

    #[test]
    fn acyclic_wildcard_expands_to_all_graph_nodes() {
        // The "*" wildcard should expand to all boundaries in the graph,
        // detecting cycles without enumerating members explicitly.
        let graph = graph_with_edges(&[("a", "b"), ("b", "c"), ("c", "a")]);
        let policy = acyclic_policy(&["*"]);
        let result = evaluate_rules(&graph, &policy);
        assert_eq!(result.violations.len(), 1);
        let detail = result.violations[0].detail.as_ref().unwrap();
        assert!(detail.contains("a"), "cycle should include a: {detail}");
        assert!(detail.contains("b"), "cycle should include b: {detail}");
        assert!(detail.contains("c"), "cycle should include c: {detail}");
    }

    #[test]
    fn acyclic_wildcard_passes_for_dag() {
        let graph = graph_with_edges(&[("a", "b"), ("b", "c"), ("a", "c")]);
        let policy = acyclic_policy(&["*"]);
        let result = evaluate_rules(&graph, &policy);
        assert!(result.violations.is_empty());
    }

    #[test]
    fn acyclic_wildcard_with_module_paths() {
        // "*" should expand to all nodes in the module graph too.
        let graph = graph_with_edges(&[
            ("render::svg::edges", "render::svg::edges::markers"),
            ("render::svg::edges::markers", "render::svg::edges"),
        ]);
        let mut policy = acyclic_policy(&["*"]);
        // Add "render" to modules so the policy is structurally valid
        policy
            .modules
            .insert("render".to_string(), Default::default());
        let result = evaluate_rules_with_module_graph(&empty_graph(), Some(&graph), &policy);
        assert_eq!(result.violations.len(), 1);
        let detail = result.violations[0].detail.as_ref().unwrap();
        assert!(
            detail.contains("render::svg::edges"),
            "cycle should include edges: {detail}"
        );
    }

    #[test]
    fn acyclic_skip_parent_child_ignores_structural_cycles() {
        // Parent/child cycles (mod.rs ↔ child) should be skipped.
        let graph = graph_with_edges(&[
            ("render::svg::edges", "render::svg::edges::markers"),
            ("render::svg::edges::markers", "render::svg::edges"),
        ]);
        let mut policy = acyclic_policy(&["*"]);
        policy
            .modules
            .insert("render".to_string(), Default::default());
        // Enable skip_parent_child on the rule
        if let RuleKind::Acyclic(ref mut acyc) = policy.rules[0].rule {
            acyc.skip_parent_child = true;
        }
        let result = evaluate_rules_with_module_graph(&empty_graph(), Some(&graph), &policy);
        assert!(
            result.violations.is_empty(),
            "parent/child cycle should be skipped, got: {:?}",
            result.violations
        );
    }

    #[test]
    fn acyclic_skip_parent_child_still_catches_peer_cycles() {
        // Non-parent/child cycles should still be detected.
        let graph = graph_with_edges(&[
            ("graph::routing", "graph::grid"),
            ("graph::grid", "graph::routing"),
        ]);
        let mut policy = acyclic_policy(&["*"]);
        policy
            .modules
            .insert("graph".to_string(), Default::default());
        if let RuleKind::Acyclic(ref mut acyc) = policy.rules[0].rule {
            acyc.skip_parent_child = true;
        }
        let result = evaluate_rules_with_module_graph(&empty_graph(), Some(&graph), &policy);
        assert_eq!(result.violations.len(), 1, "peer cycle should be caught");
    }

    #[test]
    fn is_parent_child_helper() {
        assert!(super::is_parent_child("foo", "foo::bar"));
        assert!(super::is_parent_child("foo::bar", "foo"));
        assert!(super::is_parent_child("a::b", "a::b::c::d"));
        assert!(!super::is_parent_child("foo", "foobar"));
        assert!(!super::is_parent_child("foo::bar", "foo::baz"));
        assert!(!super::is_parent_child("foo", "foo"));
    }
}
