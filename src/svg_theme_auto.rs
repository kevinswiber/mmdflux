//! CLI-only SVG auto-theme parsing and selection helpers.
//!
//! This stays in the binary so runtime/WASM/library consumers keep a concrete
//! `SvgThemeConfig` contract without inheriting terminal-specific behavior.

use std::str::FromStr;

use crate::terminal_appearance::TerminalAppearance;

pub(crate) const SVG_THEME_AUTO_DEFAULT_SPEC: &str = "light:default,dark:dark";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SvgThemeAutoMap {
    pub(crate) light: String,
    pub(crate) dark: String,
}

impl Default for SvgThemeAutoMap {
    fn default() -> Self {
        Self {
            light: "default".to_string(),
            dark: "dark".to_string(),
        }
    }
}

impl FromStr for SvgThemeAutoMap {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut light = None;
        let mut dark = None;

        for entry in value.split(',') {
            let entry = entry.trim();
            if entry.is_empty() {
                return Err(svg_theme_auto_map_error(
                    "empty entry in --svg-theme-auto map",
                ));
            }

            let (key, theme) = entry.split_once(':').ok_or_else(|| {
                svg_theme_auto_map_error("expected entries in the form light:<theme>,dark:<theme>")
            })?;

            let key = key.trim();
            let theme = theme.trim();
            if theme.is_empty() {
                return Err(svg_theme_auto_map_error(format!(
                    "missing theme name for `{key}`"
                )));
            }

            match key {
                "light" => {
                    if light.replace(theme.to_string()).is_some() {
                        return Err(svg_theme_auto_map_error(
                            "duplicate `light` entry in --svg-theme-auto map",
                        ));
                    }
                }
                "dark" => {
                    if dark.replace(theme.to_string()).is_some() {
                        return Err(svg_theme_auto_map_error(
                            "duplicate `dark` entry in --svg-theme-auto map",
                        ));
                    }
                }
                _ => {
                    return Err(svg_theme_auto_map_error(format!(
                        "unknown key `{key}` in --svg-theme-auto map (expected `light` or `dark`)"
                    )));
                }
            }
        }

        let light = light.ok_or_else(|| {
            svg_theme_auto_map_error("missing `light` entry in --svg-theme-auto map")
        })?;
        let dark = dark.ok_or_else(|| {
            svg_theme_auto_map_error("missing `dark` entry in --svg-theme-auto map")
        })?;

        Ok(Self { light, dark })
    }
}

pub(crate) fn select_auto_theme_name(
    map: &SvgThemeAutoMap,
    terminal_appearance: Option<TerminalAppearance>,
    macos_appearance: Option<TerminalAppearance>,
) -> &str {
    match terminal_appearance.or(macos_appearance) {
        Some(TerminalAppearance::Dark) => map.dark.as_str(),
        Some(TerminalAppearance::Light) | None => map.light.as_str(),
    }
}

fn svg_theme_auto_map_error(message: impl AsRef<str>) -> String {
    format!(
        "{} (expected {})",
        message.as_ref(),
        SVG_THEME_AUTO_DEFAULT_SPEC
    )
}

#[cfg(test)]
mod tests {
    use super::{SVG_THEME_AUTO_DEFAULT_SPEC, SvgThemeAutoMap, select_auto_theme_name};
    use crate::terminal_appearance::TerminalAppearance;

    #[test]
    fn svg_theme_auto_map_parses_keyed_entries_in_any_order() {
        let map: SvgThemeAutoMap = "dark:dracula, light:zinc-light".parse().unwrap();
        assert_eq!(map.light, "zinc-light");
        assert_eq!(map.dark, "dracula");
    }

    #[test]
    fn svg_theme_auto_map_rejects_duplicate_keys() {
        let error = "light:default,light:zinc-light,dark:dark"
            .parse::<SvgThemeAutoMap>()
            .unwrap_err();
        assert!(error.contains("duplicate `light` entry"));
    }

    #[test]
    fn svg_theme_auto_map_rejects_missing_entries() {
        let error = "light:default".parse::<SvgThemeAutoMap>().unwrap_err();
        assert!(error.contains("missing `dark` entry"));
        assert!(error.contains(SVG_THEME_AUTO_DEFAULT_SPEC));
    }

    #[test]
    fn svg_theme_auto_map_rejects_unknown_keys() {
        let error = "light:default,auto:dark"
            .parse::<SvgThemeAutoMap>()
            .unwrap_err();
        assert!(error.contains("unknown key `auto`"));
    }

    #[test]
    fn select_auto_theme_name_prefers_terminal_then_macos_then_light() {
        let map = SvgThemeAutoMap {
            light: "default".to_string(),
            dark: "dark".to_string(),
        };

        assert_eq!(
            select_auto_theme_name(
                &map,
                Some(TerminalAppearance::Dark),
                Some(TerminalAppearance::Light)
            ),
            "dark"
        );
        assert_eq!(
            select_auto_theme_name(&map, None, Some(TerminalAppearance::Dark)),
            "dark"
        );
        assert_eq!(select_auto_theme_name(&map, None, None), "default");
    }
}
