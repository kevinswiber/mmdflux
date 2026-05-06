use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ttf_parser::Face;

const RECORDED_PROFILE_ID: &str = "mmdflux-sans-v1";
const PROFILE_SPEC: &str = "xtask/font_metrics/profiles/mmdflux-sans-v1.toml";
const JSON_OUTPUT: &str = "data/font-metrics/mmdflux-sans-v1.json";
const RUST_OUTPUT: &str = "src/graph/font_metrics/generated/mmdflux_sans_v1.rs";
const RUST_MOD_OUTPUT: &str = "src/graph/font_metrics/generated/mod.rs";

#[derive(Debug)]
pub(crate) struct FontMetricsOptions {
    profile: String,
    check: bool,
    output_root: Option<PathBuf>,
}

#[derive(Debug)]
struct ResolvedFontMetricsOptions {
    repo_root: PathBuf,
    output_root: PathBuf,
    profile: String,
    check: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileSpec {
    profile_id: String,
    metrics_profile_source: String,
    advance_scale: f64,
    css_line_height_ratio: f64,
    aliases: Vec<String>,
    source: SourceFontSpec,
    coverage: CoverageSpec,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceFontSpec {
    family: String,
    style: String,
    version: String,
    license: String,
    release_url: String,
    artifact_url: String,
    artifact_sha256: String,
    font_path: String,
    sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CoverageSpec {
    include_printable_ascii: bool,
    fixture_globs: Vec<String>,
    ranges: Vec<CoverageRange>,
    explicit_fallback_codepoints: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CoverageRange {
    start: String,
    end: String,
    description: String,
}

#[derive(Debug)]
struct GeneratedProfile {
    json: String,
    rust: String,
    rust_mod: String,
    rust_profile_module: String,
}

#[derive(Debug)]
struct GeneratedAdvance {
    ch: char,
    advance_units: u16,
}

pub(crate) fn parse_font_metrics_args<I, S>(args: I) -> Result<FontMetricsOptions>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut args = args.into_iter();
    match args.next() {
        Some(first) if first.as_ref() == "font-metrics" => {}
        Some(first) => bail!(
            "expected `cargo xtask font-metrics`, got `cargo xtask {}`",
            first.as_ref()
        ),
        None => bail!("missing `cargo xtask font-metrics` invocation"),
    }

    let mut profile = None;
    let mut check = false;

    while let Some(arg) = args.next() {
        match arg.as_ref() {
            "-p" | "--profile" => {
                profile = Some(next_arg(&mut args, arg.as_ref())?);
            }
            "--check" => check = true,
            other if other.starts_with("--profile=") => {
                profile = Some(
                    other
                        .strip_prefix("--profile=")
                        .expect("prefix checked")
                        .to_string(),
                );
            }
            other => bail!("unknown `cargo xtask font-metrics` argument `{other}`"),
        }
    }

    let profile = profile.ok_or_else(|| anyhow::anyhow!("missing `--profile <id>`"))?;
    validate_profile_id(&profile)?;

    Ok(FontMetricsOptions {
        profile,
        check,
        output_root: None,
    })
}

fn next_arg<I, S>(args: &mut I, flag: &str) -> Result<String>
where
    I: Iterator<Item = S>,
    S: AsRef<str>,
{
    args.next()
        .map(|value| value.as_ref().to_string())
        .ok_or_else(|| anyhow::anyhow!("missing value for `{flag}`"))
}

pub(crate) fn run(options: FontMetricsOptions) -> Result<()> {
    let options = options.resolve();
    let generated = generate_profile(&options.repo_root, &options.profile)?;
    write_generated_outputs(&options, &generated)
}

pub(crate) fn help_text() -> &'static str {
    "\
cargo xtask font-metrics --profile <id> [--check]

Generate checked-in recorded font metrics tables.

Options:
    -p, --profile <id>       Profile to generate, currently mmdflux-sans-v1
        --check              Check generated files without writing"
}

impl FontMetricsOptions {
    fn resolve(self) -> ResolvedFontMetricsOptions {
        let repo_root = crate::repo_root();
        let output_root = self.output_root.unwrap_or_else(|| repo_root.clone());

        ResolvedFontMetricsOptions {
            repo_root,
            output_root,
            profile: self.profile,
            check: self.check,
        }
    }
}

fn write_generated_outputs(
    options: &ResolvedFontMetricsOptions,
    generated: &GeneratedProfile,
) -> Result<()> {
    let outputs = [
        (
            options.output_root.join(JSON_OUTPUT),
            generated.json.as_bytes(),
        ),
        (
            options.output_root.join(RUST_OUTPUT),
            generated.rust.as_bytes(),
        ),
        (
            options.output_root.join(RUST_MOD_OUTPUT),
            generated.rust_mod.as_bytes(),
        ),
    ];

    let mut changed = Vec::new();
    for (path, bytes) in outputs {
        write_or_check_file(&path, bytes, options.check, &mut changed)?;
    }

    if options.check && !changed.is_empty() {
        for path in &changed {
            println!("  stale: {}", display_path(&options.output_root, path));
        }
        bail!(
            "font metrics generated files are out of date; run `cargo xtask font-metrics --profile {}`",
            options.profile
        );
    }

    if options.check {
        println!("font metrics generated files are up to date.");
    } else {
        println!(
            "Wrote font metrics generated files for {} (module {}).",
            options.profile, generated.rust_profile_module
        );
    }

    Ok(())
}

fn write_or_check_file(
    path: &Path,
    bytes: &[u8],
    check: bool,
    changed: &mut Vec<PathBuf>,
) -> Result<()> {
    if check {
        if fs::read(path).is_ok_and(|existing| existing == bytes) {
            return Ok(());
        }
        changed.push(path.to_path_buf());
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory `{}`", parent.display()))?;
    }
    fs::write(path, bytes).with_context(|| format!("failed to write `{}`", path.display()))
}

fn parse_profile_spec_str(input: &str) -> Result<ProfileSpec> {
    let spec: ProfileSpec =
        toml::from_str(input).context("failed to parse font metrics profile")?;
    validate_profile_id(&spec.profile_id)?;

    if spec.metrics_profile_source != "recorded" {
        bail!(
            "font metrics profile `{}` must use metrics_profile_source = \"recorded\"",
            spec.profile_id
        );
    }

    Ok(spec)
}

fn load_profile_spec(repo_root: &Path, profile: &str) -> Result<ProfileSpec> {
    validate_profile_id(profile)?;
    let path = match profile {
        RECORDED_PROFILE_ID => repo_root.join(PROFILE_SPEC),
        _ => unreachable!("profile validation restricts accepted IDs"),
    };
    let input = fs::read_to_string(&path)
        .with_context(|| format!("failed to read profile spec `{}`", path.display()))?;
    parse_profile_spec_str(&input)
}

fn validate_profile_id(id: &str) -> Result<()> {
    match id {
        RECORDED_PROFILE_ID => Ok(()),
        "mermaid-sans-v1" | "mermaid-default-v1" | "default-sans-v1" => {
            bail!("`{id}` is not a persisted profile ID; use `mmdflux-sans-v1`")
        }
        _ => bail!("unsupported recorded font metrics profile `{id}`"),
    }
}

fn generate_profile(repo_root: &Path, profile: &str) -> Result<GeneratedProfile> {
    let spec = load_profile_spec(repo_root, profile)?;
    generate_from_spec(repo_root, &spec)
}

fn generate_from_spec(repo_root: &Path, spec: &ProfileSpec) -> Result<GeneratedProfile> {
    let font_path = resolve_path(repo_root, Path::new(&spec.source.font_path));
    let font_bytes = fs::read(&font_path)
        .with_context(|| format!("failed to read source font `{}`", font_path.display()))?;
    let actual_sha256 = sha256_hex(&font_bytes);
    if actual_sha256 != spec.source.sha256 {
        bail!(
            "source font sha256 mismatch for `{}`: expected {}, got {}",
            spec.source.font_path,
            spec.source.sha256,
            actual_sha256
        );
    }

    let face = Face::parse(&font_bytes, 0)
        .map_err(|err| anyhow::anyhow!("failed to parse source font: {err:?}"))?;
    let codepoints = collect_required_codepoints(repo_root, spec, &face)?;
    let advances = collect_advances(&face, &codepoints);
    let rust_profile_module = profile_module_name(&spec.profile_id);

    Ok(GeneratedProfile {
        json: emit_json(spec, &face, &advances)?,
        rust: emit_rust(spec, &face, &advances),
        rust_mod: emit_rust_mod(&rust_profile_module),
        rust_profile_module,
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn collect_required_codepoints(
    repo_root: &Path,
    spec: &ProfileSpec,
    face: &Face<'_>,
) -> Result<BTreeSet<char>> {
    let explicit_fallbacks = parse_explicit_fallbacks(spec)?;
    let mut codepoints = BTreeSet::new();

    if spec.coverage.include_printable_ascii {
        for scalar in 0x20..=0x7e {
            codepoints.insert(char::from_u32(scalar).expect("printable ASCII scalar"));
        }
    }

    for range in &spec.coverage.ranges {
        let start = parse_codepoint(&range.start)?;
        let end = parse_codepoint(&range.end)?;
        if start > end {
            bail!(
                "coverage range `{}` ends before it starts: {}..{}",
                range.description,
                range.start,
                range.end
            );
        }
        for scalar in start as u32..=end as u32 {
            if let Some(ch) = char::from_u32(scalar) {
                codepoints.insert(ch);
            }
        }
    }

    if !spec.coverage.fixture_globs.is_empty() {
        scan_fixture_scalars(&repo_root.join("tests/fixtures"), &mut codepoints)?;
    }

    for ch in &explicit_fallbacks {
        codepoints.insert(*ch);
    }

    for ch in &codepoints {
        if face.glyph_index(*ch).is_none() && !explicit_fallbacks.contains(ch) {
            bail!("missing required glyph {}", format_codepoint(*ch));
        }
    }

    Ok(codepoints)
}

fn parse_explicit_fallbacks(spec: &ProfileSpec) -> Result<BTreeSet<char>> {
    spec.coverage
        .explicit_fallback_codepoints
        .iter()
        .map(|value| parse_codepoint(value))
        .collect()
}

fn scan_fixture_scalars(root: &Path, codepoints: &mut BTreeSet<char>) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }

    let mut entries = fs::read_dir(root)
        .with_context(|| format!("failed to read fixture directory `{}`", root.display()))?
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("failed to read fixture directory `{}`", root.display()))?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            scan_fixture_scalars(&path, codepoints)?;
        } else if matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("mmd" | "json")
        ) {
            let input = fs::read_to_string(&path)
                .with_context(|| format!("failed to read fixture `{}`", path.display()))?;
            codepoints.extend(input.chars().filter(|ch| !ch.is_control()));
        }
    }

    Ok(())
}

