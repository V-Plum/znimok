//! Document model: annotation objects, styles and the document that holds them.
//!
//! Rules come from Little Helpers (see `docs/discovery/inventory_screenshots.md` §2 and §7):
//! an annotation is an object, not pixels; the crop is a property; coordinates, thicknesses
//! and font sizes are in pixels of the screenshot, never of the screen.

use std::collections::BTreeMap;

/// Integer rectangle in screenshot pixels. For [`Kind::Line`] the sign of `w`/`h` carries the
/// direction from (x, y) to (x + w, y + h) and is never normalised.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl IRect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    /// Same rectangle with non-negative width and height.
    pub fn normalized(self) -> Self {
        let (x, w) = if self.w < 0 {
            (self.x + self.w, -self.w)
        } else {
            (self.x, self.w)
        };
        let (y, h) = if self.h < 0 {
            (self.y + self.h, -self.h)
        } else {
            (self.y, self.h)
        };
        Self { x, y, w, h }
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn center(&self) -> (f64, f64) {
        (
            self.x as f64 + self.w as f64 / 2.0,
            self.y as f64 + self.h as f64 / 2.0,
        )
    }

    /// Grown by `d` on every side (negative shrinks).
    pub fn inflated(self, d: i32) -> Self {
        let r = self.normalized();
        Self {
            x: r.x - d,
            y: r.y - d,
            w: r.w + 2 * d,
            h: r.h + 2 * d,
        }
    }

    pub fn contains(&self, px: f64, py: f64) -> bool {
        let r = self.normalized();
        px >= r.x as f64 && py >= r.y as f64 && px <= r.right() as f64 && py <= r.bottom() as f64
    }

    pub fn translated(self, dx: i32, dy: i32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            ..self
        }
    }
}

/// sRGB colour with alpha 0–255 (alpha is the per-object opacity, kept separately in [`Style`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub const RED: Self = Self::new(0xFF, 0x5A, 0x5F);
    pub const BLUE: Self = Self::new(0x3D, 0x7B, 0xF5);
    pub const YELLOW: Self = Self::new(0xFF, 0xD2, 0x3F);
    pub const GREEN: Self = Self::new(0x34, 0xC4, 0x8A);
    pub const WHITE: Self = Self::new(0xFF, 0xFF, 0xFF);
    pub const BLACK: Self = Self::new(0x00, 0x00, 0x00);
}

/// Kinds of annotation. The order is not persisted anywhere (files use text tags).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Rect,
    Ellipse,
    Line,
    Pen,
    Text,
    Hide,
    Mark,
    Counter,
    Stamp,
    Image,
}

impl Kind {
    /// Line and pen are drawn along a path, not inside a box.
    pub fn is_segment(self) -> bool {
        matches!(self, Kind::Line | Kind::Pen)
    }

    /// Hide and Mark operate on the pixels below them and never rotate.
    pub fn can_rotate(self) -> bool {
        !matches!(self, Kind::Hide | Kind::Mark)
    }

    pub fn has_dash(self) -> bool {
        matches!(self, Kind::Rect | Kind::Ellipse | Kind::Line | Kind::Pen)
    }

    pub fn can_fill(self) -> bool {
        matches!(self, Kind::Rect | Kind::Ellipse)
    }

    pub fn is_effect(self) -> bool {
        matches!(self, Kind::Hide | Kind::Mark)
    }

    pub fn has_thick(self) -> bool {
        matches!(
            self,
            Kind::Rect
                | Kind::Ellipse
                | Kind::Line
                | Kind::Pen
                | Kind::Mark
                | Kind::Counter
                | Kind::Stamp
        )
    }

    /// Placed with a single click rather than dragged out.
    pub fn is_stamped(self) -> bool {
        matches!(self, Kind::Counter | Kind::Stamp)
    }

