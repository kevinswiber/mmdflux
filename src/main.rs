//! mmdflux CLI — Mermaid diagram to text/SVG renderer.

mod svg_theme_auto;
mod terminal_appearance;

use std::ffi::OsStr;
use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::{env, fmt, fs};

use clap::{Parser, ValueEnum};
use mmdflux::builtins::default_registry;
use mmdflux::format::{Curve, EdgePreset, RoutingStyle};
use mmdflux::graph::GeometryLevel;
use mmdflux::graph::measure::validate_text_metrics_profile_id;
use mmdflux::simplification::PathSimplification;
use mmdflux::{
    ColorWhen, EngineAlgorithmId, LayoutConfig, OutputFormat, Ranker, RenderConfig, SvgThemeConfig,
    SvgThemeMode, TextColorMode, apply_svg_surface_defaults, detect_diagram, render_diagram,
    validate_diagram,
};
use serde::{Deserialize, Serialize};
use svg_theme_auto::{SVG_THEME_AUTO_DEFAULT_SPEC, SvgThemeAutoMap, select_auto_theme_name};
use terminal_appearance::{TerminalAppearance, detect_os_appearance, detect_terminal_appearance};

const CURVE_CANONICAL_VALUES: &str = "basis, linear, linear-sharp, linear-rounded";
const CURVE_ARG_HELP: &str = "SVG curve style (basis, linear, linear-sharp, or linear-rounded). \
     Overrides the curve component of --edge-preset when both are set.";
const SEVERITY_ERROR: &str = "error";
const SEVERITY_WARNING: &str = "warning";

#[derive(Debug, Deserialize, Serialize)]
struct ValidationResult {
    valid: bool,
    #[serde(default)]
    diagnostics: Vec<ValidationDiagnostic>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ValidationDiagnostic {
    #[serde(default)]
    severity: String,
    line: Option<usize>,
    column: Option<usize>,
    message: String,
}

#[derive(Debug, Serialize)]
struct CliLintJson {
    valid: bool,
    errors: Vec<ValidationDiagnostic>,
    warnings: Vec<ValidationDiagnostic>,
}

const STRICT_PARSE_WARNING_PREFIX: &str = "Strict parsing would reject this input:";

fn normalize_validation_result(result: ValidationResult) -> CliLintJson {
    let default_severity = if result.valid {
        SEVERITY_WARNING
    } else {
        SEVERITY_ERROR
    };
    let diagnostics = result
        .diagnostics
        .into_iter()
        .map(|diag| diag.normalized(default_severity))
        .collect::<Vec<_>>();

    let (warnings, errors): (Vec<_>, Vec<_>) = diagnostics
        .into_iter()
        .partition(ValidationDiagnostic::is_warning);

    CliLintJson {
        valid: result.valid && errors.is_empty(),
        errors,
        warnings,
    }
}

impl ValidationDiagnostic {
    fn normalized(mut self, default_severity: &str) -> Self {
        if self.severity.is_empty() {
            self.severity = default_severity.to_string();
        }

        if self.message.contains(STRICT_PARSE_WARNING_PREFIX) {
            self.severity = SEVERITY_ERROR.to_string();
        }

        self
    }

    fn severity_label(&self) -> &str {
        if self.severity.is_empty() {
            SEVERITY_ERROR
        } else {
            self.severity.as_str()
        }
    }

    fn is_warning(&self) -> bool {
        self.severity_label().eq_ignore_ascii_case(SEVERITY_WARNING)
    }
}

impl fmt::Display for ValidationDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.line, self.column) {
            (Some(line), Some(column)) => {
                write!(
                    f,
                    "{}: line {}, column {}: {}",
                    self.severity_label(),
                    line,
                    column,
                    self.message
                )
            }
            (Some(line), None) => {
                write!(
                    f,
                    "{}: line {}: {}",
                    self.severity_label(),
                    line,
                    self.message
                )
            }
            _ => write!(f, "{}: {}", self.severity_label(), self.message),
        }
    }
}

