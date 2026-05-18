//! Internal SVG theme resolution and CSS metadata helpers.
//!
//! Runtime owns whether a diagram is themed at all. When it chooses to apply a
//! theme, this module turns the public facade config into concrete SVG slot
//! colors, derived renderer roles, and optional browser-only CSS metadata.

/// Render-owned SVG theme mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SvgThemeRenderMode {
    #[default]
    Static,
    Dynamic,
}

/// Render-owned SVG theme input spec.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SvgThemeSpec {
    pub name: Option<String>,
    pub mode: SvgThemeRenderMode,
    pub bg: Option<String>,
    pub fg: Option<String>,
    pub line: Option<String>,
    pub accent: Option<String>,
    pub muted: Option<String>,
    pub surface: Option<String>,
    pub border: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SvgThemeError {
    pub message: String,
}

impl std::fmt::Display for SvgThemeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SvgThemeError {}

const MIX_NODE_FILL: f64 = 0.03;
const MIX_NODE_STROKE: f64 = 0.20;
const MIX_TEXT_FAINT: f64 = 0.25;
const MIX_TEXT_MUTED: f64 = 0.40;
const MIX_LINE: f64 = 0.50;
const MIX_TEXT_SECONDARY: f64 = 0.60;
const MIX_ARROW: f64 = 0.85;

