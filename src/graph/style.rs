//! Node styling types and Mermaid `style` statement parsing.

use std::error::Error;
use std::fmt;

use serde::{Serialize, Serializer};

/// Parsed CSS-like style properties for a diagram node (fill, stroke, color).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct NodeStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<ColorToken>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke: Option<ColorToken>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorToken>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_style: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_weight: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke_dasharray: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rx: Option<String>,
}

impl NodeStyle {
    pub fn is_empty(&self) -> bool {
        self.fill.is_none()
            && self.stroke.is_none()
            && self.color.is_none()
            && self.font_style.is_none()
            && self.font_weight.is_none()
            && self.stroke_width.is_none()
            && self.stroke_dasharray.is_none()
            && self.rx.is_none()
    }

    pub fn with_fill(mut self, fill: ColorToken) -> Self {
        self.fill = Some(fill);
        self
    }

    pub fn with_stroke(mut self, stroke: ColorToken) -> Self {
        self.stroke = Some(stroke);
        self
    }

    pub fn with_color(mut self, color: ColorToken) -> Self {
        self.color = Some(color);
        self
    }

    pub fn merge(&self, overlay: &Self) -> Self {
        Self {
            fill: overlay.fill.clone().or_else(|| self.fill.clone()),
            stroke: overlay.stroke.clone().or_else(|| self.stroke.clone()),
            color: overlay.color.clone().or_else(|| self.color.clone()),
            font_style: overlay
                .font_style
                .clone()
                .or_else(|| self.font_style.clone()),
            font_weight: overlay
                .font_weight
                .clone()
                .or_else(|| self.font_weight.clone()),
            stroke_width: overlay
                .stroke_width
                .clone()
                .or_else(|| self.stroke_width.clone()),
            stroke_dasharray: overlay
                .stroke_dasharray
                .clone()
                .or_else(|| self.stroke_dasharray.clone()),
            rx: overlay.rx.clone().or_else(|| self.rx.clone()),
        }
    }
}

/// A CSS color value (hex, named, rgb, etc.) parsed from a Mermaid style statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorToken {
    raw: String,
    rgb: Option<(u8, u8, u8)>,
}

impl ColorToken {
    pub fn parse(raw: &str) -> Result<Self, ColorTokenParseError> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(ColorTokenParseError::Empty);
        }

        Ok(Self {
            raw: raw.to_string(),
            rgb: parse_hex_color(raw).or_else(|| named_color_rgb(raw)),
        })
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn to_rgb(&self) -> Option<(u8, u8, u8)> {
        self.rgb
    }
}

impl Serialize for ColorToken {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.raw)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorTokenParseError {
    Empty,
}

impl fmt::Display for ColorTokenParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ColorTokenParseError::Empty => write!(f, "color token cannot be empty"),
        }
    }
}