#[derive(Debug, Parser)]
#[command(name = "mmdflux")]
#[command(version)]
#[command(about = "Convert Mermaid diagrams to text, SVG, or MMDS JSON")]
struct Cli {
    /// Input file (reads from stdin if not provided)
    input: Option<PathBuf>,

    /// Output file (prints to stdout if not provided)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Show diagnostic info (detected diagram type)
    #[arg(long)]
    debug: bool,

    /// Output format (text, ascii, svg, or mmds; json is an alias)
    #[arg(short = 'f', long, value_enum, default_value_t = FormatArg::Text)]
    format: FormatArg,

    /// Text and ASCII color policy (off, auto, or always). Explicit --color overrides NO_COLOR.
    #[arg(long)]
    color: Option<ColorWhen>,

    /// Ranking algorithm
    #[arg(long, value_enum, default_value_t = RankerArg::NetworkSimplex)]
    ranker: RankerArg,

    /// Layout nodesep (node spacing)
    #[arg(long)]
    node_spacing: Option<f64>,

    /// Layout ranksep (rank spacing)
    #[arg(long)]
    rank_spacing: Option<f64>,

    /// Layout edgesep (edge segment spacing)
    #[arg(long)]
    edge_spacing: Option<f64>,

    /// Layout margin (translateGraph margin)
    #[arg(long)]
    margin: Option<f64>,

    /// Extra ranksep applied when subgraphs are present (Mermaid clusters)
    #[arg(long)]
    cluster_ranksep: Option<f64>,

    /// Validate input and report diagnostics (no rendering)
    #[arg(long)]
    lint: bool,

    /// Suppress warnings during rendering
    #[arg(short, long)]
    quiet: bool,

    /// Show node IDs alongside labels (e.g., "A: Start")
    #[arg(long)]
    show_ids: bool,

    /// ASCII padding around the diagram
    #[arg(long)]
    padding: Option<usize>,

    /// SVG scale factor
    #[arg(long)]
    svg_scale: Option<f64>,

    /// Named SVG theme to resolve before slot overrides.
    #[arg(long)]
    svg_theme: Option<String>,

    /// Select a concrete SVG theme from terminal appearance before slot overrides.
    /// Accepts light:<theme>,dark:<theme>; if omitted, defaults to light:default,dark:dark.
    #[arg(
        long,
        conflicts_with = "svg_theme",
        require_equals = true,
        num_args = 0..=1,
        default_missing_value = SVG_THEME_AUTO_DEFAULT_SPEC,
        value_name = "MAP"
    )]
    svg_theme_auto: Option<SvgThemeAutoMap>,

    /// SVG theme output mode (static or dynamic).
    #[arg(long, value_enum)]
    svg_theme_mode: Option<SvgThemeModeArg>,

    /// SVG theme background color override.
    #[arg(long)]
    svg_theme_bg: Option<String>,

    /// SVG theme foreground color override.
    #[arg(long)]
    svg_theme_fg: Option<String>,

    /// SVG theme line color override.
    #[arg(long)]
    svg_theme_line: Option<String>,

    /// SVG theme accent color override.
    #[arg(long)]
    svg_theme_accent: Option<String>,

    /// SVG theme muted color override.
    #[arg(long)]
    svg_theme_muted: Option<String>,

    /// SVG theme surface color override.
    #[arg(long)]
    svg_theme_surface: Option<String>,

    /// SVG theme border color override.
    #[arg(long)]
    svg_theme_border: Option<String>,

    /// SVG node padding on x-axis (px)
    #[arg(long)]
    svg_node_padding_x: Option<f64>,

    /// SVG node padding on y-axis (px)
    #[arg(long)]
    svg_node_padding_y: Option<f64>,

    /// Text metrics profile (supported: mmdflux-heuristic-proportional-v1, mmdflux-sans-v1; default: mmdflux-heuristic-proportional-v1).
    #[arg(long)]
    font_metrics_profile: Option<String>,

    /// Edge style preset (straight, polyline, step, smooth-step, curved-step, or basis).
    /// Expands to routing + curve defaults.
    /// `straight` uses direct routing (prefers one segment, but falls back to
    /// node-avoidance geometry when a direct segment would cross node interiors).
    /// Explicit --routing-style / --curve take precedence.
    #[arg(long)]
    edge_preset: Option<String>,

    /// SVG routing style (direct, polyline, or orthogonal).
    /// `direct` prefers a single segment when clear, with collision-aware fallback.
    /// Overrides the routing component of --edge-preset when both are set.
    #[arg(long)]
    routing_style: Option<String>,

    #[arg(long, help = CURVE_ARG_HELP)]
    curve: Option<String>,

    /// SVG corner arc radius (px) for rounded corners.
    /// Clamped to half the shortest adjacent segment length.
    #[arg(long)]
    edge_radius: Option<f64>,

    /// SVG diagram padding (px)
    #[arg(long)]
    svg_diagram_padding: Option<f64>,

    /// Layout engine (flux-layered, mermaid-layered)
    #[arg(long)]
    layout_engine: Option<String>,

    /// MMDS geometry level for JSON output (layout or routed)
    #[arg(long, value_enum)]
    geometry_level: Option<GeometryLevelArg>,

    /// Path simplification level for MMDS and SVG output.
    /// Ignored for text/ASCII.
    #[arg(long, value_enum)]
    path_simplification: Option<PathSimplificationArg>,

    /// Enable tracing output with an EnvFilter directive.
    #[arg(long, value_name = "FILTER")]
    log: Option<String>,

    /// Tracing output format.
    #[arg(long, value_enum, default_value_t = LogFormatArg::Compact)]
    log_format: LogFormatArg,

    /// Write tracing output to a file instead of stderr.
    #[arg(long, value_name = "PATH")]
    log_file: Option<PathBuf>,
}

