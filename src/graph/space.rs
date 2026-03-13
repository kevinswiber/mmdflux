//! Shared float-space primitives for graph-family geometry.
//!
//! These types are used across core geometry contracts, grid replay metadata,
//! and routing helpers. They live outside `geometry` so consumers can depend
//! on float-space points and rectangles without pulling in higher-level graph
//! solve contracts.

/// Float-precision rectangle (layout coordinate space).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl FRect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn center_x(&self) -> f64 {
        self.x + self.width / 2.0
    }

    pub fn center_y(&self) -> f64 {
        self.y + self.height / 2.0
    }

    pub fn center(&self) -> FPoint {
        FPoint {
            x: self.center_x(),
            y: self.center_y(),
        }
    }
}

/// Float-precision point (layout coordinate space).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FPoint {
    pub x: f64,
    pub y: f64,
}

impl FPoint {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frect_center() {
        let rect = FRect::new(10.0, 20.0, 100.0, 50.0);
        assert_eq!(rect.center_x(), 60.0);
        assert_eq!(rect.center_y(), 45.0);
        assert_eq!(rect.center(), FPoint::new(60.0, 45.0));
    }
}