impl Error for ColorTokenParseError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedNodeStyleDeclaration {
    pub style: NodeStyle,
    pub issues: Vec<NodeStyleIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedNodeStyleDirective {
    pub node_id: String,
    pub style: NodeStyle,
    pub issues: Vec<NodeStyleIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeStyleIssue {
    UnsupportedProperty { property: String },
    UnsupportedColorSyntax { property: String, value: String },
    MalformedDeclaration { declaration: String },
}

impl NodeStyleIssue {
    pub fn message(&self) -> String {
        match self {
            NodeStyleIssue::UnsupportedProperty { property } => format!(
                "style property '{}' is not supported; supported properties are fill, stroke, and color",
                property
            ),
            NodeStyleIssue::UnsupportedColorSyntax { property, value } => format!(
                "style property '{}' uses unsupported color syntax '{}'; supported color formats are #rgb, #rrggbb, and named colors",
                property, value
            ),
            NodeStyleIssue::MalformedDeclaration { declaration } => format!(
                "style declaration '{}' must use key:value syntax",
                declaration
            ),
        }
    }
}

pub fn parse_node_style_statement(raw: &str) -> Option<ParsedNodeStyleDirective> {
    let trimmed = raw.trim();
    let rest = strip_keyword(trimmed, "style")?.trim_start();
    if rest.is_empty() {
        return None;
    }

    let mut parts = rest.splitn(2, char::is_whitespace);
    let node_id = parts.next()?.trim();
    if node_id.is_empty() {
        return None;
    }

    let declarations = parts.next().unwrap_or("").trim();
    let parsed = parse_node_style_declarations(declarations);
    Some(ParsedNodeStyleDirective {
        node_id: node_id.to_string(),
        style: parsed.style,
        issues: parsed.issues,
    })
}

/// Reassemble comma-split fragments: if a fragment has no `:` it's part of the
/// previous value (e.g. `stroke-dasharray:5,3` → one declaration, not two).
fn reassemble_declarations(raw: &str) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    for part in raw.split(',') {
        if part.contains(':') || result.is_empty() {
            result.push(part.to_string());
        } else {
            // Append to previous declaration's value.
            if let Some(last) = result.last_mut() {
                last.push(',');
                last.push_str(part);
            }
        }
    }
    result
}

pub(crate) fn parse_node_style_declarations(raw: &str) -> ParsedNodeStyleDeclaration {
    let mut style = NodeStyle::default();
    let mut issues = Vec::new();

    // Reassemble comma-separated fragments: fragments without `:` are part of
    // the previous declaration's value (e.g. `stroke-dasharray:5,3`).
    let declarations = reassemble_declarations(raw);

    for declaration in &declarations {
        let declaration = declaration.trim();
        if declaration.is_empty() {
            continue;
        }

        let Some((key, value)) = declaration.split_once(':') else {
            issues.push(NodeStyleIssue::MalformedDeclaration {
                declaration: declaration.to_string(),
            });
            continue;
        };

        let property = key.trim().to_ascii_lowercase();
        let value = value.trim();
        if value.is_empty() {
            issues.push(NodeStyleIssue::MalformedDeclaration {
                declaration: declaration.to_string(),
            });
            continue;
        }

        // String-valued properties (non-color).
        match property.as_str() {
            "font-style" => {
                style.font_style = Some(value.to_string());
                continue;
            }
            "font-weight" => {
                style.font_weight = Some(value.to_string());
                continue;
            }
            "stroke-width" => {
                style.stroke_width = Some(value.to_string());
                continue;
            }
            "stroke-dasharray" => {
                style.stroke_dasharray = Some(value.to_string());
                continue;
            }
            "rx" => {
                style.rx = Some(value.to_string());
                continue;
            }
            _ => {}
        }

        // Color-valued properties.
        let token = match ColorToken::parse(value) {
            Ok(token) => token,
            Err(_) => {
                issues.push(NodeStyleIssue::MalformedDeclaration {
                    declaration: declaration.to_string(),
                });
                continue;
            }
        };

        match property.as_str() {
            "fill" => {
                if token.to_rgb().is_none() {
                    issues.push(NodeStyleIssue::UnsupportedColorSyntax {
                        property: property.clone(),
                        value: token.raw().to_string(),
                    });
                }
                style.fill = Some(token);
            }
            "stroke" => {
                if token.to_rgb().is_none() {
                    issues.push(NodeStyleIssue::UnsupportedColorSyntax {
                        property: property.clone(),
                        value: token.raw().to_string(),
                    });
                }
                style.stroke = Some(token);
            }
            "color" => {
                if token.to_rgb().is_none() {
                    issues.push(NodeStyleIssue::UnsupportedColorSyntax {
                        property: property.clone(),
                        value: token.raw().to_string(),
                    });
                }
                style.color = Some(token);
            }
            _ => issues.push(NodeStyleIssue::UnsupportedProperty { property }),
        }
    }

    ParsedNodeStyleDeclaration { style, issues }
}

/// Parsed `classDef className fill:#f9f,stroke:#333` directive.
pub struct ParsedClassDefDirective {
    /// The class name.
    pub class_name: String,
    /// Resolved style properties.
    pub style: NodeStyle,
    /// Issues found during parsing (unsupported properties, etc.).
    pub issues: Vec<NodeStyleIssue>,
}

/// Parse a `classDef className fill:#f9f,stroke:#333` statement.
///
/// Supports multi-class syntax: `classDef a,b fill:#f9f` registers both `a` and `b`.
pub fn parse_classdef_statement(raw: &str) -> Option<ParsedClassDefDirective> {
    parse_classdef_statement_multi(raw).into_iter().next()
}

/// Parse a `classDef` statement, returning one directive per class name.
///
/// `classDef a,b fill:#f9f` returns two directives (for `a` and `b`) sharing
/// the same style and issues.
pub fn parse_classdef_statement_multi(raw: &str) -> Vec<ParsedClassDefDirective> {
    let trimmed = raw.trim();
    let Some(rest) = strip_keyword(trimmed, "classDef") else {
        return Vec::new();
    };
    let rest = rest.trim_start();
    if rest.is_empty() {
        return Vec::new();
    }

    let mut parts = rest.splitn(2, char::is_whitespace);
    let class_names_raw = match parts.next() {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => return Vec::new(),
    };

    let declarations = parts.next().unwrap_or("").trim();
    let parsed = parse_node_style_declarations(declarations);

    class_names_raw
        .split(',')
        .filter(|name| !name.is_empty())
        .map(|name| ParsedClassDefDirective {
            class_name: name.trim().to_string(),
            style: parsed.style.clone(),
            issues: parsed.issues.clone(),
        })
        .collect()
}

/// Parsed `class nodeA,nodeB className` directive.
pub struct ParsedClassApplyDirective {
    /// Node IDs to apply the class to.
    pub node_ids: Vec<String>,
    /// The class name to apply.
    pub class_name: String,
}

/// Parse a `class nodeA,nodeB className` statement.
pub fn parse_class_apply_statement(raw: &str) -> Option<ParsedClassApplyDirective> {
    let trimmed = raw.trim();

    // Reject "classDef" lines (keyword prefix overlap).
    if trimmed.len() >= 8
        && trimmed[..8].eq_ignore_ascii_case("classDef")
        && trimmed[8..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_')
    {
        return None;
    }

    let rest = strip_keyword(trimmed, "class")?.trim_start();
    if rest.is_empty() {
        return None;
    }

    // Last whitespace-delimited token is the class name.
    let last_space = rest.rfind(char::is_whitespace)?;
    let node_list = rest[..last_space].trim();
    let class_name = rest[last_space..].trim();
    if class_name.is_empty() || node_list.is_empty() {
        return None;
    }

    let node_ids: Vec<String> = node_list
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if node_ids.is_empty() {
        return None;
    }

    Some(ParsedClassApplyDirective {
        node_ids,
        class_name: class_name.to_string(),
    })
}

fn strip_keyword<'a>(input: &'a str, keyword: &str) -> Option<&'a str> {
    if input.len() < keyword.len() || !input.is_char_boundary(keyword.len()) {
        return None;
    }