#[derive(Clone, Copy, ValueEnum, Debug)]
enum FormatArg {
    /// Unicode text output (default)
    Text,
    /// ASCII-only text output
    Ascii,
    /// SVG vector graphics
    Svg,
    /// MMDS structured output (`json` is an alias)
    #[value(name = "mmds", alias = "json")]
    Mmds,
    /// Mermaid syntax output (from MMDS input)
    Mermaid,
}

impl From<FormatArg> for OutputFormat {
    fn from(arg: FormatArg) -> Self {
        match arg {
            FormatArg::Text => OutputFormat::Text,
            FormatArg::Ascii => OutputFormat::Ascii,
            FormatArg::Svg => OutputFormat::Svg,
            FormatArg::Mmds => OutputFormat::Mmds,
            FormatArg::Mermaid => OutputFormat::Mermaid,
        }
    }
}

#[derive(Clone, Copy, ValueEnum, Debug)]
enum RankerArg {
    NetworkSimplex,
    LongestPath,
}

impl From<RankerArg> for Ranker {
    fn from(arg: RankerArg) -> Self {
        match arg {
            RankerArg::NetworkSimplex => Ranker::NetworkSimplex,
            RankerArg::LongestPath => Ranker::LongestPath,
        }
    }
}

#[derive(Clone, Copy, ValueEnum, Debug)]
enum GeometryLevelArg {
    /// Node geometry + edge topology only (default)
    Layout,
    /// Full geometry including routed edge paths
    Routed,
}

impl From<GeometryLevelArg> for GeometryLevel {
    fn from(arg: GeometryLevelArg) -> Self {
        match arg {
            GeometryLevelArg::Layout => GeometryLevel::Layout,
            GeometryLevelArg::Routed => GeometryLevel::Routed,
        }
    }
}

#[derive(Clone, Copy, ValueEnum, Debug)]
enum PathSimplificationArg {
    /// No simplification — all routed waypoints (default)
    None,
    /// Lossless: remove redundant interior points, preserve path shape
    Lossless,
    /// Lossy: start, midpoint, and end only
    Lossy,
    /// Minimal: start and end only
    Minimal,
}

impl From<PathSimplificationArg> for PathSimplification {
    fn from(arg: PathSimplificationArg) -> Self {
        match arg {
            PathSimplificationArg::None => PathSimplification::None,
            PathSimplificationArg::Lossless => PathSimplification::Lossless,
            PathSimplificationArg::Lossy => PathSimplification::Lossy,
            PathSimplificationArg::Minimal => PathSimplification::Minimal,
        }
    }
}