fn collect_advances(face: &Face<'_>, codepoints: &BTreeSet<char>) -> Vec<GeneratedAdvance> {
    codepoints
        .iter()
        .filter_map(|ch| {
            let glyph = face.glyph_index(*ch)?;
            let advance_units = face.glyph_hor_advance(glyph)?;
            Some(GeneratedAdvance {
                ch: *ch,
                advance_units,
            })
        })
        .collect()
}

fn parse_codepoint(input: &str) -> Result<char> {
    let hex = input
        .strip_prefix("U+")
        .ok_or_else(|| anyhow::anyhow!("codepoint `{input}` must use U+XXXX format"))?;
    let scalar =
        u32::from_str_radix(hex, 16).with_context(|| format!("invalid codepoint `{input}`"))?;
    char::from_u32(scalar).ok_or_else(|| anyhow::anyhow!("invalid Unicode scalar `{input}`"))
}

fn format_codepoint(ch: char) -> String {
    format!("U+{:04X}", ch as u32)
}

fn emit_json(spec: &ProfileSpec, face: &Face<'_>, advances: &[GeneratedAdvance]) -> Result<String> {
    let output = JsonProfile {
        profile_id: &spec.profile_id,
        metrics_profile_source: &spec.metrics_profile_source,
        source: JsonSource {
            family: &spec.source.family,
            style: &spec.source.style,
            version: &spec.source.version,
            license: &spec.source.license,
            release_url: &spec.source.release_url,
            artifact_url: &spec.source.artifact_url,
            artifact_sha256: &spec.source.artifact_sha256,
            font_sha256: &spec.source.sha256,
        },
        aliases: &spec.aliases,
        advance_scale: spec.advance_scale,
        line_metrics: JsonLineMetrics {
            units_per_em: face.units_per_em(),
            ascender: face.ascender(),
            descender: face.descender(),
            line_gap: face.line_gap(),
            css_line_height_ratio: spec.css_line_height_ratio,
        },
        default_text_style: JsonDefaultTextStyle {
            font_family: "\"trebuchet ms\", verdana, arial, sans-serif",
            font_size: 16.0,
            line_height: 24.0,
            font_style: "normal",
            font_weight: 400,
        },
        coverage: JsonCoverage {
            include_printable_ascii: spec.coverage.include_printable_ascii,
            fixture_globs: &spec.coverage.fixture_globs,
            ranges: spec
                .coverage
                .ranges
                .iter()
                .map(|range| JsonCoverageRange {
                    start: &range.start,
                    end: &range.end,
                    description: &range.description,
                })
                .collect(),
            explicit_fallback_codepoints: &spec.coverage.explicit_fallback_codepoints,
        },
        fallback_policy: JsonFallbackPolicy {
            tab: "four spaces",
            combining_mark_em: 0.0,
            space_separator_em: 0.25,
            wide_scalar_em: 1.0,
            missing_scalar_em: 0.56,
            control_em: 0.0,
        },
        advances: advances
            .iter()
            .map(|advance| JsonAdvance {
                codepoint: format_codepoint(advance.ch),
                advance_units: advance.advance_units,
            })
            .collect(),
    };

    let mut json =
        serde_json::to_string_pretty(&output).context("failed to encode metrics JSON")?;
    json.push('\n');
    Ok(json)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonProfile<'a> {
    profile_id: &'a str,
    metrics_profile_source: &'a str,
    source: JsonSource<'a>,
    aliases: &'a [String],
    advance_scale: f64,
    line_metrics: JsonLineMetrics,
    default_text_style: JsonDefaultTextStyle<'a>,
    coverage: JsonCoverage<'a>,
    fallback_policy: JsonFallbackPolicy<'a>,
    advances: Vec<JsonAdvance>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonSource<'a> {
    family: &'a str,
    style: &'a str,
    version: &'a str,
    license: &'a str,
    release_url: &'a str,
    artifact_url: &'a str,
    artifact_sha256: &'a str,
    font_sha256: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonLineMetrics {
    units_per_em: u16,
    ascender: i16,
    descender: i16,
    line_gap: i16,
    css_line_height_ratio: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonDefaultTextStyle<'a> {
    font_family: &'a str,
    font_size: f64,
    line_height: f64,
    font_style: &'a str,
    font_weight: u16,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonCoverage<'a> {
    include_printable_ascii: bool,
    fixture_globs: &'a [String],
    ranges: Vec<JsonCoverageRange<'a>>,
    explicit_fallback_codepoints: &'a [String],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonCoverageRange<'a> {
    start: &'a str,
    end: &'a str,
    description: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonFallbackPolicy<'a> {
    tab: &'a str,
    combining_mark_em: f64,
    space_separator_em: f64,
    wide_scalar_em: f64,
    missing_scalar_em: f64,
    control_em: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonAdvance {
    codepoint: String,
    advance_units: u16,
}

fn emit_rust(spec: &ProfileSpec, face: &Face<'_>, advances: &[GeneratedAdvance]) -> String {
    let mut output = String::new();
    output.push_str("// @generated by `cargo xtask font-metrics --profile ");
    output.push_str(&spec.profile_id);
    output.push_str("`; do not edit by hand.\n");
    output.push_str("// Source: ");
    output.push_str(&spec.source.family);
    output.push(' ');
    output.push_str(&spec.source.style);
    output.push(' ');
    output.push_str(&spec.source.version);
    output.push_str(" (");
    output.push_str(&spec.source.license);
    output.push_str(")\n");
    output.push_str("// Source font SHA-256: ");
    output.push_str(&spec.source.sha256);
    output.push_str("\n#![allow(dead_code)]\n\n");
    output.push_str("pub const PROFILE_ID: &str = \"");
    output.push_str(&spec.profile_id);
    output.push_str("\";\n");
    output.push_str("pub const METRICS_PROFILE_SOURCE: &str = \"");
    output.push_str(&spec.metrics_profile_source);
    output.push_str("\";\n");
    output.push_str("pub const SOURCE_FONT_SHA256: &str =\n    \"");
    output.push_str(&spec.source.sha256);
    output.push_str("\";\n");
    output.push_str(&format!(
        "pub const UNITS_PER_EM: u16 = {};\n",
        face.units_per_em()
    ));
    output.push_str(&format!("pub const ASCENDER: i16 = {};\n", face.ascender()));
    output.push_str(&format!(
        "pub const DESCENDER: i16 = {};\n",
        face.descender()
    ));
    output.push_str(&format!("pub const LINE_GAP: i16 = {};\n", face.line_gap()));
    output.push_str(&format!(
        "pub const ADVANCE_SCALE: f64 = {:.1};\n",
        spec.advance_scale
    ));
    output.push_str(&format!(
        "pub const CSS_LINE_HEIGHT_RATIO: f64 = {:.1};\n\n",
        spec.css_line_height_ratio
    ));
    output.push_str("pub const ADVANCES: &[(char, u16)] = &[\n");
    for advance in advances {
        output.push_str(&format!(
            "    ('\\u{{{:X}}}', {}),\n",
            advance.ch as u32, advance.advance_units
        ));
    }
    output.push_str("];\n");
    output
}

fn emit_rust_mod(module: &str) -> String {
    format!(
        "// @generated by `cargo xtask font-metrics`; do not edit by hand.\n\npub mod {module};\n"
    )
}

fn profile_module_name(profile_id: &str) -> String {
    profile_id.replace('-', "_")
}

fn resolve_path(repo_root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root.join(path)
    }
}

fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

#[cfg(test)]
fn generate_profile_for_test(profile: &str) -> Result<GeneratedProfile> {
    generate_profile(&crate::repo_root(), profile)
}

#[cfg(test)]
fn generate_from_spec_for_test(spec: &ProfileSpec) -> Result<GeneratedProfile> {
    generate_from_spec(&crate::repo_root(), spec)
}

#[cfg(test)]
fn fixture_spec() -> ProfileSpec {
    load_profile_spec(&crate::repo_root(), RECORDED_PROFILE_ID).expect("fixture spec loads")
}

#[cfg(test)]
fn run_font_metrics_check(mut options: FontMetricsOptions) -> Result<()> {
    options.check = true;
    run(options)
}

#[cfg(test)]
fn assert_codepoints_are_sorted(json: &str) {
    let value: serde_json::Value = serde_json::from_str(json).expect("generated JSON parses");
    let advances = value["advances"].as_array().expect("advances is an array");
    let codepoints = advances
        .iter()
        .map(|advance| {
            advance["codepoint"]
                .as_str()
                .expect("advance has codepoint")
                .to_string()
        })
        .collect::<Vec<_>>();
    let mut sorted = codepoints.clone();
    sorted.sort();

    assert_eq!(codepoints, sorted);
}

#[cfg(test)]
struct TempGeneratedProfile {
    root: PathBuf,
}

#[cfg(test)]
impl TempGeneratedProfile {
    fn with_modified_output() -> Self {
        let root = unique_temp_root();
        let json = root.join(JSON_OUTPUT);
        let rust = root.join(RUST_OUTPUT);
        let rust_mod = root.join(RUST_MOD_OUTPUT);
        fs::create_dir_all(json.parent().expect("json parent")).unwrap();
        fs::create_dir_all(rust.parent().expect("rust parent")).unwrap();
        fs::write(json, b"stale\n").unwrap();
        fs::write(rust, b"stale\n").unwrap();
        fs::write(rust_mod, b"stale\n").unwrap();

        Self { root }
    }

    fn options(&self) -> FontMetricsOptions {
        FontMetricsOptions {
            profile: RECORDED_PROFILE_ID.to_string(),
            check: true,
            output_root: Some(self.root.clone()),
        }
    }
}

#[cfg(test)]
impl Drop for TempGeneratedProfile {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[cfg(test)]
fn unique_temp_root() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("mmdflux-font-metrics-{nanos}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mmdflux_sans_v1_profile_spec() {
        let spec = parse_profile_spec_str(include_str!(
            "../font_metrics/profiles/mmdflux-sans-v1.toml"
        ))
        .expect("profile spec parses");

        assert_eq!(spec.profile_id, "mmdflux-sans-v1");
        assert_eq!(spec.advance_scale, 1.0);
        assert_eq!(spec.css_line_height_ratio, 1.5);
        assert!(
            spec.aliases
                .iter()
                .any(|alias| alias.contains("trebuchet ms"))
        );
        assert!(spec.coverage.include_printable_ascii);
        assert_eq!(spec.metrics_profile_source, "recorded");
        assert_eq!(spec.source.family, "Liberation Sans");
        assert_eq!(spec.source.style, "Regular");
        assert_eq!(spec.source.version, "2.1.5");
        assert_eq!(spec.source.license, "SIL Open Font License 1.1");
        assert!(spec.source.release_url.contains("/releases/tag/2.1.5"));
        assert!(
            spec.source
                .artifact_url
                .ends_with("liberation-fonts-ttf-2.1.5.tar.gz")
        );
        assert_eq!(spec.source.artifact_sha256.len(), 64);
        assert!(
            spec.source
                .font_path
                .ends_with("LiberationSans-Regular.ttf")
        );
        assert!(spec.source.sha256.len() == 64);
        assert_eq!(
            spec.coverage.fixture_globs,
            ["tests/fixtures/**/*.mmd", "tests/fixtures/**/*.json"]
        );
        assert!(spec.coverage.explicit_fallback_codepoints.is_empty());
        assert!(
            spec.coverage
                .ranges
                .iter()
                .any(|range| range.start == "U+2190"
                    && range.end == "U+2195"
                    && range.description == "common arrows")
        );
    }

    #[test]
    fn rejects_forbidden_profile_ids() {
        for id in ["mermaid-sans-v1", "mermaid-default-v1", "default-sans-v1"] {
            let err = validate_profile_id(id).unwrap_err();
            assert!(err.to_string().contains("mmdflux-sans-v1"));
        }
    }

    #[test]
    fn font_metrics_check_reports_stale_generated_output() {
        let temp = TempGeneratedProfile::with_modified_output();
        let err = run_font_metrics_check(temp.options()).unwrap_err();

        assert!(
            err.to_string()
                .contains("font metrics generated files are out of date")
        );
    }

    #[test]
    fn generator_emits_sorted_json_and_rust_constants() {
        let output = generate_profile_for_test("mmdflux-sans-v1").expect("profile generated");

        assert!(output.json.contains("\"profileId\": \"mmdflux-sans-v1\""));
        assert_eq!(output.rust_profile_module, "mmdflux_sans_v1");
        assert!(
            output
                .rust
                .contains("pub const PROFILE_ID: &str = \"mmdflux-sans-v1\";")
        );
        assert_codepoints_are_sorted(&output.json);
    }

    #[test]
    fn generator_rejects_source_font_sha_mismatch() {
        let mut spec = fixture_spec();
        spec.source.sha256 = "0".repeat(64);
        let err = generate_from_spec_for_test(&spec).unwrap_err();

        assert!(err.to_string().contains("sha256"));
    }
}
