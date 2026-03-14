//! Text output color policy and ANSI color mode selection.

use std::str::FromStr;

use crate::errors::RenderError;
use crate::format::normalize_enum_token;

/// Caller-facing text color policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorWhen {
    /// Never emit ANSI color.
    Off,
    /// Emit ANSI color only when the output sink supports it.
    #[default]
    Auto,
    /// Always emit ANSI color.
    Always,
}

impl ColorWhen {
    /// Parse text color policy from user-provided text.
    pub fn parse(s: &str) -> Result<Self, RenderError> {
        match normalize_enum_token(s).as_str() {
            "off" => Ok(ColorWhen::Off),
            "auto" => Ok(ColorWhen::Auto),
            "always" => Ok(ColorWhen::Always),
            _ => Err(RenderError {
                message: format!(
                    "unknown color policy: {s:?} (expected one of: off, auto, always)"
                ),
            }),
        }
    }

    /// Resolve the caller-facing policy into a renderer-facing mode.
    pub fn resolve(self, output_is_terminal: bool) -> TextColorMode {
        match self {
            ColorWhen::Off => TextColorMode::Plain,
            ColorWhen::Auto => {
                if output_is_terminal {
                    TextColorMode::Ansi
                } else {
                    TextColorMode::Plain
                }
            }
            ColorWhen::Always => TextColorMode::Ansi,
        }
    }
}

impl std::fmt::Display for ColorWhen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ColorWhen::Off => write!(f, "off"),
            ColorWhen::Auto => write!(f, "auto"),
            ColorWhen::Always => write!(f, "always"),
        }
    }
}

impl FromStr for ColorWhen {
    type Err = RenderError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ColorWhen::parse(s)
    }
}

/// Resolved renderer-facing text color mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextColorMode {
    /// Plain text output with no ANSI color.
    #[default]
    Plain,
    /// ANSI-capable text output.
    Ansi,
}

impl TextColorMode {
    pub fn uses_ansi(self) -> bool {
        matches!(self, TextColorMode::Ansi)
    }
}