#[derive(Clone, Copy, ValueEnum, Debug)]
enum SvgThemeModeArg {
    Static,
    Dynamic,
}

impl From<SvgThemeModeArg> for SvgThemeMode {
    fn from(arg: SvgThemeModeArg) -> Self {
        match arg {
            SvgThemeModeArg::Static => SvgThemeMode::Static,
            SvgThemeModeArg::Dynamic => SvgThemeMode::Dynamic,
        }
    }
}

#[derive(Clone, Copy, ValueEnum, Debug)]
enum LogFormatArg {
    Compact,
    Pretty,
    Json,
}

fn resolve_curve_from_cli(raw: Option<&str>) -> Result<Option<Curve>, String> {
    raw.map(Curve::parse).transpose().map_err(|err| {
        if err.message.contains("expected one of") {
            err.message
        } else {
            format!("{err} (expected one of: {CURVE_CANONICAL_VALUES})")
        }
    })
}

fn resolve_text_color_mode(
    color_when: Option<ColorWhen>,
    stdout_is_terminal: bool,
    no_color_env: Option<&OsStr>,
) -> TextColorMode {
    if let Some(color_when) = color_when {
        return color_when.resolve(stdout_is_terminal);
    }

    if matches!(no_color_env, Some(value) if !value.is_empty()) {
        return TextColorMode::Plain;
    }

    ColorWhen::Auto.resolve(stdout_is_terminal)
}

fn has_svg_theme_input(cli: &Cli) -> bool {
    cli.svg_theme.is_some()
        || cli.svg_theme_auto.is_some()
        || cli.svg_theme_mode.is_some()
        || cli.svg_theme_bg.is_some()
        || cli.svg_theme_fg.is_some()
        || cli.svg_theme_line.is_some()
        || cli.svg_theme_accent.is_some()
        || cli.svg_theme_muted.is_some()
        || cli.svg_theme_surface.is_some()
        || cli.svg_theme_border.is_some()
}

fn svg_theme_from_cli_with_appearance(
    cli: &Cli,
    terminal_appearance: Option<TerminalAppearance>,
    os_appearance: Option<TerminalAppearance>,
) -> Option<SvgThemeConfig> {
    if !has_svg_theme_input(cli) {
        return None;
    }

    let name = match (&cli.svg_theme, &cli.svg_theme_auto) {
        (_, Some(map)) => {
            Some(select_auto_theme_name(map, terminal_appearance, os_appearance).to_string())
        }
        (theme, None) => theme.clone(),
    };

    Some(SvgThemeConfig {
        name,
        mode: cli.svg_theme_mode.map(Into::into).unwrap_or_default(),
        bg: cli.svg_theme_bg.clone(),
        fg: cli.svg_theme_fg.clone(),
        line: cli.svg_theme_line.clone(),
        accent: cli.svg_theme_accent.clone(),
        muted: cli.svg_theme_muted.clone(),
        surface: cli.svg_theme_surface.clone(),
        border: cli.svg_theme_border.clone(),
    })
}

fn svg_theme_from_cli(cli: &Cli) -> Option<SvgThemeConfig> {
    svg_theme_from_cli_with_appearance(cli, detect_terminal_appearance(), detect_os_appearance())
}

fn resolve_log_filter(cli: &Cli) -> Option<String> {
    if let Some(filter) = cli.log.as_deref().filter(|value| !value.trim().is_empty()) {
        return Some(filter.to_string());
    }

    env::var("MMDFLUX_LOG")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var("RUST_LOG")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
}

fn init_tracing(cli: &Cli) -> io::Result<()> {
    let Some(filter) = resolve_log_filter(cli) else {
        return Ok(());
    };

    let env_filter = tracing_subscriber::EnvFilter::try_new(&filter).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid log filter: {error}"),
        )
    })?;

    match &cli.log_file {
        Some(path) => {
            let writer = SharedLogWriter::new(fs::File::create(path)?);
            init_tracing_with_writer(env_filter, cli.log_format, move || writer.clone())
        }
        None => init_tracing_with_writer(env_filter, cli.log_format, io::stderr),
    }
}

