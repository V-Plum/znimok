//! Document model: annotation objects, styles and the document that holds them.
//!
//! Rules come from Little Helpers (see `docs/discovery/inventory_screenshots.md` §2 and §7):
//! an annotation is an object, not pixels; the crop is a property; coordinates, thicknesses
//! and font sizes are in pixels of the screenshot, never of the screen; tone and geometry are a
//! recipe over the untouched original; pixel banks are addressed by number and never shrink.

use std::collections::BTreeMap;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Integer rectangle in screenshot pixels. For [`Kind::Line`] the sign of `w`/`h` carries the
/// direction from (x, y) to (x + w, y + h) and is never normalised.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
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

    /// Smallest rectangle containing both (normalised).
    pub fn union(self, other: IRect) -> Self {
        let (a, b) = (self.normalized(), other.normalized());
        let x0 = a.x.min(b.x);
        let y0 = a.y.min(b.y);
        Self {
            x: x0,
            y: y0,
            w: a.right().max(b.right()) - x0,
            h: a.bottom().max(b.bottom()) - y0,
        }
    }
}

/// sRGB colour. Opacity is kept separately in [`Style`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
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

/// Kinds of annotation. Files use text tags, never this order (§7 п.48).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
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

    /// Marks that only move with the picture when the whole document is rotated or mirrored:
    /// their content (letters, digits, emoji) must stay upright (§7 п.25).
    pub fn upright(self) -> bool {
        matches!(self, Kind::Text | Kind::Counter | Kind::Stamp)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Dash {
    #[default]
    Solid,
    Dashed,
    DashDot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Corners {
    #[default]
    Sharp,
    Soft,
    Round,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    #[default]
    None,
    Light,
    Strong,
}

/// Visual style shared by every kind. Fields that a kind does not use are ignored by the renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HideMode {
    #[default]
    Blur,
    Pixelate,
    Plate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Head {
    #[default]
    None,
    Triangle,
    Chevron,
    Dot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
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
/// Stable identity of an object inside its document: survives reordering, undo and saving.
/// 0 means "not assigned yet" — [`Document::push`] assigns one.
pub type ObjectId = u32;

/// Kind-specific data.
#[derive(Clone, Debug, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Data {
    Rect,
    Ellipse,
    /// `head_size` is a size step 0..=2 (small, medium, large), not pixels.
    Line {
        head_front: Head,
        head_back: Head,
        head_size: u8,
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
#[derive(Clone, Debug, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct Object {
    #[serde(default)]
    pub id: ObjectId,
    pub rect: IRect,
    #[serde(default)]
    pub style: Style,
    /// Rotation 0..360 degrees around the centre; ignored for kinds that cannot rotate.
    #[serde(default)]
    pub rot: u16,
    /// Selection group (0 = none). Members of a group are adjacent in z-order (§7 п.47).
    #[serde(default)]
    pub group: GroupId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub hidden: bool,
    pub data: Data,
}

impl Object {
    pub fn new(rect: IRect, data: Data) -> Self {
        Self {
            id: 0,
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

    /// Moves the object; pen points move with it.
    pub fn translate(&mut self, dx: i32, dy: i32) {
        self.rect = self.rect.translated(dx, dy);
        if let Data::Pen { points } = &mut self.data {
            for p in points.iter_mut() {
                p.0 += dx;
                p.1 += dy;
            }
        }
    }

    /// Applies an image-space transform `f` (a quarter turn or a mirror of the whole picture)
    /// following LH rules: segments move point by point; upright marks and the axis-aligned
    /// effects move by their centre / corners; other rotatable boxes move by their centre and
    /// turn by angle — never by swapping sides, or a double turn goes wrong (§7 п.25).
    fn transform_with(&mut self, f: &dyn Fn(i32, i32) -> (i32, i32), angle: &dyn Fn(u16) -> u16) {
        let kind = self.kind();
        match &mut self.data {
            Data::Line { .. } => {
                let (x0, y0) = f(self.rect.x, self.rect.y);
                let (x1, y1) = f(self.rect.x + self.rect.w, self.rect.y + self.rect.h);
                self.rect = IRect::new(x0, y0, x1 - x0, y1 - y0);
            }
            Data::Pen { points } => {
                for p in points.iter_mut() {
                    *p = f(p.0, p.1);
                }
                self.rect = self.bounds();
            }
            _ if kind.is_effect() => {
                let r = self.rect.normalized();
                let (x0, y0) = f(r.x, r.y);
                let (x1, y1) = f(r.right(), r.bottom());
                self.rect = IRect::new(x0, y0, x1 - x0, y1 - y0).normalized();
            }
            _ => {
                // Centre in doubled coordinates keeps it exact for any parity of w and h.
                let r = self.rect.normalized();
                let (cx2, cy2) = (2 * r.x + r.w, 2 * r.y + r.h);
                let (ncx2, ncy2) = f2(f, cx2, cy2);
                let (w, h) = (r.w, r.h);
                self.rect = IRect::new((ncx2 - w).div_euclid(2), (ncy2 - h).div_euclid(2), w, h);
                if !kind.upright() {
                    self.rot = angle(self.rot);
                }
            }
        }
    }
}

/// Applies a point transform to doubled coordinates (used for exact centres).
fn f2(f: &dyn Fn(i32, i32) -> (i32, i32), x2: i32, y2: i32) -> (i32, i32) {
    // f is affine with integer image-size offsets: f(x) = A·x + t. Doubling: f2(2x) = A·2x + 2t
    // = 2·f(x) when x is integral; for half-integers use f(0) to recover t.
    let (tx, ty) = f(0, 0);
    let (ax, ay) = f(1, 0);
    let (bx, by) = f(0, 1);
    let (a11, a21) = (ax - tx, ay - ty);
    let (a12, a22) = (bx - tx, by - ty);
    (a11 * x2 + a12 * y2 + 2 * tx, a21 * x2 + a22 * y2 + 2 * ty)
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

/// Tone and geometry recipe applied over the untouched original. The displayed image is
/// `rotate(rot_quarters) · mirror? · source` (mirror first, as in LH).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
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

impl Recipe {
    /// Same recipe with tone values clamped to their ranges.
    pub fn clamped(self) -> Self {
        Self {
            exposure: self.exposure.clamp(-2.0, 2.0),
            gamma: self.gamma.clamp(0.5, 2.0),
            contrast: self.contrast.clamp(-50, 50),
            rot_quarters: self.rot_quarters % 4,
            mirror: self.mirror,
        }
    }

    pub fn has_tone(&self) -> bool {
        self.exposure != 0.0 || self.gamma != 1.0 || self.contrast != 0
    }
}

/// Descriptive metadata of a record.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Meta {
    /// Creation time, Unix milliseconds UTC.
    pub created_ms: i64,
    /// Where the picture came from: "screen", "window", "region", "clipboard", "file"…
    pub source: String,
    pub description: String,
    pub author: String,
    pub copyright: String,
    pub tags: Vec<String>,
}

/// A screenshot with its annotations.
#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    /// Stable identity: survives renaming and synchronisation.
    pub id: Uuid,
    pub name: String,
    pub meta: Meta,
    /// Pixel bank: originals and pasted images, addressed by number. Shared, never copied by
    /// undo, never shrinks during a session (§7 п.22).
    pub banks: Vec<Arc<Raster>>,
    /// Current original in `banks`; resizing bakes a new one, the old stays for undo.
    pub source: BankId,
    pub recipe: Recipe,
    /// Crop is a property, not a pixel operation; objects outside keep living (§7 п.20).
    pub crop: Option<IRect>,
    /// DPI scale of the monitor the shot was taken on, ×1000 (corner radii).
    pub shot_scale: u16,
    /// Objects in z-order: index 0 is at the bottom.
    pub objects: Vec<Object>,
    pub group_names: BTreeMap<GroupId, String>,
    /// Next object id to hand out; only grows.
    pub next_id: ObjectId,
}

impl Document {
    pub fn from_raster(name: impl Into<String>, raster: Raster) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            meta: Meta::default(),
            banks: vec![Arc::new(raster)],
            source: 0,
            recipe: Recipe::default(),
            crop: None,
            shot_scale: 1000,
            objects: Vec::new(),
            group_names: BTreeMap::new(),
            next_id: 1,
        }
    }

    pub fn source(&self) -> &Raster {
        &self.banks[self.source as usize]
    }

    /// Adds pixels to the bank and returns their number.
    pub fn add_bank(&mut self, raster: Raster) -> BankId {
        self.banks.push(Arc::new(raster));
        (self.banks.len() - 1) as BankId
    }

    /// Width and height of the displayed image (source after the recipe's quarter turns).
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
    /// creation sequence. Deleting one renumbers the rest, as in Little Helpers (§7 п.24).
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

    /// Start number of a counter group (the same for every member), if the group exists.
    pub fn counter_group_start(&self, group: CounterGroup) -> Option<i32> {
        self.objects.iter().find_map(|o| match o.data {
            Data::Counter {
                group: g, start, ..
            } if g == group => Some(start),
            _ => None,
        })
    }

    /// Changes the start of a whole counter group; every member is renumbered.
    pub fn set_counter_group_start(&mut self, group: CounterGroup, new_start: i32) {
        for o in &mut self.objects {
            if let Data::Counter {
                group: g, start, ..
            } = &mut o.data
                && *g == group
            {
                *start = new_start;
            }
        }
    }

    /// Appends on top, assigning an id if the object has none (or a taken one). Returns the index.
    pub fn push(&mut self, mut object: Object) -> usize {
        if object.id == 0 || self.index_of(object.id).is_some() {
            object.id = self.next_id;
        }
        self.next_id = self.next_id.max(object.id) + 1;
        if object.kind() == Kind::Pen {
            object.rect = object.bounds();
        }
        self.objects.push(object);
        self.objects.len() - 1
    }

    pub fn index_of(&self, id: ObjectId) -> Option<usize> {
        self.objects.iter().position(|o| o.id == id)
    }

    pub fn get(&self, id: ObjectId) -> Option<&Object> {
        self.objects.iter().find(|o| o.id == id)
    }

    pub fn get_mut(&mut self, id: ObjectId) -> Option<&mut Object> {
        self.objects.iter_mut().find(|o| o.id == id)
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

    /// Next free selection-group id.
    pub fn next_group_id(&self) -> GroupId {
        self.objects.iter().map(|o| o.group).max().unwrap_or(0) + 1
    }

    /// Makes members of every selection group adjacent in z-order, gathered under the highest
    /// member (LH `EdGroupsCompact`, §7 п.47). Returns the index permutation `new[i] = old[k]`.
    pub fn compact_groups(&mut self) -> Vec<usize> {
        let n = self.objects.len();
        let mut placed = vec![false; n];
        // Walk from the top; the first time a group is met (its highest member), emit all of
        // its members right there in their relative order.
        let mut top_down: Vec<usize> = Vec::with_capacity(n);
        for i in (0..n).rev() {
            if placed[i] {
                continue;
            }
            let g = self.objects[i].group;
            if g == 0 {
                top_down.push(i);
                placed[i] = true;
                continue;
            }
            let members: Vec<usize> = (0..n)
                .rev()
                .filter(|&k| self.objects[k].group == g)
                .collect();
            for k in members {
                top_down.push(k);
                placed[k] = true;
            }
        }
        let order: Vec<usize> = top_down.into_iter().rev().collect();
        let old = std::mem::take(&mut self.objects);
        let mut slots: Vec<Option<Object>> = old.into_iter().map(Some).collect();
        self.objects = order
            .iter()
            .map(|&k| slots[k].take().expect("each index once"))
            .collect();
        order
    }

    /// Turns the whole picture by quarter turns clockwise: the recipe changes, objects and the
    /// crop follow (§7 п.25).
    pub fn rotate_quarters(&mut self, quarters: i32) {
        let q = quarters.rem_euclid(4);
        for _ in 0..q {
            let (_, h) = self.image_size();
            let h = h as i32;
            let f = move |x: i32, y: i32| (h - y, x);
            let angle = |a: u16| (a + 90) % 360;
            for o in &mut self.objects {
                o.transform_with(&f, &angle);
            }
            if let Some(c) = self.crop {
                let c = c.normalized();
                let (x0, y0) = f(c.x, c.y);
                let (x1, y1) = f(c.right(), c.bottom());
                self.crop = Some(IRect::new(x0, y0, x1 - x0, y1 - y0).normalized());
            }
            self.recipe.rot_quarters = (self.recipe.rot_quarters + 1) % 4;
        }
    }

    /// Mirrors the displayed picture left–right. In the recipe the mirror is applied before the
    /// turn, so `F·R(q)·M = R(−q)·M'`: the turn inverts and the mirror flag toggles; rotated
    /// objects get `−angle` (`M·R(a) = R(−a)·M`).
    pub fn mirror_horizontal(&mut self) {
        let (w, _) = self.image_size();
        let w = w as i32;
        let f = move |x: i32, y: i32| (w - x, y);
        let angle = |a: u16| (360 - a % 360) % 360;
        for o in &mut self.objects {
            o.transform_with(&f, &angle);
        }
        if let Some(c) = self.crop {
            let c = c.normalized();
            self.crop = Some(IRect::new(w - c.right(), c.y, c.w, c.h));
        }
        self.recipe.rot_quarters = (4 - self.recipe.rot_quarters % 4) % 4;
        self.recipe.mirror = !self.recipe.mirror;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counter(seq: u32, group: u32, start: i32) -> Object {
        Object::new(
            IRect::new(0, 0, 24, 24),
            Data::Counter {
                seq,
                group,
                start,
                shape: CounterShape::Circle,
            },
        )
    }

    #[test]
    fn line_rect_keeps_direction_but_bounds_normalise() {
        let o = Object::new(
            IRect::new(100, 100, -40, -30),
            Data::Line {
                head_front: Head::Triangle,
                head_back: Head::None,
                head_size: 1,
            },
        );
        assert_eq!(o.rect.w, -40);
        assert_eq!(o.bounds(), IRect::new(60, 70, 40, 30));
    }

    #[test]
    fn counters_number_by_rank_within_group() {
        let mut doc = Document::from_raster("t", Raster::solid(10, 10, Rgb::WHITE));
        for seq in 0..3 {
            doc.push(counter(seq, 1, 5));
        }
        doc.push(counter(9, 2, 1));
        assert_eq!(doc.counter_number(0), Some(5));
        assert_eq!(doc.counter_number(2), Some(7));
        assert_eq!(doc.counter_number(3), Some(1));
        doc.objects.remove(1);
        assert_eq!(doc.counter_number(1), Some(6));
        assert_eq!(doc.next_counter_seq(), 10);
        doc.set_counter_group_start(1, 10);
        assert_eq!(doc.counter_number(0), Some(10));
        assert_eq!(doc.counter_group_start(2), Some(1));
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

    fn sample_doc() -> Document {
        let mut doc = Document::from_raster("t", Raster::solid(100, 60, Rgb::WHITE));
        doc.push(Object::new(IRect::new(10, 5, 30, 20), Data::Rect));
        doc.push(Object::new(
            IRect::new(80, 50, -60, -30),
            Data::Line {
                head_front: Head::Triangle,
                head_back: Head::None,
                head_size: 1,
            },
        ));
        doc.push(Object::new(
            IRect::new(0, 0, 0, 0),
            Data::Pen {
                points: vec![(1, 2), (40, 3), (50, 55)],
            },
        ));
        doc.push(Object::new(
            IRect::new(60, 10, 30, 12),
            Data::Hide {
                mode: HideMode::Blur,
                strength: 50,
            },
        ));
        doc.push(Object::new(
            IRect::new(20, 40, 36, 36),
            Data::Counter {
                seq: 0,
                group: 1,
                start: 1,
                shape: CounterShape::Circle,
            },
        ));
        doc.push(Object::new(
            IRect::new(5, 30, 21, 13),
            Data::Text {
                text: "Ab".into(),
                size: 20,
                bold: false,
                italic: false,
                align: Align::Left,
                box_w: 0,
            },
        ));
        doc.objects[0].rot = 30;
        doc.crop = Some(IRect::new(5, 4, 70, 50));
        doc
    }

    #[test]
    fn four_quarter_turns_are_identity() {
        let orig = sample_doc();
        let mut doc = orig.clone();
        doc.rotate_quarters(1);
        assert_eq!(doc.image_size(), (60, 100));
        assert_ne!(doc.objects, orig.objects);
        doc.rotate_quarters(3);
        assert_eq!(doc.recipe, orig.recipe);
        assert_eq!(doc.crop, orig.crop);
        assert_eq!(doc.objects, orig.objects);
    }

    #[test]
    fn quarter_turn_moves_points_and_keeps_upright_marks() {
        let mut doc = sample_doc();
        doc.rotate_quarters(1);
        // (x, y) → (H − y, x) with H = 60.
        assert_eq!(doc.objects[1].rect, IRect::new(10, 80, 30, -60));
        let Data::Pen { points } = &doc.objects[2].data else {
            unreachable!()
        };
        assert_eq!(points[0], (58, 1));
        // The rectangle turns by angle, keeps its sides.
        assert_eq!(doc.objects[0].rot, 120);
        assert_eq!((doc.objects[0].rect.w, doc.objects[0].rect.h), (30, 20));
        // Hide swaps its sides (axis-aligned effect region).
        assert_eq!((doc.objects[3].rect.w, doc.objects[3].rect.h), (12, 30));
        // Text and counters stay upright.
        assert_eq!(doc.objects[5].rot, 0);
        assert_eq!((doc.objects[5].rect.w, doc.objects[5].rect.h), (21, 13));
    }

    #[test]
    fn double_mirror_is_identity_and_mirror_negates_angle() {
        let orig = sample_doc();
        let mut doc = orig.clone();
        doc.rotate_quarters(1);
        let turned = doc.clone();
        doc.mirror_horizontal();
        assert_eq!(doc.objects[0].rot, 240);
        assert_eq!(doc.recipe.rot_quarters, 3);
        assert!(doc.recipe.mirror);
        doc.mirror_horizontal();
        assert_eq!(doc.objects, turned.objects);
        assert_eq!(doc.recipe, turned.recipe);
        assert_eq!(doc.crop, turned.crop);
    }

    #[test]
    fn compact_groups_gathers_members_under_the_highest() {
        let mut doc = Document::from_raster("t", Raster::solid(10, 10, Rgb::WHITE));
        let mk = |g: u32, n: &str| {
            let mut o = Object::new(IRect::new(0, 0, 1, 1), Data::Rect);
            o.group = g;
            o.name = Some(n.into());
            o
        };
        for (g, n) in [
            (1, "a1"),
            (0, "x"),
            (2, "b1"),
            (1, "a2"),
            (0, "y"),
            (2, "b2"),
        ] {
            doc.push(mk(g, n));
        }
        doc.compact_groups();
        let names: Vec<_> = doc
            .objects
            .iter()
            .map(|o| o.name.clone().unwrap())
            .collect();
        assert_eq!(names, ["x", "a1", "a2", "y", "b1", "b2"]);
    }

    #[test]
    fn objects_round_trip_through_json() {
        let doc = sample_doc();
        for o in &doc.objects {
            let s = serde_json::to_string(o).unwrap();
            let back: Object = serde_json::from_str(&s).unwrap();
            assert_eq!(&back, o, "{s}");
        }
        let minimal: Object =
            serde_json::from_str(r#"{"rect":{"x":1,"y":2,"w":3,"h":4},"data":{"kind":"rect"}}"#)
                .unwrap();
        assert_eq!(minimal.style, Style::default());
        assert_eq!(minimal.id, 0);
    }

    #[test]
    fn push_assigns_unique_ids() {
        let mut doc = sample_doc();
        let ids: Vec<_> = doc.objects.iter().map(|o| o.id).collect();
        assert_eq!(ids, [1, 2, 3, 4, 5, 6]);
        let mut dup = doc.objects[0].clone();
        dup.name = Some("dup".into());
        let i = doc.push(dup);
        assert_eq!(doc.objects[i].id, 7);
        assert_eq!(doc.index_of(7), Some(6));
    }
}