    let (prefix, rest) = input.split_at(keyword.len());
    if !prefix.eq_ignore_ascii_case(keyword) {
        return None;
    }

    if let Some(next) = rest.chars().next()
        && (next.is_alphanumeric() || next == '_')
    {
        return None;
    }

    Some(rest)
}

fn parse_hex_color(raw: &str) -> Option<(u8, u8, u8)> {
    let hex = raw.strip_prefix('#')?;
    match hex.len() {
        3 => {
            let mut digits = hex.chars();
            let r = digits.next()?.to_digit(16)? as u8;
            let g = digits.next()?.to_digit(16)? as u8;
            let b = digits.next()?.to_digit(16)? as u8;
            Some((r * 17, g * 17, b * 17))
        }
        6 => Some((
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
        )),
        _ => None,
    }
}

fn named_color_rgb(raw: &str) -> Option<(u8, u8, u8)> {
    match raw.to_ascii_lowercase().as_str() {
        "black" => Some((0, 0, 0)),
        "white" => Some((255, 255, 255)),
        "red" => Some((255, 0, 0)),
        "green" => Some((0, 128, 0)),
        "blue" => Some((0, 0, 255)),
        "yellow" => Some((255, 255, 0)),
        "cyan" => Some((0, 255, 255)),
        "magenta" => Some((255, 0, 255)),
        "gray" | "grey" => Some((128, 128, 128)),
        "orange" => Some((255, 165, 0)),
        "purple" => Some((128, 0, 128)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ColorToken, NodeStyle, NodeStyleIssue, parse_class_apply_statement,
        parse_classdef_statement, parse_classdef_statement_multi, parse_node_style_declarations,
        parse_node_style_statement,
    };

    #[test]
    fn node_style_merge_is_property_level_last_write_wins() {
        let base = NodeStyle::default()
            .with_fill(ColorToken::parse("#ffeeaa").unwrap())
            .with_stroke(ColorToken::parse("#333").unwrap());
        let overlay = NodeStyle::default()
            .with_color(ColorToken::parse("#111").unwrap())
            .with_stroke(ColorToken::parse("#555").unwrap());

        let merged = base.merge(&overlay);

        assert_eq!(merged.fill.unwrap().raw(), "#ffeeaa");
        assert_eq!(merged.stroke.unwrap().raw(), "#555");
        assert_eq!(merged.color.unwrap().raw(), "#111");
    }

    #[test]
    fn color_token_parses_hex_and_named_colors_for_ansi_resolution() {
        let short_hex = ColorToken::parse("#abc").unwrap();
        let long_hex = ColorToken::parse("#aabbcc").unwrap();
        let named = ColorToken::parse("red").unwrap();

        assert_eq!(short_hex.to_rgb().unwrap(), (170, 187, 204));
        assert_eq!(long_hex.to_rgb().unwrap(), (170, 187, 204));
        assert_eq!(named.to_rgb().unwrap(), (255, 0, 0));
    }

    #[test]
    fn parse_node_style_statement_collects_supported_properties_and_issues() {
        let parsed =
            parse_node_style_statement("style A fill:#fff,stroke-width:4px,color:var(--accent)")
                .unwrap();

        assert_eq!(parsed.node_id, "A");
        assert_eq!(parsed.style.fill.as_ref().unwrap().raw(), "#fff");
        assert!(parsed.style.stroke.is_none());
        assert_eq!(parsed.style.color.as_ref().unwrap().raw(), "var(--accent)");
        assert_eq!(parsed.style.stroke_width.as_deref(), Some("4px"));
        assert!(
            !parsed
                .issues
                .contains(&NodeStyleIssue::UnsupportedProperty {
                    property: "stroke-width".to_string(),
                })
        );
        assert!(
            parsed
                .issues
                .contains(&NodeStyleIssue::UnsupportedColorSyntax {
                    property: "color".to_string(),
                    value: "var(--accent)".to_string(),
                })
        );
    }

    #[test]
    fn parse_node_style_declarations_is_last_write_wins_per_property() {
        let parsed = parse_node_style_declarations("fill:#aaa,stroke:#111,fill:#bbb");

        assert_eq!(parsed.style.fill.as_ref().unwrap().raw(), "#bbb");
        assert_eq!(parsed.style.stroke.as_ref().unwrap().raw(), "#111");
        assert!(parsed.issues.is_empty());
    }

    // --- classDef parsing ---

    #[test]
    fn parse_classdef_basic() {
        let result = parse_classdef_statement("classDef highlight fill:#ff0,stroke:#333");
        assert!(result.is_some());
        let parsed = result.unwrap();
        assert_eq!(parsed.class_name, "highlight");
        assert_eq!(parsed.style.fill.as_ref().unwrap().raw(), "#ff0");
        assert_eq!(parsed.style.stroke.as_ref().unwrap().raw(), "#333");
        assert!(parsed.issues.is_empty());
    }

    #[test]
    fn parse_classdef_single_property() {
        let result = parse_classdef_statement("classDef err fill:#f00");
        let parsed = result.unwrap();
        assert_eq!(parsed.class_name, "err");
        assert!(parsed.style.fill.is_some());
        assert!(parsed.style.stroke.is_none());
    }

    #[test]
    fn parse_classdef_default_class_name() {
        let result = parse_classdef_statement("classDef default fill:#fff");
        let parsed = result.unwrap();
        assert_eq!(parsed.class_name, "default");
    }

    #[test]
    fn parse_classdef_missing_body() {
        let result = parse_classdef_statement("classDef highlight");
        assert!(result.is_some());
        let parsed = result.unwrap();
        assert_eq!(parsed.class_name, "highlight");
        assert!(parsed.style.is_empty());
    }

    #[test]
    fn parse_classdef_no_class_name() {
        let result = parse_classdef_statement("classDef ");
        assert!(result.is_none());
    }

    #[test]
    fn parse_classdef_not_classdef() {
        assert!(parse_classdef_statement("style A fill:#f00").is_none());
        assert!(parse_classdef_statement("class A highlight").is_none());
    }

    #[test]
    fn parse_classdef_unsupported_property_reported() {
        let result = parse_classdef_statement("classDef foo fill:#f00,font-size:14px");
        let parsed = result.unwrap();
        assert!(parsed.style.fill.is_some());
        assert!(!parsed.issues.is_empty());
    }

    #[test]
    fn parse_classdef_case_insensitive() {
        assert!(parse_classdef_statement("CLASSDEF foo fill:#f00").is_some());
        assert!(parse_classdef_statement("ClassDef foo fill:#f00").is_some());
    }

    #[test]
    fn parse_classdef_multi_class_names() {
        let results = parse_classdef_statement_multi("classDef a,b fill:#f00");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].class_name, "a");
        assert_eq!(results[1].class_name, "b");
        assert_eq!(results[0].style.fill.as_ref().unwrap().raw(), "#f00");
        assert_eq!(results[1].style.fill.as_ref().unwrap().raw(), "#f00");
    }

    #[test]
    fn parse_classdef_multi_single_name_returns_one() {
        let results = parse_classdef_statement_multi("classDef highlight fill:#ff0");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].class_name, "highlight");
    }

    // --- class apply parsing ---

    #[test]
    fn parse_class_apply_single_node() {
        let result = parse_class_apply_statement("class A highlight");
        assert!(result.is_some());
        let parsed = result.unwrap();
        assert_eq!(parsed.node_ids, vec!["A"]);
        assert_eq!(parsed.class_name, "highlight");
    }

    #[test]
    fn parse_class_apply_multiple_nodes() {
        let result = parse_class_apply_statement("class A,B,C highlight");
        let parsed = result.unwrap();
        assert_eq!(parsed.node_ids, vec!["A", "B", "C"]);
        assert_eq!(parsed.class_name, "highlight");
    }

    #[test]
    fn parse_class_apply_with_spaces() {
        let result = parse_class_apply_statement("class A, B, C highlight");
        let parsed = result.unwrap();
        assert_eq!(parsed.node_ids, vec!["A", "B", "C"]);
        assert_eq!(parsed.class_name, "highlight");
    }

    #[test]
    fn parse_class_apply_empty_input() {
        assert!(parse_class_apply_statement("class ").is_none());
        assert!(parse_class_apply_statement("class").is_none());
    }

    #[test]
    fn parse_class_apply_only_class_name_no_nodes() {
        assert!(parse_class_apply_statement("class highlight").is_none());
    }

    #[test]
    fn parse_class_apply_not_class() {
        assert!(parse_class_apply_statement("classDef foo fill:#f00").is_none());
        assert!(parse_class_apply_statement("style A fill:#f00").is_none());
    }
}
