//! Post-routing path simplification levels for MMDS and SVG output.

use std::str::FromStr;

use crate::errors::RenderError;
use crate::format::normalize_enum_token;

/// Post-routing path simplification level for MMDS and SVG output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PathSimplification {
    /// No simplification. All routed waypoints are retained.
    None,
    /// Lossless: remove redundant collinear and duplicate interior points.
    #[default]
    Lossless,
    /// Lossy: reduce to start, midpoint, and end (3 points max).
    Lossy,
    /// Minimal: start and end only (2 points max).
    Minimal,
}

impl std::fmt::Display for PathSimplification {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PathSimplification::None => write!(f, "none"),
            PathSimplification::Lossless => write!(f, "lossless"),
            PathSimplification::Lossy => write!(f, "lossy"),
            PathSimplification::Minimal => write!(f, "minimal"),
        }
    }
}

impl PathSimplification {
    /// Parse path simplification level from user-provided text.
    pub fn parse(s: &str) -> Result<Self, RenderError> {
        match normalize_enum_token(s).as_str() {
            "none" => Ok(PathSimplification::None),
            "lossless" => Ok(PathSimplification::Lossless),
            "lossy" => Ok(PathSimplification::Lossy),
            "minimal" => Ok(PathSimplification::Minimal),
            _ => Err(RenderError {
                message: format!("unknown path simplification: {s:?}"),
            }),
        }
    }

    /// Simplify a path according to the simplification level.
    pub fn simplify<T: Clone>(&self, points: &[T]) -> Vec<T> {
        match self {
            PathSimplification::None => points.to_vec(),
            PathSimplification::Lossless => points.to_vec(),
            PathSimplification::Lossy if points.len() > 3 => {
                let mid = points.len() / 2;
                vec![
                    points[0].clone(),
                    points[mid].clone(),
                    points[points.len() - 1].clone(),
                ]
            }
            PathSimplification::Minimal if points.len() > 2 => {
                vec![points[0].clone(), points[points.len() - 1].clone()]
            }
            _ => points.to_vec(),
        }
    }

    /// Simplify path points with coordinate-aware compacting.
    pub fn simplify_with_coords<T: Clone>(
        &self,
        points: &[T],
        coords: impl Fn(&T) -> (f64, f64),
    ) -> Vec<T> {
        match self {
            PathSimplification::Lossless => compact_points(points, coords),
            _ => self.simplify(points),
        }
    }
}

impl FromStr for PathSimplification {
    type Err = RenderError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        PathSimplification::parse(s)
    }
}

fn compact_points<T: Clone>(points: &[T], coords: impl Fn(&T) -> (f64, f64)) -> Vec<T> {
    const EPS: f64 = 1e-6;

    if points.len() <= 2 {
        return points.to_vec();
    }

    let mut deduped = Vec::with_capacity(points.len());
    for point in points {
        let keep = deduped.last().is_none_or(|prev: &T| {
            let (px, py) = coords(prev);
            let (x, y) = coords(point);
            (px - x).abs() > EPS || (py - y).abs() > EPS
        });
        if keep {
            deduped.push(point.clone());
        }
    }

    if deduped.len() <= 2 {
        return deduped;
    }

    let mut result = Vec::with_capacity(deduped.len());
    result.push(deduped[0].clone());
    for idx in 1..(deduped.len() - 1) {
        let prev = result.last().expect("result has first element");
        let curr = &deduped[idx];
        let next = &deduped[idx + 1];

        let (px, py) = coords(prev);
        let (cx, cy) = coords(curr);
        let (nx, ny) = coords(next);

        let dx1 = cx - px;
        let dy1 = cy - py;
        let dx2 = nx - cx;
        let dy2 = ny - cy;
        let cross = dx1 * dy2 - dy1 * dx2;
        let dot = dx1 * dx2 + dy1 * dy2;
        let collinear_same_direction = cross.abs() <= EPS && dot >= -EPS;

        if !collinear_same_direction {
            result.push(curr.clone());
        }
    }
    result.push(deduped[deduped.len() - 1].clone());
    result
}