fn init_tracing_with_writer<W>(
    env_filter: tracing_subscriber::EnvFilter,
    log_format: LogFormatArg,
    make_writer: W,
) -> io::Result<()>
where
    W: for<'writer> tracing_subscriber::fmt::MakeWriter<'writer> + Send + Sync + 'static,
{
    let builder = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(make_writer)
        .with_ansi(false)
        .with_target(true);

    let result = match log_format {
        LogFormatArg::Compact => builder.compact().try_init(),
        LogFormatArg::Pretty => builder.pretty().try_init(),
        LogFormatArg::Json => builder.json().try_init(),
    };

    result.map_err(|error| io::Error::other(format!("failed to initialize tracing: {error}")))
}

#[derive(Clone)]
struct SharedLogWriter {
    file: Arc<Mutex<fs::File>>,
}

impl SharedLogWriter {
    fn new(file: fs::File) -> Self {
        Self {
            file: Arc::new(Mutex::new(file)),
        }
    }
}

impl Write for SharedLogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file
            .lock()
            .map_err(|_| io::Error::other("log file lock poisoned"))?
            .write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file
            .lock()
            .map_err(|_| io::Error::other("log file lock poisoned"))?
            .flush()
    }
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();
    init_tracing(&cli)?;

    let input = match &cli.input {
        Some(path) => fs::read_to_string(path)?,
        None => {
            let mut buffer = String::new();
            io::stdin().read_to_string(&mut buffer)?;
            buffer
        }
    };

    let format: OutputFormat = cli.format.into();
    let no_color_env = env::var_os("NO_COLOR");
    let text_color_mode = resolve_text_color_mode(
        cli.color,
        cli.output.is_none() && io::stdout().is_terminal(),
        no_color_env.as_deref(),
    );

    // Lint mode: validate and exit
    if cli.lint {
        let json = validate_diagram(&input);
        let result: ValidationResult = serde_json::from_str(&json).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("failed to parse validation output: {error}"),
            )
        })?;
        let lint_json = normalize_validation_result(result);

        if matches!(format, OutputFormat::Mmds) {
            println!(
                "{}",
                serde_json::to_string(&lint_json).expect("lint JSON serialization should succeed")
            );
        } else {
            for diag in &lint_json.errors {
                eprintln!("{}", diag);
            }
            for diag in &lint_json.warnings {
                eprintln!("{}", diag);
            }
        }

        std::process::exit(if lint_json.valid { 0 } else { 1 });
    }

    // Parse CLI style flags.
    let edge_preset: Option<EdgePreset> = match cli.edge_preset.as_deref() {
        Some(s) => match EdgePreset::parse(s) {
            Ok(p) => Some(p),
            Err(err) => {
                eprintln!("Error: {}", err);
                std::process::exit(1);
            }
        },
        None => None,
    };

    let routing_style: Option<RoutingStyle> = match cli.routing_style.as_deref() {
        Some(s) => match RoutingStyle::parse(s) {
            Ok(rs) => Some(rs),
            Err(err) => {
                eprintln!("Error: {}", err);
                std::process::exit(1);
            }
        },
        None => None,
    };

    let curve = match resolve_curve_from_cli(cli.curve.as_deref()) {
        Ok(curve) => curve,
        Err(err) => {
            eprintln!("Error: {}", err);
            std::process::exit(1);
        }
    };

    let engine_algo: Option<EngineAlgorithmId> = match cli
        .layout_engine
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        Some(raw) => match EngineAlgorithmId::parse(raw) {
            Ok(id) => {
                if let Err(err) = id.check_available() {
                    eprintln!("Error: {}", err);
                    std::process::exit(1);
                }
                Some(id)
            }
            Err(err) => {
                eprintln!("Error: {}", err);
                std::process::exit(1);
            }
        },
        None => None,
    };

    let font_metrics_profile = match cli.font_metrics_profile.as_deref() {
        Some(profile_id) => match validate_text_metrics_profile_id(profile_id) {
            Ok(()) => Some(profile_id.to_string()),
            Err(err) => {
                eprintln!("Error: {err}");
                std::process::exit(1);
            }
        },
        None => None,
    };

    // Build render config from CLI flags.
    let mut config = RenderConfig {
        layout: LayoutConfig {
            node_sep: cli.node_spacing.unwrap_or(50.0),
            edge_sep: cli.edge_spacing.unwrap_or(20.0),
            rank_sep: cli.rank_spacing.unwrap_or(50.0),
            margin: cli.margin.unwrap_or(8.0),
            ranker: cli.ranker.into(),
            ..LayoutConfig::default()
        },
        layout_engine: engine_algo,
        cluster_ranksep: cli.cluster_ranksep,
        padding: cli.padding,
        text_color_mode,
        svg_scale: cli.svg_scale,
        svg_node_padding_x: cli.svg_node_padding_x,
        svg_node_padding_y: cli.svg_node_padding_y,
        edge_preset,
        routing_style,
        curve,
        edge_radius: cli.edge_radius,
        svg_diagram_padding: cli.svg_diagram_padding,
        svg_theme: svg_theme_from_cli(&cli),
        font_metrics_profile,
        show_ids: cli.show_ids,
        geometry_level: cli.geometry_level.map(Into::into).unwrap_or_default(),
        path_simplification: cli.path_simplification.map(Into::into).unwrap_or_default(),
    };
    // CLI does not force engine for SVG (auto-detect later).
    apply_svg_surface_defaults(format, &mut config, false);

    // Detect diagram type first for CLI-specific error formatting.
    let diagram_id = match detect_diagram(&input) {
        Some(id) => id,
        None => {
            eprintln!("Error: Unknown diagram type");
            std::process::exit(1);
        }
    };

    if cli.debug {
        eprintln!("Detected diagram type: {}", diagram_id);
    }

    // Collect and print warnings to stderr (unless --quiet).
    if !cli.quiet
        && let Some(instance) = default_registry().create(diagram_id)
    {
        let warnings = instance.validation_warnings(&input);
        for w in &warnings {
            let diag = ValidationDiagnostic {
                severity: w.severity.clone(),
                line: w.line,
                column: w.column,
                message: w.message.clone(),
            };
            eprintln!("{diag}");
        }
    }

    // Render through the shared facade contract.
    match render_diagram(&input, format, &config) {
        Ok(output) => match &cli.output {
            Some(path) => fs::write(path, &output)?,
            None => print!("{}", output),
        },
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use clap::error::ErrorKind;

    use super::*;

    fn parse_cli(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).expect("CLI args should parse")
    }

    fn render_svg_with_config(input: &str, config: RenderConfig) -> String {
        render_diagram(input, OutputFormat::Svg, &config).expect("SVG render should succeed")
    }

    #[test]
    fn color_auto_defaults_to_plain_when_stdout_is_not_a_terminal() {
        assert_eq!(
            resolve_text_color_mode(None, false, None),
            TextColorMode::Plain
        );
    }

    #[test]
    fn no_color_env_disables_default_auto_color_on_terminal() {
        assert_eq!(
            resolve_text_color_mode(None, true, Some(OsStr::new("1"))),
            TextColorMode::Plain
        );
        assert_eq!(
            resolve_text_color_mode(None, true, Some(OsStr::new("true"))),
            TextColorMode::Plain
        );
    }

    #[test]
    fn empty_no_color_env_does_not_disable_default_auto_color_on_terminal() {
        assert_eq!(
            resolve_text_color_mode(None, true, Some(OsStr::new(""))),
            TextColorMode::Ansi
        );
    }

    #[test]
    fn explicit_color_flag_overrides_no_color_env() {
        assert_eq!(
            resolve_text_color_mode(Some(ColorWhen::Always), true, Some(OsStr::new("1"))),
            TextColorMode::Ansi
        );
        assert_eq!(
            resolve_text_color_mode(Some(ColorWhen::Off), true, Some(OsStr::new("1"))),
            TextColorMode::Plain
        );
        assert_eq!(
            resolve_text_color_mode(Some(ColorWhen::Auto), true, Some(OsStr::new("1"))),
            TextColorMode::Ansi
        );
    }

    #[test]
    fn cli_parses_bare_svg_theme_auto_as_default_map() {
        let cli = parse_cli(&["mmdflux", "--svg-theme-auto"]);
        assert_eq!(cli.svg_theme_auto, Some(SvgThemeAutoMap::default()));
    }

    #[test]
    fn cli_parses_custom_svg_theme_auto_map() {
        let cli = parse_cli(&["mmdflux", "--svg-theme-auto=dark:dracula, light:zinc-light"]);
        assert_eq!(
            cli.svg_theme_auto,
            Some(SvgThemeAutoMap {
                light: "zinc-light".to_string(),
                dark: "dracula".to_string(),
            })
        );
    }

    #[test]
    fn cli_rejects_invalid_svg_theme_auto_maps() {
        for value in [
            "",
            "light:default",
            "light:default,dark:dark,dark:dracula",
            "light:default,auto:dark",
            "light:,dark:dark",
        ] {
            let error = Cli::try_parse_from(["mmdflux", &format!("--svg-theme-auto={value}")])
                .expect_err("invalid svg auto theme map should fail");
            assert_eq!(error.kind(), ErrorKind::ValueValidation);
        }
    }

    #[test]
    fn cli_rejects_svg_theme_and_svg_theme_auto_together() {
        let error = Cli::try_parse_from(["mmdflux", "--svg-theme", "dark", "--svg-theme-auto"])
            .expect_err("conflicting theme sources should fail");
        assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
    }

    #[test]
    fn svg_theme_auto_resolves_to_explicit_theme_with_slot_overrides() {
        let cli = parse_cli(&[
            "mmdflux",
            "--svg-theme-auto=light:zinc-light,dark:dracula",
            "--svg-theme-mode",
            "dynamic",
            "--svg-theme-accent",
            "#7dd3fc",
        ]);

        let theme = svg_theme_from_cli_with_appearance(
            &cli,
            Some(TerminalAppearance::Dark),
            Some(TerminalAppearance::Light),
        )
        .expect("theme should resolve");

        assert_eq!(theme.name.as_deref(), Some("dracula"));
        assert_eq!(theme.mode, SvgThemeMode::Dynamic);
        assert_eq!(theme.accent.as_deref(), Some("#7dd3fc"));
    }

    #[test]
    fn svg_theme_auto_falls_back_from_terminal_to_os_to_light_map() {
        let cli = parse_cli(&["mmdflux", "--svg-theme-auto=light:zinc-light,dark:dracula"]);

        let mac_dark =
            svg_theme_from_cli_with_appearance(&cli, None, Some(TerminalAppearance::Dark))
                .expect("theme should resolve");
        assert_eq!(mac_dark.name.as_deref(), Some("dracula"));

        let fallback_light =
            svg_theme_from_cli_with_appearance(&cli, None, None).expect("theme should resolve");
        assert_eq!(fallback_light.name.as_deref(), Some("zinc-light"));
    }

    #[test]
    fn svg_theme_auto_suppresses_mermaid_theme_hints() {
        let input = "%%{init: {\"theme\": \"forest\"}}%%\ngraph TD\nA-->B\n";
        let cli = parse_cli(&["mmdflux", "--svg-theme-auto=light:default,dark:dark"]);

        let auto_theme = svg_theme_from_cli_with_appearance(
            &cli,
            Some(TerminalAppearance::Dark),
            Some(TerminalAppearance::Light),
        )
        .expect("theme should resolve");

        let auto_output = render_svg_with_config(
            input,
            RenderConfig {
                svg_theme: Some(auto_theme),
                ..RenderConfig::default()
            },
        );
        let explicit_output = render_svg_with_config(
            input,
            RenderConfig {
                svg_theme: Some(SvgThemeConfig {
                    name: Some("dark".to_string()),
                    ..SvgThemeConfig::default()
                }),
                ..RenderConfig::default()
            },
        );

        assert_eq!(auto_output, explicit_output);
    }
}
