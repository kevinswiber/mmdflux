use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Public model — the internal representation after parsing and validation
// ---------------------------------------------------------------------------

/// Parsed and validated architecture policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ArchitecturePolicy {
    /// Format version.
    pub(crate) version: u32,
    /// Declared modules keyed by name.
    pub(crate) modules: BTreeMap<String, ModuleSpec>,
    /// Typed architecture rules.
    pub(crate) rules: Vec<RuleSpec>,
    /// Named exceptions that suppress known violations.
    pub(crate) exceptions: Vec<ExceptionSpec>,
}

impl ArchitecturePolicy {
    /// Extract the allow map (source → allowed targets) from allow rules.
    /// Used for rendering compatibility with the existing text reporter.
    pub(crate) fn extract_allow_map(&self) -> BTreeMap<String, BTreeSet<String>> {
        let mut map = BTreeMap::new();
        for rule in &self.rules {
            if let RuleKind::Allow(allow) = &rule.rule {
                map.insert(
                    allow.source.clone(),
                    allow.allowed.iter().cloned().collect(),
                );
            }
        }
        map
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ModuleSpec {
    /// Freeform key-value tags for grouping and filtering.
    pub(crate) tags: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuleSpec {
    /// Unique identifier for this rule (used in exceptions and reports).
    pub(crate) id: String,
    /// The typed rule body.
    pub(crate) rule: RuleKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuleKind {
    Allow(AllowRule),
    Layers(LayersRule),
    Protected(ProtectedRule),
    Independence(IndependenceRule),
    Acyclic(AcyclicRule),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AllowRule {
    /// The source boundary this rule governs.
    pub(crate) source: String,
    /// Boundaries that `source` is allowed to depend on.
    pub(crate) allowed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LayersRule {
    /// Ordered list of boundaries from lowest to highest.
    /// A boundary may only depend on boundaries earlier in the list.
    pub(crate) order: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProtectedRule {
    /// Boundaries whose access is restricted.
    pub(crate) targets: Vec<String>,
    /// Only these boundaries may import the protected targets.
    pub(crate) allowed_importers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndependenceRule {
    /// Boundaries that must not depend on each other.
    pub(crate) members: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AcyclicRule {
    /// Boundaries that must form a DAG (no dependency cycles among them).
    pub(crate) members: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExceptionSpec {
    /// Unique identifier for this exception.
    pub(crate) id: String,
    /// Which rule this exception applies to.
    pub(crate) rule_id: String,
    /// Source boundary of the suppressed edge.
    pub(crate) source: String,
    /// Target boundary of the suppressed edge.
    pub(crate) target: String,
    /// Human-readable justification.
    pub(crate) reason: String,
    /// Who owns this exception.
    pub(crate) owner: String,
}

// ---------------------------------------------------------------------------
// Serde shapes — raw TOML deserialization before validation
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RawPolicy {
    version: u32,
    #[serde(default)]
    modules: BTreeMap<String, RawModuleSpec>,
    #[serde(default)]
    rules: Vec<RawRuleSpec>,
    #[serde(default)]
    exceptions: Vec<RawExceptionSpec>,
}

#[derive(Debug, Deserialize, Default)]
struct RawModuleSpec {
    #[serde(default)]
    tags: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct RawRuleSpec {
    id: String,
    #[serde(rename = "type")]
    rule_type: Option<String>,
    #[serde(default)]
    config: toml::Table,
}

#[derive(Debug, Deserialize)]
struct RawExceptionSpec {
    id: String,
    rule_id: Option<String>,
    source: Option<String>,
    target: Option<String>,
    reason: Option<String>,
    owner: Option<String>,
}

// ---------------------------------------------------------------------------
// Parsing entrypoint
// ---------------------------------------------------------------------------

pub(crate) fn parse_policy_str(input: &str) -> Result<ArchitecturePolicy> {
    let raw: RawPolicy =
        toml::from_str(input).context("failed to parse architecture policy TOML")?;

    match raw.version {
        1 => validate_policy(raw),
        v => bail!("unsupported policy version: {v}"),
    }
}

pub(crate) fn parse_policy_file(path: &Path) -> Result<ArchitecturePolicy> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    parse_policy_str(&content).with_context(|| format!("in policy file {}", path.display()))
}

const BOUNDARIES_CONFIG_ENV: &str = "SEMANTIC_BOUNDARIES_CONFIG";

/// Resolve the policy file path using the same logic as the existing checker:
/// `SEMANTIC_BOUNDARIES_CONFIG` env var, or `boundaries.toml` at the repo root.
pub(crate) fn resolve_policy_path(repo_root: &Path) -> std::path::PathBuf {
    std::env::var_os(BOUNDARIES_CONFIG_ENV)
        .map(std::path::PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                repo_root.join(path)
            }
        })
        .unwrap_or_else(|| repo_root.join("boundaries.toml"))
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

fn validate_policy(raw: RawPolicy) -> Result<ArchitecturePolicy> {
    let modules: BTreeMap<String, ModuleSpec> = raw
        .modules
        .into_iter()
        .map(|(name, spec)| (name, ModuleSpec { tags: spec.tags }))
        .collect();

    let mut rules = Vec::with_capacity(raw.rules.len());
    for raw_rule in raw.rules {
        let rule = parse_rule(&raw_rule, &modules)?;
        rules.push(RuleSpec {
            id: raw_rule.id,
            rule,
        });
    }

    let mut exceptions = Vec::with_capacity(raw.exceptions.len());
    for raw_exc in raw.exceptions {
        let exc = validate_exception(raw_exc, &modules)?;
        exceptions.push(exc);
    }

    Ok(ArchitecturePolicy {
        version: 1,
        modules,
        rules,
        exceptions,
    })
}

fn parse_rule(raw: &RawRuleSpec, boundaries: &BTreeMap<String, ModuleSpec>) -> Result<RuleKind> {
    let rule_type = raw
        .rule_type
        .as_deref()
        .with_context(|| format!("rule {:?} is missing required field \"type\"", raw.id))?;

    match rule_type {
        "allow" => {
            let source: String = get_config_string(&raw.config, "source", &raw.id)?;
            let allowed =
                get_config_string_array_or_tag(&raw.config, "allowed", &raw.id, boundaries)?;
            ensure_boundary_exists(&source, boundaries, &raw.id)?;
            for dep in &allowed {
                ensure_boundary_exists(dep, boundaries, &raw.id)?;
            }
            Ok(RuleKind::Allow(AllowRule { source, allowed }))
        }
        "layers" => {
            // layers.order stays explicit — order matters, tags have no natural ordering
            let order: Vec<String> = get_config_string_array(&raw.config, "order", &raw.id)?;
            check_no_duplicates(&order, "order", &raw.id)?;
            for boundary in &order {
                ensure_boundary_exists(boundary, boundaries, &raw.id)?;
            }
            Ok(RuleKind::Layers(LayersRule { order }))
        }
        "protected" => {
            let targets =
                get_config_string_array_or_tag(&raw.config, "targets", &raw.id, boundaries)?;
            let allowed_importers = get_config_string_array_or_tag(
                &raw.config,
                "allowed_importers",
                &raw.id,
                boundaries,
            )?;
            for t in &targets {
                ensure_boundary_exists(t, boundaries, &raw.id)?;
            }
            for i in &allowed_importers {
                ensure_boundary_exists(i, boundaries, &raw.id)?;
            }
            Ok(RuleKind::Protected(ProtectedRule {
                targets,
                allowed_importers,
            }))
        }
        "independence" => {
            let members =
                get_config_string_array_or_tag(&raw.config, "members", &raw.id, boundaries)?;
            for m in &members {
                ensure_boundary_exists(m, boundaries, &raw.id)?;
            }
            Ok(RuleKind::Independence(IndependenceRule { members }))
        }
        "acyclic" => {
            let members =
                get_config_string_array_or_tag(&raw.config, "members", &raw.id, boundaries)?;
            for m in &members {
                ensure_boundary_exists(m, boundaries, &raw.id)?;
            }
            Ok(RuleKind::Acyclic(AcyclicRule { members }))
        }
        other => bail!("rule {:?} has unknown type {:?}", raw.id, other),
    }
}

fn validate_exception(
    raw: RawExceptionSpec,
    boundaries: &BTreeMap<String, ModuleSpec>,
) -> Result<ExceptionSpec> {
    let rule_id = raw.rule_id.with_context(|| {
        format!(
            "exception {:?} is missing required field \"rule_id\"",
            raw.id
        )
    })?;
    let source = raw.source.with_context(|| {
        format!(
            "exception {:?} is missing required field \"source\"",
            raw.id
        )
    })?;
    let target = raw.target.with_context(|| {
        format!(
            "exception {:?} is missing required field \"target\"",
            raw.id
        )
    })?;
    let reason = raw.reason.unwrap_or_default();
    let owner = raw.owner.unwrap_or_default();

    ensure_boundary_exists(&source, boundaries, &format!("exception {:?}", raw.id))?;
    ensure_boundary_exists(&target, boundaries, &format!("exception {:?}", raw.id))?;

    Ok(ExceptionSpec {
        id: raw.id,
        rule_id,
        source,
        target,
        reason,
        owner,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ensure_boundary_exists(
    name: &str,
    boundaries: &BTreeMap<String, ModuleSpec>,
    context: &str,
) -> Result<()> {
    if !boundaries.contains_key(name) {
        bail!("{context}: unknown boundary {name:?}");
    }
    Ok(())
}

fn check_no_duplicates(items: &[String], field: &str, rule_id: &str) -> Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    for item in items {
        if !seen.insert(item) {
            bail!("rule {rule_id:?}: duplicate boundary {item:?} in {field}");
        }
    }
    Ok(())
}

fn get_config_string(table: &toml::Table, key: &str, rule_id: &str) -> Result<String> {
    table
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .with_context(|| format!("rule {rule_id:?}: missing or invalid field {key:?}"))
}

fn get_config_string_array(table: &toml::Table, key: &str, rule_id: &str) -> Result<Vec<String>> {
    let arr = table
        .get(key)
        .and_then(|v| v.as_array())
        .with_context(|| format!("rule {rule_id:?}: missing or invalid field {key:?}"))?;

    arr.iter()
        .map(|v| {
            v.as_str()
                .map(|s| s.to_string())
                .with_context(|| format!("rule {rule_id:?}: {key} must contain only strings"))
        })
        .collect()
}

fn resolve_tag(
    tag_key: &str,
    tag_value: &str,
    boundaries: &BTreeMap<String, ModuleSpec>,
    rule_id: &str,
    field_name: &str,
) -> Result<Vec<String>> {
    let matched: Vec<String> = boundaries
        .iter()
        .filter(|(_, spec)| spec.tags.get(tag_key).is_some_and(|v| v == tag_value))
        .map(|(name, _)| name.clone())
        .collect();
    if matched.is_empty() {
        bail!(
            "rule {rule_id:?}: tag {tag_key}={tag_value:?} in {field_name} matches no boundaries"
        );
    }
    Ok(matched)
}

fn get_config_string_array_or_tag(
    table: &toml::Table,
    key: &str,
    rule_id: &str,
    boundaries: &BTreeMap<String, ModuleSpec>,
) -> Result<Vec<String>> {
    let value = table
        .get(key)
        .with_context(|| format!("rule {rule_id:?}: missing field {key:?}"))?;

    if let Some(arr) = value.as_array() {
        arr.iter()
            .map(|v| {
                v.as_str()
                    .map(|s| s.to_string())
                    .with_context(|| format!("rule {rule_id:?}: {key} must contain only strings"))
            })
            .collect()
    } else if let Some(tbl) = value.as_table() {
        let tag_key = tbl.get("tag").and_then(|v| v.as_str()).with_context(|| {
            format!("rule {rule_id:?}: {key} table must have a \"tag\" string key")
        })?;
        let tag_value = tbl.get("value").and_then(|v| v.as_str()).with_context(|| {
            format!("rule {rule_id:?}: {key} table must have a \"value\" string key")
        })?;
        resolve_tag(tag_key, tag_value, boundaries, rule_id, key)
    } else {
        bail!("rule {rule_id:?}: {key} must be an array or a {{ tag, value }} table")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_path(name: &str) -> std::path::PathBuf {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        manifest_dir
            .join("tests")
            .join("fixtures")
            .join("architecture-policy")
            .join(name)
    }

    fn parse_fixture(name: &str) -> Result<ArchitecturePolicy> {
        parse_policy_file(&fixture_path(name))
    }

    // -- v2 shape tests --

    #[test]
    fn parse_v2_minimal() {
        let policy = parse_fixture("v2-minimal.toml").unwrap();
        assert_eq!(policy.version, 1);
        assert!(policy.modules.contains_key("graph"));
        assert!(policy.modules.contains_key("errors"));
        assert!(policy.modules.contains_key("format"));
        assert!(policy.modules.contains_key("render"));
        assert_eq!(policy.rules.len(), 1);
        assert_eq!(policy.rules[0].id, "core-deps");
        assert!(matches!(policy.rules[0].rule, RuleKind::Allow(_)));
        assert!(policy.exceptions.is_empty());
    }

    #[test]
    fn parse_v2_with_tags_and_exceptions() {
        let policy = parse_fixture("v2-tags-and-exceptions.toml").unwrap();
        assert_eq!(policy.version, 1);
        assert_eq!(policy.modules["runtime"].tags["layer"], "runtime");
        assert_eq!(policy.modules["errors"].tags["layer"], "foundation");
        assert_eq!(policy.rules.len(), 2);
        assert_eq!(policy.rules[1].id, "pipeline-layers");
        assert!(matches!(policy.rules[1].rule, RuleKind::Layers(_)));
        assert_eq!(policy.exceptions.len(), 1);
        assert_eq!(policy.exceptions[0].rule_id, "pipeline-layers");
        assert_eq!(policy.exceptions[0].source, "render");
        assert_eq!(policy.exceptions[0].target, "graph");
        assert_eq!(policy.exceptions[0].owner, "kevin");
    }

    // -- v1 shape tests --

    // -- repo compatibility tests --

    fn repo_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask should live at the workspace root")
            .to_path_buf()
    }

    #[test]
    fn current_repo_boundaries_file_loads() {
        let path = resolve_policy_path(&repo_root());
        let policy = parse_policy_file(&path).unwrap();
        assert_eq!(policy.version, 1);
        assert!(!policy.modules.is_empty());
        assert!(policy.modules.contains_key("graph"));
        assert!(policy.modules.contains_key("runtime"));
        // Must have at least the allow rules
        assert!(
            policy
                .rules
                .iter()
                .any(|c| matches!(c.rule, RuleKind::Allow(_)))
        );
    }

    // -- extract_allow_map tests --

    #[test]
    fn extract_allow_map_from_policy() {
        let policy = parse_fixture("v2-minimal.toml").unwrap();
        let allow_map = policy.extract_allow_map();
        assert_eq!(
            allow_map["graph"],
            BTreeSet::from(["errors".to_string(), "format".to_string()])
        );
    }

    // -- tag resolution tests --

    #[test]
    fn parse_v2_tag_targeting() {
        let policy = parse_fixture("v2-tag-targeting.toml").unwrap();
        // protect-facades should resolve tag to builtins, engines, render (sorted)
        let prot = policy
            .rules
            .iter()
            .find(|c| c.id == "protect-facades")
            .expect("should have protect-facades");
        match &prot.rule {
            RuleKind::Protected(p) => {
                assert_eq!(p.targets, vec!["builtins", "engines", "render"]);
                assert_eq!(p.allowed_importers, vec!["runtime"]);
            }
            _ => panic!("expected protected"),
        }
        // facade-independence should resolve to the same set
        let ind = policy
            .rules
            .iter()
            .find(|c| c.id == "facade-independence")
            .expect("should have facade-independence");
        match &ind.rule {
            RuleKind::Independence(i) => {
                assert_eq!(i.members, vec!["builtins", "engines", "render"]);
            }
            _ => panic!("expected independence"),
        }
    }

    #[test]
    fn rejects_tag_matching_zero_boundaries() {
        let err = parse_fixture("invalid-tag-no-match.toml").unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("matches no boundaries"), "got: {msg}");
    }

    #[test]
    fn tag_and_explicit_array_both_work_for_same_rule_type() {
        let toml = "\
            version = 1\n\
            [modules.a]\n\
            tags = { group = \"x\" }\n\
            [modules.b]\n\
            tags = { group = \"x\" }\n\
            [modules.c]\n\
            \n\
            [[rules]]\n\
            id = \"ind-by-tag\"\n\
            type = \"independence\"\n\
            [rules.config]\n\
            members = { tag = \"group\", value = \"x\" }\n\
            \n\
            [[rules]]\n\
            id = \"ind-explicit\"\n\
            type = \"independence\"\n\
            [rules.config]\n\
            members = [\"a\", \"b\"]\n\
        ";
        let policy = parse_policy_str(toml).unwrap();
        let by_tag = policy.rules.iter().find(|c| c.id == "ind-by-tag").unwrap();
        let explicit = policy
            .rules
            .iter()
            .find(|c| c.id == "ind-explicit")
            .unwrap();
        match (&by_tag.rule, &explicit.rule) {
            (RuleKind::Independence(t), RuleKind::Independence(e)) => {
                assert_eq!(t.members, e.members);
            }
            _ => panic!("expected independence"),
        }
    }

    // -- invalid config tests --

    #[test]
    fn rejects_unknown_boundary_in_rule() {
        let err = parse_fixture("invalid-unknown-boundary.toml").unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("unknown boundary"), "got: {msg}");
        assert!(msg.contains("nonexistent"), "got: {msg}");
    }

    #[test]
    fn rejects_rule_missing_type() {
        let err = parse_fixture("invalid-rule-shape.toml").unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("type") || msg.contains("missing"),
            "got: {msg}"
        );
    }

    #[test]
    fn rejects_layers_with_duplicate_boundary() {
        let err = parse_fixture("invalid-layers-duplicate.toml").unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("duplicate"), "got: {msg}");
        assert!(msg.contains("errors"), "got: {msg}");
    }

    #[test]
    fn rejects_exception_missing_rule_id() {
        let err = parse_fixture("invalid-exception-missing-rule-id.toml").unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("rule_id"), "got: {msg}");
    }

    #[test]
    fn rejects_exception_with_unknown_boundary() {
        let err = parse_fixture("invalid-exception-unknown-boundary.toml").unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("unknown boundary"), "got: {msg}");
        assert!(msg.contains("nonexistent"), "got: {msg}");
    }

    #[test]
    fn rejects_unknown_rule_type() {
        let err = parse_fixture("invalid-unknown-type.toml").unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("unknown type"), "got: {msg}");
        assert!(msg.contains("transitive"), "got: {msg}");
    }

    #[test]
    fn rejects_unsupported_version() {
        let err = parse_fixture("invalid-version.toml").unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("unsupported policy version"), "got: {msg}");
    }
}
