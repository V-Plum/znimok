//! Desktop geometry.
//!
//! **Desktop units** are what the OS uses for its global screen space: physical pixels on Windows
//! (the process is Per-Monitor-v2 aware, so the virtual desktop is continuous in pixels even with
//! mixed DPI), points on macOS (global display space; each display has its own backing scale).
//! Every display and window carries its `scale`, so `pixels = desktop units × scale` on macOS and
//! `scale` is informational (DPI / 96) on Windows. Captured frames are always in physical pixels.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// A rectangle in desktop units: `x, y` is the top-left corner, `width × height` the size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> i32 {
        self.x + self.width as i32
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.height as i32
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.y >= self.y && p.x < self.right() && p.y < self.bottom()
    }

    /// Overlap of two rectangles, `None` when they do not touch.
    pub fn intersect(&self, o: &Rect) -> Option<Rect> {
        let (x0, y0) = (self.x.max(o.x), self.y.max(o.y));
        let (x1, y1) = (self.right().min(o.right()), self.bottom().min(o.bottom()));
        (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32))
    }

    /// This rectangle relative to `origin` (e.g. a region inside a display).
    pub fn relative_to(&self, origin: Point) -> Rect {
        Rect::new(
            self.x - origin.x,
            self.y - origin.y,
            self.width,
            self.height,
        )
    }

    pub fn origin(&self) -> Point {
        Point {
            x: self.x,
            y: self.y,
        }
    }

    /// Scale to pixels (macOS points → pixels), rounding outwards so no pixel is lost.
    pub fn scaled(&self, scale: f32) -> Rect {
        let s = f64::from(scale);
        let x0 = (f64::from(self.x) * s).floor();
        let y0 = (f64::from(self.y) * s).floor();
        let x1 = (f64::from(self.right()) * s).ceil();
        let y1 = (f64::from(self.bottom()) * s).ceil();
        Rect::new(x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersect_and_contains() {
        let a = Rect::new(0, 0, 100, 50);
        let b = Rect::new(80, 40, 50, 50);
        assert_eq!(a.intersect(&b), Some(Rect::new(80, 40, 20, 10)));
        assert_eq!(a.intersect(&Rect::new(100, 0, 5, 5)), None);
        assert!(a.contains(Point { x: 99, y: 49 }));
        assert!(!a.contains(Point { x: 100, y: 0 }));
    }

    #[test]
    fn negative_desktop_coordinates() {
        // A monitor left of the primary one.
        let left = Rect::new(-1920, 0, 1920, 1080);
        assert!(left.contains(Point { x: -1, y: 10 }));
        assert_eq!(left.relative_to(left.origin()), Rect::new(0, 0, 1920, 1080));
    }

    #[test]
    fn scaled_rounds_outwards() {
        assert_eq!(Rect::new(1, 1, 3, 3).scaled(1.5), Rect::new(1, 1, 5, 5));
        assert_eq!(
            Rect::new(10, 20, 100, 50).scaled(2.0),
            Rect::new(20, 40, 200, 100)
        );
    }
}