    pub fn fx_allowed(self) -> bool {
        !matches!(self, Kind::Hide | Kind::Mark | Kind::Pen)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Dash {
    #[default]
    Solid,
    Dashed,
    DashDot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Corners {
    #[default]
    Sharp,
    Soft,
    Round,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Effect {
    #[default]
    None,
    Light,
    Strong,
}

/// Visual style shared by every kind. Fields that a kind does not use are ignored by the renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    pub color: Rgb,
    /// Thickness in screenshot pixels (2/4/7); marker: band height; counter/stamp: diameter.
    pub thick: i32,
    /// Opacity 10–100 %.
    pub alpha: u8,
    /// Outline disabled (rect/ellipse become a solid plate).
    pub no_main: bool,
    /// Second colour: fill (rect/ellipse), outline (text), digit (counter).
    pub color2: Option<Rgb>,
    /// Opacity of the second colour 10–100 %.
    pub alpha2: u8,
    pub dash: Dash,
    pub corners: Corners,
    /// Corner radius in screenshot pixels, computed when the corner level was chosen.
    pub corner_px: i32,
    pub shadow: Effect,
    pub glow: Effect,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            color: Rgb::RED,
            thick: 4,
            alpha: 100,
            no_main: false,
            color2: None,
            alpha2: 100,
            dash: Dash::Solid,
            corners: Corners::Sharp,
            corner_px: 0,
            shadow: Effect::None,
            glow: Effect::None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HideMode {
    #[default]
    Blur,
    Pixelate,
    Plate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Head {
    #[default]
    None,
    Triangle,
    Chevron,
    Dot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CounterShape {
    #[default]
    Circle,
    RoundedBox,
    Pin,
}

/// Group of counters that share one numbering sequence.
pub type CounterGroup = u32;
/// Selection group id (0 = not grouped).
pub type GroupId = u32;
/// Index into the document's image bank.
pub type BankId = u32;

/// Kind-specific data.
#[derive(Clone, Debug, PartialEq)]
pub enum Data {
    Rect,
    Ellipse,
    Line {
        head_front: Head,
        head_back: Head,
        head_size: i32,
    },
    /// Trail in screenshot coordinates, absolute.
    Pen {
        points: Vec<(i32, i32)>,
    },
    Text {
        text: String,
        size: i32,
        bold: bool,
        italic: bool,
        align: Align,
        box_w: i32,
    },
    Hide {
        mode: HideMode,
        strength: u8,
    },
    Mark,
    Counter {
        seq: u32,
        group: CounterGroup,
        start: i32,
        shape: CounterShape,
    },
    /// 0..=5 are vector stamps, 100+ are emoji indexes.
    Stamp {
        id: u32,
    },
    Image {
        bank: BankId,
    },
}

impl Data {
    pub fn kind(&self) -> Kind {
        match self {
            Data::Rect => Kind::Rect,
            Data::Ellipse => Kind::Ellipse,
            Data::Line { .. } => Kind::Line,
            Data::Pen { .. } => Kind::Pen,
            Data::Text { .. } => Kind::Text,
            Data::Hide { .. } => Kind::Hide,
            Data::Mark => Kind::Mark,
            Data::Counter { .. } => Kind::Counter,
            Data::Stamp { .. } => Kind::Stamp,
            Data::Image { .. } => Kind::Image,
        }
    }
}

/// One annotation. `rect` is the box in screenshot pixels; see [`IRect`] for the line convention.
#[derive(Clone, Debug, PartialEq)]
pub struct Object {
    pub rect: IRect,
    pub style: Style,
    /// Rotation 0..360 degrees around the centre; ignored for kinds that cannot rotate.
    pub rot: u16,
    pub group: GroupId,
    pub name: Option<String>,
    pub hidden: bool,
    pub data: Data,
}

impl Object {
    pub fn new(rect: IRect, data: Data) -> Self {
        Self {
            rect,
            style: Style::default(),
            rot: 0,
            group: 0,
            name: None,
            hidden: false,
            data,
        }
    }

    pub fn with_style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    pub fn kind(&self) -> Kind {
        self.data.kind()
    }

    /// Bounding box with non-negative size (pen: from its points).
    pub fn bounds(&self) -> IRect {
        match &self.data {
            Data::Pen { points } if !points.is_empty() => {
                let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
                for &(x, y) in points {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
                IRect::new(x0, y0, x1 - x0, y1 - y0)
            }
            _ => self.rect.normalized(),
        }
    }
}

/// Source pixels of a document: RGBA8, straight alpha, row-major.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Raster {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Self {
        debug_assert_eq!(rgba.len(), (width * height * 4) as usize);
        Self {
            width,
            height,
            rgba,
        }
    }

    pub fn solid(width: u32, height: u32, rgb: Rgb) -> Self {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..width * height {
            rgba.extend_from_slice(&[rgb.r, rgb.g, rgb.b, 255]);
        }
        Self {
            width,
            height,
            rgba,
        }
    }
}

/// Tone and geometry recipe applied over the untouched original.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Recipe {
    /// Exposure in EV, -2.0..=2.0.
    pub exposure: f32,
    /// 0.5..=2.0.
    pub gamma: f32,
    /// -50..=50.
    pub contrast: i32,
    /// Quarter turns clockwise, 0..4.
    pub rot_quarters: u8,
    pub mirror: bool,
}

impl Default for Recipe {
    fn default() -> Self {
        Self {
            exposure: 0.0,
            gamma: 1.0,
            contrast: 0,
            rot_quarters: 0,
            mirror: false,
        }
    }
}

/// A screenshot with its annotations.
#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub name: String,
    /// Image bank: index 0 is the current source; later entries are pasted images or old sources
    /// kept for undo. Never shrinks during a session.
    pub banks: Vec<Raster>,
    pub source: BankId,
    pub recipe: Recipe,
    /// Crop is a property, not a pixel operation; objects outside keep living.
    pub crop: Option<IRect>,
    /// DPI scale of the monitor the shot was taken on, ×1000.
    pub shot_scale: u16,
    /// Objects in z-order: index 0 is at the bottom.
    pub objects: Vec<Object>,
    pub group_names: BTreeMap<GroupId, String>,
}

impl Document {
    pub fn from_raster(name: impl Into<String>, raster: Raster) -> Self {
        Self {
            name: name.into(),
            banks: vec![raster],
            source: 0,
            recipe: Recipe::default(),
            crop: None,
            shot_scale: 1000,
            objects: Vec::new(),
            group_names: BTreeMap::new(),
        }
    }

    pub fn source(&self) -> &Raster {
        &self.banks[self.source as usize]
    }

    /// Width and height of the source, honouring the recipe's quarter turns.
    pub fn image_size(&self) -> (u32, u32) {
        let s = self.source();
        if self.recipe.rot_quarters % 2 == 1 {
            (s.height, s.width)
        } else {
            (s.width, s.height)
        }
    }

    /// The visible frame: the crop, or the whole image.
    pub fn frame(&self) -> IRect {
        let (w, h) = self.image_size();
        self.crop
            .map(IRect::normalized)
            .unwrap_or(IRect::new(0, 0, w as i32, h as i32))
    }

    /// Number shown on a counter: `start + rank` among counters of the same group, ordered by
    /// creation sequence. Deleting one renumbers the rest, as in Little Helpers.
    pub fn counter_number(&self, index: usize) -> Option<i32> {
        let Data::Counter {
            seq, group, start, ..
        } = self.objects.get(index)?.data
        else {
            return None;
        };
        let rank = self
            .objects
            .iter()
            .filter(|o| matches!(o.data, Data::Counter { group: g, seq: s, .. } if g == group && s < seq))
            .count();
        Some(start + rank as i32)
    }

    pub fn push(&mut self, object: Object) -> usize {
        self.objects.push(object);
        self.objects.len() - 1
    }

    /// Sequence number for a new counter: one past the largest so far.
    pub fn next_counter_seq(&self) -> u32 {
        self.objects
            .iter()
            .filter_map(|o| match o.data {
                Data::Counter { seq, .. } => Some(seq + 1),
                _ => None,
            })
            .max()
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_rect_keeps_direction_but_bounds_normalise() {
        let o = Object::new(
            IRect::new(100, 100, -40, -30),
            Data::Line {
                head_front: Head::Triangle,
                head_back: Head::None,
                head_size: 12,
            },
        );
        assert_eq!(o.rect.w, -40);
        assert_eq!(o.bounds(), IRect::new(60, 70, 40, 30));
    }

    #[test]
    fn counters_number_by_rank_within_group() {
        let mut doc = Document::from_raster("t", Raster::solid(10, 10, Rgb::WHITE));
        for seq in 0..3 {
            doc.push(Object::new(
                IRect::new(0, 0, 24, 24),
                Data::Counter {
                    seq,
                    group: 1,
                    start: 5,
                    shape: CounterShape::Circle,
                },
            ));
        }
        doc.push(Object::new(
            IRect::new(0, 0, 24, 24),
            Data::Counter {
                seq: 9,
                group: 2,
                start: 1,
                shape: CounterShape::Circle,
            },
        ));
        assert_eq!(doc.counter_number(0), Some(5));
        assert_eq!(doc.counter_number(2), Some(7));
        assert_eq!(doc.counter_number(3), Some(1));
        doc.objects.remove(1);
        assert_eq!(doc.counter_number(1), Some(6));
        assert_eq!(doc.next_counter_seq(), 10);
    }

    #[test]
    fn frame_is_crop_or_whole_image() {
        let mut doc = Document::from_raster("t", Raster::solid(40, 20, Rgb::WHITE));
        assert_eq!(doc.frame(), IRect::new(0, 0, 40, 20));
        doc.crop = Some(IRect::new(30, 15, -10, -5));
        assert_eq!(doc.frame(), IRect::new(20, 10, 10, 5));
        doc.recipe.rot_quarters = 1;
        doc.crop = None;
        assert_eq!(doc.frame(), IRect::new(0, 0, 20, 40));
    }
}