const DEFAULT_BG: &str = "#ffffff";
const DEFAULT_FG: &str = "#27272a";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SvgRootStyle {
    pub background_color: Option<String>,
    pub css_variables: Vec<(String, String)>,
    pub style_block: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedSvgTheme {
    pub canonical_name: Option<String>,
    pub slots: ResolvedThemeSlots,
    pub roles: DerivedThemeRoles,
    pub dynamic: Option<DynamicThemeMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedThemeSlots {
    pub bg: String,
    pub fg: String,
    pub line: String,
    pub accent: String,
    pub muted: String,
    pub surface: String,
    pub border: String,
    pub cluster_bkg: String,
    pub note_fill: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DerivedThemeRoles {
    pub background: String,
    pub text: String,
    pub text_secondary: String,
    pub text_muted: String,
    pub text_faint: String,
    pub line: String,
    pub arrow: String,
    pub node_fill: String,
    pub node_stroke: String,
    pub group_fill: String,
    pub cluster_bkg: String,
    pub note_fill: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DynamicThemeMetadata {
    pub root_style: SvgRootStyle,
    pub style_block: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NamedSvgThemeDefinition {
    pub canonical_name: &'static str,
    pub seed: ThemeSeed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ThemeSeed {
    pub bg: &'static str,
    pub fg: &'static str,
    pub line: Option<&'static str>,
    pub accent: Option<&'static str>,
    pub muted: Option<&'static str>,
    pub surface: Option<&'static str>,
    pub border: Option<&'static str>,
    /// Mermaid `clusterBkg` parity — background fill for subgraph cluster rects.
    /// `None` falls back to `surface` (matching node fill) inside
    /// `resolve_optional_slot`.
    pub cluster_bkg: Option<&'static str>,
    /// Optional sticky-note background fill (Mermaid's `noteBkgColor`).
    /// When `None`, falls back to `surface` so themes without an explicit
    /// value keep the post-#357 behavior (notes share the participant/node
    /// fill) instead of regressing to a near-bg mix.
    pub note_fill: Option<&'static str>,
}

const BEAUTIFUL_MERMAID_THEMES: &[NamedSvgThemeDefinition] = &[
    named_theme(
        "zinc-light",
        ThemeSeed {
            bg: "#ffffff",
            fg: "#27272a",
            line: None,
            accent: None,
            muted: None,
            surface: None,
            border: None,
            cluster_bkg: None,
            note_fill: Some("#fef9c3"),
        },
    ),
    named_theme(
        "zinc-dark",
        ThemeSeed {
            bg: "#18181b",
            fg: "#fafafa",
            line: None,
            accent: None,
            muted: None,
            surface: None,
            border: None,
            cluster_bkg: None,
            note_fill: Some("#854d0e"),
        },
    ),
    named_theme(
        "tokyo-night",
        ThemeSeed {
            bg: "#1a1b26",
            fg: "#a9b1d6",
            line: Some("#3d59a1"),
            accent: Some("#7aa2f7"),
            muted: Some("#565f89"),
            surface: None,
            border: None,
            cluster_bkg: None,
            note_fill: None,
        },
    ),
    named_theme(
        "tokyo-night-storm",
        ThemeSeed {
            bg: "#24283b",
            fg: "#a9b1d6",
            line: Some("#3d59a1"),
            accent: Some("#7aa2f7"),
            muted: Some("#565f89"),
            surface: None,
            border: None,
            cluster_bkg: None,
            note_fill: None,
        },
    ),
    named_theme(
        "tokyo-night-light",
        ThemeSeed {
            bg: "#d5d6db",
            fg: "#343b58",
            line: Some("#34548a"),
            accent: Some("#34548a"),
            muted: Some("#9699a3"),
            surface: None,
            border: None,
            cluster_bkg: None,
            note_fill: None,
        },
    ),
    named_theme(
        "catppuccin-mocha",
        ThemeSeed {
            bg: "#1e1e2e",
            fg: "#cdd6f4",
            line: Some("#585b70"),
            accent: Some("#cba6f7"),
            muted: Some("#6c7086"),
            surface: None,
            border: None,
            cluster_bkg: None,
            note_fill: None,
        },
    ),
    named_theme(
        "catppuccin-latte",
        ThemeSeed {
            bg: "#eff1f5",
            fg: "#4c4f69",
            line: Some("#9ca0b0"),
            accent: Some("#8839ef"),
            muted: Some("#9ca0b0"),
            surface: None,
            border: None,
            cluster_bkg: None,
            note_fill: None,
        },
    ),
    named_theme(
        "nord",
        ThemeSeed {
            bg: "#2e3440",
            fg: "#d8dee9",
            line: Some("#4c566a"),
            accent: Some("#88c0d0"),
            muted: Some("#616e88"),
            surface: None,
            border: None,
            cluster_bkg: None,
            note_fill: None,
        },
    ),
    named_theme(
        "nord-light",
        ThemeSeed {
            bg: "#eceff4",
            fg: "#2e3440",
            line: Some("#aab1c0"),
            accent: Some("#5e81ac"),
            muted: Some("#7b88a1"),
            surface: None,
            border: None,
            cluster_bkg: None,
            note_fill: None,
        },
    ),
    named_theme(
        "dracula",
        ThemeSeed {
            bg: "#282a36",
            fg: "#f8f8f2",
            line: Some("#6272a4"),
            accent: Some("#bd93f9"),
            muted: Some("#6272a4"),
            surface: None,
            border: None,
            cluster_bkg: None,
            note_fill: None,
        },
    ),
    named_theme(
        "github-light",
        ThemeSeed {
            bg: "#ffffff",
            fg: "#1f2328",
            line: Some("#d1d9e0"),
            accent: Some("#0969da"),
            muted: Some("#59636e"),
            surface: None,
            border: None,
            cluster_bkg: None,
            note_fill: None,
        },
    ),
    named_theme(
        "github-dark",
        ThemeSeed {
            bg: "#0d1117",
            fg: "#e6edf3",
            line: Some("#3d444d"),
            accent: Some("#4493f8"),
            muted: Some("#9198a1"),
            surface: None,
            border: None,
            cluster_bkg: None,
            note_fill: None,
        },
    ),
    named_theme(
        "solarized-light",
        ThemeSeed {
            bg: "#fdf6e3",
            fg: "#657b83",
            line: Some("#93a1a1"),
            accent: Some("#268bd2"),
            muted: Some("#93a1a1"),
            surface: None,
            border: None,
            cluster_bkg: None,
            note_fill: None,
        },
    ),
    named_theme(
        "solarized-dark",
        ThemeSeed {
            bg: "#002b36",
            fg: "#839496",
            line: Some("#586e75"),
            accent: Some("#268bd2"),
            muted: Some("#586e75"),
            surface: None,
            border: None,
            cluster_bkg: None,
            note_fill: None,
        },
    ),
    named_theme(
        "one-dark",
        ThemeSeed {
            bg: "#282c34",
            fg: "#abb2bf",
            line: Some("#4b5263"),
            accent: Some("#c678dd"),
            muted: Some("#5c6370"),
            surface: None,
            border: None,
            cluster_bkg: None,
            note_fill: None,
        },
    ),
];

const MERMAID_THEMES: &[NamedSvgThemeDefinition] = &[
    named_theme(
        "default",
        ThemeSeed {
            bg: "#ffffff",
            fg: "#333333",
            line: Some("#333333"),
            accent: Some("#333333"),
            muted: Some("#666666"),
            surface: Some("#ececff"),
            border: Some("#9370db"),
            // Mermaid base theme: clusterBkg = tertiaryColor (#ECECFF).
            cluster_bkg: Some("#ececff"),
            note_fill: Some("#fff5ad"),
        },
    ),
    named_theme(
        "dark",
        ThemeSeed {
            bg: "#333333",
            fg: "#cccccc",
            line: Some("#d3d3d3"),
            accent: Some("#d3d3d3"),
            muted: Some("#cccccc"),
            surface: Some("#1f2020"),
            border: Some("#cccccc"),
            // Mermaid dark theme: clusterBkg = '#302F3D' (explicit).
            cluster_bkg: Some("#302f3d"),
            note_fill: Some("#fff5ad"),
        },
    ),
    named_theme(
        "forest",
        ThemeSeed {
            bg: "#ffffff",
            fg: "#333333",
            line: Some("#13540c"),
            accent: Some("#13540c"),
            muted: Some("#333333"),
            surface: Some("#cde498"),
            border: Some("#13540c"),
            // Mermaid forest theme: clusterBkg = secondBkg = '#cdffb2'.
            cluster_bkg: Some("#cdffb2"),
            // Mermaid forest: noteBkgColor = '#fff5ad' (theme-forest.js:59).
            note_fill: Some("#fff5ad"),
        },
    ),
    named_theme(
        "neutral",
        ThemeSeed {
            bg: "#ffffff",
            fg: "#333333",
            line: Some("#666666"),
            accent: Some("#333333"),
            muted: Some("#333333"),
            surface: Some("#eeeeee"),
            border: Some("#999999"),
            // Mermaid neutral theme: clusterBkg = secondBkg, calculated from
            // contrast (#707070) via lighten(.., 55); resolves to ~#eeeeee.
            cluster_bkg: Some("#eeeeee"),
            // Mermaid neutral: noteBkgColor = '#666' (theme-neutral.js:145).
            note_fill: Some("#666666"),
        },
    ),
];

const fn named_theme(canonical_name: &'static str, seed: ThemeSeed) -> NamedSvgThemeDefinition {
    NamedSvgThemeDefinition {
        canonical_name,
        seed,
    }
}

pub(crate) fn resolve_theme_name(name: &str) -> Result<NamedSvgThemeDefinition, SvgThemeError> {
    let normalized = name.trim().to_ascii_lowercase();
    let canonical = match normalized.as_str() {
        "base" => "default",
        other => other,
    };

    BEAUTIFUL_MERMAID_THEMES
        .iter()
        .chain(MERMAID_THEMES.iter())
        .copied()
        .find(|theme| theme.canonical_name == canonical)
        .ok_or_else(|| SvgThemeError {
            message: format!("unknown SVG theme `{name}`"),
        })
}

pub(crate) fn resolve_svg_theme(config: &SvgThemeSpec) -> Result<ResolvedSvgTheme, SvgThemeError> {
    let named = config.name.as_deref().map(resolve_theme_name).transpose()?;

    let bg = resolve_slot(
        config.bg.as_deref(),
        named.map(|theme| theme.seed.bg),
        DEFAULT_BG,
    )?;
    let fg = resolve_slot(
        config.fg.as_deref(),
        named.map(|theme| theme.seed.fg),
        DEFAULT_FG,
    )?;

    let line = resolve_optional_slot(
        config.line.as_deref(),
        named.and_then(|theme| theme.seed.line),
        &fg,
        &bg,
        MIX_LINE,
    )?;
    let accent = resolve_optional_slot(
        config.accent.as_deref(),
        named.and_then(|theme| theme.seed.accent),
        &fg,
        &bg,
        MIX_ARROW,
    )?;
    let muted = resolve_optional_slot(
        config.muted.as_deref(),
        named.and_then(|theme| theme.seed.muted),
        &fg,
        &bg,
        MIX_TEXT_MUTED,
    )?;
    let surface = resolve_optional_slot(
        config.surface.as_deref(),
        named.and_then(|theme| theme.seed.surface),
        &fg,
        &bg,
        MIX_NODE_FILL,
    )?;
    let border = resolve_optional_slot(
        config.border.as_deref(),
        named.and_then(|theme| theme.seed.border),
        &fg,
        &bg,
        MIX_NODE_STROKE,
    )?;
    // `cluster_bkg` mirrors Mermaid's `clusterBkg` theme variable. There is no
    // direct public override hook on `SvgThemeSpec`; the seed comes from the
    // named theme, and otherwise falls back to the resolved `surface` slot so
    // unseeded themes (beautiful palettes, custom slot overrides) keep behaving
    // like a tinted node background instead of leaking `bg` through.
    let cluster_bkg = match named.and_then(|theme| theme.seed.cluster_bkg) {
        Some(color) => normalize_hex_color(color)?,
        None => surface.clone(),
    };

    // `note_fill` falls back to `surface` so themes without an explicit
    // sticky-note seed keep the post-#357 behavior (notes share the
    // participant/node fill) instead of regressing to a near-bg mix.
    let note_fill = match named.and_then(|theme| theme.seed.note_fill) {
        Some(value) => normalize_hex_color(value)?,
        None => surface.clone(),
    };

    let slots = ResolvedThemeSlots {
        bg: bg.clone(),
        fg: fg.clone(),
        line,
        accent,
        muted,
        surface,
        border,
        cluster_bkg,
        note_fill,
    };

    let roles = DerivedThemeRoles {
        background: slots.bg.clone(),
        text: slots.fg.clone(),
        text_secondary: mix_hex(&slots.fg, &slots.bg, MIX_TEXT_SECONDARY)?,
        text_muted: slots.muted.clone(),
        text_faint: mix_hex(&slots.fg, &slots.bg, MIX_TEXT_FAINT)?,
        line: slots.line.clone(),
        arrow: slots.accent.clone(),
        node_fill: slots.surface.clone(),
        node_stroke: slots.border.clone(),
        group_fill: slots.bg.clone(),
        cluster_bkg: slots.cluster_bkg.clone(),
        note_fill: slots.note_fill.clone(),
    };

    let dynamic = if matches!(config.mode, SvgThemeRenderMode::Dynamic) {
        let style_block = build_dynamic_style_block();
        Some(DynamicThemeMetadata {
            root_style: SvgRootStyle {
                background_color: Some(slots.bg.clone()),
                css_variables: vec![
                    ("--bg".into(), slots.bg.clone()),
                    ("--fg".into(), slots.fg.clone()),
                    ("--line".into(), slots.line.clone()),
                    ("--accent".into(), slots.accent.clone()),
                    ("--muted".into(), slots.muted.clone()),
                    ("--surface".into(), slots.surface.clone()),
                    ("--border".into(), slots.border.clone()),
                    ("--cluster-bkg".into(), slots.cluster_bkg.clone()),
                    ("--note-fill".into(), slots.note_fill.clone()),
                ],
                style_block: Some(style_block.clone()),
            },
            style_block,
        })
    } else {
        None
    };

    Ok(ResolvedSvgTheme {
        canonical_name: named.map(|theme| theme.canonical_name.to_string()),
        slots,
        roles,
        dynamic,
    })
}

fn resolve_slot(
    explicit: Option<&str>,
    named: Option<&str>,
    default: &str,
) -> Result<String, SvgThemeError> {
    normalize_hex_color(explicit.or(named).unwrap_or(default))
}

fn resolve_optional_slot(
    explicit: Option<&str>,
    named: Option<&str>,
    fg: &str,
    bg: &str,
    mix_pct: f64,
) -> Result<String, SvgThemeError> {
    match explicit.or(named) {
        Some(color) => normalize_hex_color(color),
        None => mix_hex(fg, bg, mix_pct),
    }
}

fn normalize_hex_color(input: &str) -> Result<String, SvgThemeError> {
    let parsed = parse_hex_color(input)?;
    Ok(format!(
        "#{:02x}{:02x}{:02x}",
        parsed[0], parsed[1], parsed[2]
    ))
}

fn parse_hex_color(input: &str) -> Result<[u8; 3], SvgThemeError> {
    let value = input.trim();
    let raw = value.strip_prefix('#').ok_or_else(|| SvgThemeError {
        message: format!("invalid SVG theme color `{input}`"),
    })?;

    match raw.len() {
        3 => {
            let mut out = [0_u8; 3];
            for (idx, ch) in raw.chars().enumerate() {
                let digit = ch.to_digit(16).ok_or_else(|| SvgThemeError {
                    message: format!("invalid SVG theme color `{input}`"),
                })? as u8;
                out[idx] = digit * 17;
            }
            Ok(out)
        }
        6 => {
            let mut out = [0_u8; 3];
            for (idx, chunk) in raw.as_bytes().chunks_exact(2).enumerate() {
                let chunk = std::str::from_utf8(chunk).map_err(|_| SvgThemeError {
                    message: format!("invalid SVG theme color `{input}`"),
                })?;
                out[idx] = u8::from_str_radix(chunk, 16).map_err(|_| SvgThemeError {
                    message: format!("invalid SVG theme color `{input}`"),
                })?;
            }
            Ok(out)
        }
        _ => Err(SvgThemeError {
            message: format!("invalid SVG theme color `{input}`"),
        }),
    }
}

fn mix_hex(fg: &str, bg: &str, pct: f64) -> Result<String, SvgThemeError> {
    let fg = parse_hex_color(fg)?;
    let bg = parse_hex_color(bg)?;
    let mix_channel =
        |fg: u8, bg: u8| -> u8 { ((fg as f64 * pct) + (bg as f64 * (1.0 - pct))).round() as u8 };

    Ok(format!(
        "#{:02x}{:02x}{:02x}",
        mix_channel(fg[0], bg[0]),
        mix_channel(fg[1], bg[1]),
        mix_channel(fg[2], bg[2]),
    ))
}

fn build_dynamic_style_block() -> String {
    [
        "svg {",
        "  --_text: var(--fg);",
        "  --_text-sec: color-mix(in srgb, var(--fg) 60%, var(--bg));",
        "  --_text-muted: var(--muted);",
        "  --_text-faint: color-mix(in srgb, var(--fg) 25%, var(--bg));",
        "  --_line: var(--line);",
        "  --_arrow: var(--accent);",
        "  --_node-fill: var(--surface);",
        "  --_node-stroke: var(--border);",
        "  --_group-fill: var(--bg);",
        "  --_cluster-bkg: var(--cluster-bkg);",
        "  --_note-fill: var(--note-fill);",
        "}",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{SvgThemeRenderMode, SvgThemeSpec, resolve_svg_theme, resolve_theme_name};

    #[test]
    fn named_mermaid_aliases_resolve_to_expected_base_theme() {
        assert_eq!(
            resolve_theme_name("base").unwrap().canonical_name,
            "default"
        );
        assert_eq!(resolve_theme_name("dark").unwrap().canonical_name, "dark");
        assert_eq!(
            resolve_theme_name("forest").unwrap().canonical_name,
            "forest"
        );
        assert_eq!(
            resolve_theme_name("neutral").unwrap().canonical_name,
            "neutral"
        );
    }

    #[test]
    fn beautiful_mermaid_builtin_palette_values_are_pinned() {
        let resolved = resolve_theme_name("tokyo-night").unwrap();

        assert_eq!(resolved.seed.bg, "#1a1b26");
        assert_eq!(resolved.seed.fg, "#a9b1d6");
        assert_eq!(resolved.seed.line, Some("#3d59a1"));
        assert_eq!(resolved.seed.accent, Some("#7aa2f7"));
        assert_eq!(resolved.seed.muted, Some("#565f89"));
    }

    #[test]
    fn slot_overrides_apply_on_top_of_named_theme() {
        let resolved = resolve_svg_theme(&SvgThemeSpec {
            name: Some("dark".into()),
            accent: Some("#ff00aa".into()),
            ..Default::default()
        })
        .unwrap();

        assert_eq!(resolved.canonical_name.as_deref(), Some("dark"));
        assert_eq!(resolved.slots.accent, "#ff00aa");
    }

    #[test]
    fn invalid_theme_names_fail_cleanly() {
        let err = resolve_theme_name("not-a-real-theme").unwrap_err();
        assert!(err.message.contains("unknown SVG theme"));
    }

    #[test]
    fn invalid_slot_color_tokens_fail_cleanly() {
        let err = resolve_svg_theme(&SvgThemeSpec {
            bg: Some("not-a-color".into()),
            ..Default::default()
        })
        .unwrap_err();

        assert!(err.message.contains("invalid SVG theme color"));
    }

    #[test]
    fn derived_roles_follow_the_approved_mix_ratios() {
        let resolved = resolve_svg_theme(&SvgThemeSpec {
            bg: Some("#ffffff".into()),
            fg: Some("#27272a".into()),
            ..Default::default()
        })
        .unwrap();

        assert_eq!(resolved.slots.line, "#939395");
        assert_eq!(resolved.slots.accent, "#47474a");
        assert_eq!(resolved.slots.muted, "#a9a9aa");
        assert_eq!(resolved.slots.surface, "#f9f9f9");
        assert_eq!(resolved.slots.border, "#d4d4d4");
        assert_eq!(resolved.roles.text, "#27272a");
        assert_eq!(resolved.roles.text_secondary, "#7d7d7f");
        assert_eq!(resolved.roles.text_muted, "#a9a9aa");
        assert_eq!(resolved.roles.text_faint, "#c9c9ca");
    }

    #[test]
    fn dynamic_mode_includes_public_root_variables_and_css_payload() {
        let resolved = resolve_svg_theme(&SvgThemeSpec {
            mode: SvgThemeRenderMode::Dynamic,
            ..Default::default()
        })
        .unwrap();

        let dynamic = resolved
            .dynamic
            .expect("dynamic metadata should be present");
        let root_vars: Vec<_> = dynamic
            .root_style
            .css_variables
            .iter()
            .map(|(name, value)| format!("{name}:{value}"))
            .collect();

        assert!(root_vars.contains(&"--bg:#ffffff".to_string()));
        assert!(root_vars.contains(&"--fg:#27272a".to_string()));
        assert!(root_vars.contains(&"--line:#939395".to_string()));
        assert!(root_vars.contains(&"--accent:#47474a".to_string()));
        assert!(root_vars.contains(&"--muted:#a9a9aa".to_string()));
        assert!(root_vars.contains(&"--surface:#f9f9f9".to_string()));
        assert!(root_vars.contains(&"--border:#d4d4d4".to_string()));
        // Unseeded themes fall the cluster_bkg slot back to `surface` so the
        // dynamic CSS payload still surfaces a value (no surprise blanks).
        assert!(root_vars.contains(&"--cluster-bkg:#f9f9f9".to_string()));
        assert!(dynamic.style_block.contains("--_text: var(--fg);"));
        assert!(dynamic.style_block.contains("--_line: var(--line);"));
        assert!(
            dynamic
                .style_block
                .contains("--_node-fill: var(--surface);")
        );
        assert!(
            dynamic
                .style_block
                .contains("--_node-stroke: var(--border);")
        );
        assert!(
            dynamic
                .style_block
                .contains("--_cluster-bkg: var(--cluster-bkg);")
        );
    }

    #[test]
    fn mermaid_named_themes_seed_cluster_bkg_to_documented_values() {
        // Mermaid base theme: clusterBkg = tertiaryColor (#ECECFF).
        let base = resolve_svg_theme(&SvgThemeSpec {
            name: Some("default".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(base.slots.cluster_bkg, "#ececff");
        assert_eq!(base.roles.cluster_bkg, "#ececff");

        // Mermaid dark theme pins clusterBkg explicitly as #302F3D.
        let dark = resolve_svg_theme(&SvgThemeSpec {
            name: Some("dark".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(dark.slots.cluster_bkg, "#302f3d");

        // Mermaid forest theme: clusterBkg = secondBkg = #cdffb2 (distinct
        // from the resolved surface slot at #cde498).
        let forest = resolve_svg_theme(&SvgThemeSpec {
            name: Some("forest".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(forest.slots.cluster_bkg, "#cdffb2");
        assert_ne!(forest.slots.cluster_bkg, forest.slots.surface);

        // Mermaid neutral theme: clusterBkg derives to a pale gray; mmdflux
        // seeds #eeeeee to match the established surface tone.
        let neutral = resolve_svg_theme(&SvgThemeSpec {
            name: Some("neutral".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(neutral.slots.cluster_bkg, "#eeeeee");
    }

    #[test]
    fn unseeded_themes_fall_cluster_bkg_back_to_resolved_surface() {
        // Beautiful palette themes have no Mermaid-style clusterBkg parity
        // documentation, so the slot inherits the resolved surface fill.
        let resolved = resolve_svg_theme(&SvgThemeSpec {
            name: Some("tokyo-night".into()),
            ..Default::default()
        })
        .unwrap();

        assert_eq!(resolved.slots.cluster_bkg, resolved.slots.surface);
    }
}
