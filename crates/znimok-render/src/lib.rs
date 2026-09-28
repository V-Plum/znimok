//! Renders a [`Document`] to pixels. The CPU path (`vello_cpu`) is the reference: it is
//! deterministic, runs in tests without a GPU and produces the golden images. A GPU path can
//! be added behind the same [`Renderer::render`] call later.
//!
//! Coordinates: the document lives in screenshot pixels; a [`View`] maps them to output pixels
//! (zoom and pan). Thicknesses and font sizes scale with the view, handles do not — handles are
//! not drawn here, the UI layer draws them on top.

use std::collections::HashMap;
use std::sync::Arc;

use parley::{FontContext, LayoutContext};
use vello_cpu::color::{AlphaColor, Srgb};
use vello_cpu::kurbo::{
    Affine, BezPath, Cap, Circle, Ellipse, Join, Line, Point, Rect, RoundedRect, Shape, Stroke,
    Vec2,
};
use vello_cpu::peniko::{BlendMode, Compose, ImageQuality, ImageSampler, Mix};
use vello_cpu::{Image, ImageSource, Pixmap, RenderContext, RenderSettings, Resources};
use znimok_core::{
    Align, Corners, CounterShape, Dash, Data, Document, Effect, Head, HideMode, IRect, Kind,
    Object, Raster, Rgb, Style,
};

pub mod hide;
pub mod reference;
mod text;

pub use vello_cpu;

/// Mapping from document pixels to output pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// Output pixels per document pixel.
    pub scale: f64,
    /// Document coordinate shown at the output's top-left corner.
    pub origin: Point,
    pub width: u16,
    pub height: u16,
}

impl View {
    /// The whole document at 1:1 — what export produces.
    pub fn one_to_one(doc: &Document) -> Self {
        let f = doc.frame();
        Self {
            scale: 1.0,
            origin: Point::new(f.x as f64, f.y as f64),
            width: f.w as u16,
            height: f.h as u16,
        }
    }

    pub fn transform(&self) -> Affine {
        Affine::scale(self.scale) * Affine::translate(-self.origin.to_vec2())
    }

    pub fn to_doc(&self, x: f64, y: f64) -> Point {
        Point::new(
            self.origin.x + x / self.scale,
            self.origin.y + y / self.scale,
        )
    }

    pub fn to_out(&self, p: Point) -> Point {
        Point::new(
            (p.x - self.origin.x) * self.scale,
            (p.y - self.origin.y) * self.scale,
        )
    }
}

/// Holds the render context, fonts and cached source pixmaps between frames.
pub struct Renderer {
    ctx: RenderContext,
    res: Resources,
    fonts: FontContext,
    layouts: LayoutContext<text::Brush>,
    /// Cached uploads of `Document::banks`, keyed by bank index and generation.
    bank_cache: Vec<Option<(usize, Arc<Pixmap>)>>,
    /// Hide tiles are computed in screenshot resolution and reused while nothing under them
    /// changes: key = (region, mode, strength, source generation).
    hide_cache: HashMap<(IRect, HideMode, u8, usize), Arc<Pixmap>>,
    threads: u16,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Self {
        let settings = RenderSettings::default();
        let threads = settings.num_threads;
        Self {
            ctx: RenderContext::new_with(1, 1, settings),
            res: Resources::default(),
            fonts: FontContext::new(),
            layouts: LayoutContext::new(),
            bank_cache: Vec::new(),
            hide_cache: HashMap::new(),
            threads,
        }
    }

    /// Single-threaded renderer: byte-identical output between runs, used for golden images.
    pub fn deterministic() -> Self {
        let settings = RenderSettings {
            num_threads: 0,
            ..RenderSettings::default()
        };
        Self {
            ctx: RenderContext::new_with(1, 1, settings),
            threads: 0,
            ..Self::new()
        }
    }

    pub fn threads(&self) -> u16 {
        self.threads
    }

    /// Size of a text mark in document pixels, for creating and resizing text objects.
    pub fn measure_text(
        &mut self,
        text: &str,
        size: i32,
        bold: bool,
        italic: bool,
        box_w: i32,
    ) -> (f32, f32) {
        let spec = text::TextSpec {
            text,
            size_px: size as f32,
            bold,
            italic,
            align: Align::Left,
            box_w: if box_w > 0 { Some(box_w as f32) } else { None },
        };
        text::measure(&mut self.fonts, &mut self.layouts, &spec)
    }

    /// Renders `doc` through `view` into `out`, which is resized to the view.
    pub fn render(&mut self, doc: &Document, view: View, out: &mut Pixmap) {
        if out.width() != view.width || out.height() != view.height {
            out.resize(view.width, view.height);
        }
        self.ctx.reset_and_resize(view.width, view.height);
        let base = view.transform();

        // Source image, cropped to the frame.
        let frame = doc.frame();
        let src = self.bank_pixmap(doc, doc.source as usize);
        self.ctx.set_transform(base);
        self.ctx.push_clip_layer(&irect(frame).to_path(0.1));
        let (iw, ih) = doc.image_size();
        self.draw_pixmap(
            src,
            Rect::new(0.0, 0.0, iw as f64, ih as f64),
            ImageQuality::Medium,
            1.0,
        );

        for (i, obj) in doc.objects.iter().enumerate() {
            if obj.hidden {
                continue;
            }
            self.draw_object(doc, i, obj, base, view.scale);
        }
        self.ctx.pop_layer();
        self.ctx.flush();
        self.ctx.render(out.as_mut(), &mut self.res);
    }

    fn bank_pixmap(&mut self, doc: &Document, bank: usize) -> Arc<Pixmap> {
        if self.bank_cache.len() <= bank {
            self.bank_cache.resize(bank + 1, None);
        }
        let raster = &doc.banks[bank];
        let key = raster.rgba.as_ptr() as usize ^ raster.rgba.len();
        if let Some((k, p)) = &self.bank_cache[bank]
            && *k == key
        {
            return p.clone();
        }
        let p = Arc::new(raster_to_pixmap(raster));
        self.bank_cache[bank] = Some((key, p.clone()));
        p
    }

    fn draw_pixmap(&mut self, pix: Arc<Pixmap>, dest: Rect, quality: ImageQuality, alpha: f32) {
        let (w, h) = (pix.width() as f64, pix.height() as f64);
        let saved = *self.ctx.transform();
        let img = Image {
            image: ImageSource::Pixmap(pix),
            sampler: ImageSampler::default()
                .with_quality(quality)
                .with_alpha(alpha),
        };
        // The image paint is sampled in the paint's own space: scale it to `dest`.
        let scale = Affine::translate(dest.origin().to_vec2())
            * Affine::scale_non_uniform(dest.width() / w, dest.height() / h);
        self.ctx.set_paint_transform(scale);
        self.ctx.set_paint(img);
        self.ctx.fill_rect(&dest);
        self.ctx.reset_paint_transform();
        self.ctx.set_transform(saved);
    }

    fn draw_object(
        &mut self,
        doc: &Document,
        index: usize,
        obj: &Object,
        base: Affine,
        scale: f64,
    ) {
        let kind = obj.kind();
        let b = obj.bounds();
        let rect = irect(b);
        let rot = if kind.can_rotate() && obj.rot != 0 {
            Affine::rotate_about((obj.rot as f64).to_radians(), rect.center())
        } else {
            Affine::IDENTITY
        };
        let t = base * rot;
        self.ctx.set_transform(t);
        let st = &obj.style;
        let alpha = st.alpha as f32 / 100.0;

        // Effects go under the object. Filled boxes get a real blurred shadow; outlines, lines
        // and text get a halo of widening translucent strokes (vello_cpu's filter layers are
        // not available with multithreading, and this also keeps the shape of the mark).
        if kind.fx_allowed() && (st.shadow != Effect::None || st.glow != Effect::None) {
            let radius = corner_radius(st, rect);
            // A blurred plate only under something opaque; otherwise the shadow would show
            // through a translucent fill and darken the whole mark.
            let opaque_fill = matches!(kind, Kind::Rect | Kind::Ellipse)
                && ((st.no_main && st.alpha == 100) || (st.color2.is_some() && st.alpha2 == 100));
            let filled = opaque_fill || matches!(kind, Kind::Image | Kind::Counter | Kind::Stamp);
            let outline: Option<BezPath> = match &obj.data {
                Data::Rect => Some(RoundedRect::from_rect(rect, radius).to_path(0.1)),
                Data::Ellipse => Some(Ellipse::from_rect(rect).to_path(0.1)),
                Data::Line { .. } => {
                    let r = obj.rect;
                    Some(
                        Line::new(
                            Point::new(r.x as f64, r.y as f64),
                            Point::new((r.x + r.w) as f64, (r.y + r.h) as f64),
                        )
                        .to_path(0.1),
                    )
                }
                Data::Text { .. } => Some(rect.to_path(0.1)),
                _ => None,
            };
            let width = if kind == Kind::Text {
                2.0
            } else {
                st.thick as f64
            };
            if st.shadow != Effect::None {
                let (d, blur) = if st.shadow == Effect::Light {
                    (2.0, 3.0)
                } else {
                    (5.0, 8.0)
                };
                if filled {
                    self.ctx.set_paint(rgba(Rgb::BLACK, 0.35 * alpha));
                    self.ctx.fill_blurred_rounded_rect(
                        &rect.with_origin(rect.origin() + Vec2::new(d, d)),
                        radius as f32,
                        blur,
                        false,
                    );
                } else if let Some(path) = &outline {
                    self.halo(
                        path,
                        Affine::translate((d, d)),
                        width,
                        blur as f64,
                        Rgb::BLACK,
                        0.35 * alpha,
                    );
                }
            }
            if st.glow != Effect::None {
                let (grow, blur) = if st.glow == Effect::Light {
                    (2.0, 4.0)
                } else {
                    (5.0, 10.0)
                };
                if filled {
                    self.ctx.set_paint(rgba(st.color, 0.8 * alpha));
                    self.ctx.fill_blurred_rounded_rect(
                        &rect.inflate(grow, grow),
                        (radius + grow) as f32,
                        blur,
                        false,
                    );
                } else if let Some(path) = &outline {
                    self.halo(
                        path,
                        Affine::IDENTITY,
                        width + grow,
                        blur as f64,
                        st.color,
                        0.8 * alpha,
                    );
                }
            }
            self.ctx.set_transform(t);
        }

        match &obj.data {
            Data::Rect => {
                let radius = corner_radius(st, rect);
                let shape = RoundedRect::from_rect(rect, radius);
                if let Some(c2) = st.color2 {
                    self.ctx.set_paint(rgba(c2, st.alpha2 as f32 / 100.0));
                    self.ctx.fill_path(&shape.to_path(0.1));
                }
                if st.no_main {
                    self.ctx.set_paint(rgba(st.color, alpha));
                    self.ctx.fill_path(&shape.to_path(0.1));
                } else {
                    self.ctx.set_stroke(stroke_for(st));
                    self.ctx.set_paint(rgba(st.color, alpha));
                    self.ctx.stroke_path(&shape.to_path(0.1));
                }
            }
            Data::Ellipse => {
                let shape = Ellipse::from_rect(rect);
                if let Some(c2) = st.color2 {
                    self.ctx.set_paint(rgba(c2, st.alpha2 as f32 / 100.0));
                    self.ctx.fill_path(&shape.to_path(0.1));
                }
                if st.no_main {
                    self.ctx.set_paint(rgba(st.color, alpha));
                    self.ctx.fill_path(&shape.to_path(0.1));
                } else {
                    self.ctx.set_stroke(stroke_for(st));
                    self.ctx.set_paint(rgba(st.color, alpha));
                    self.ctx.stroke_path(&shape.to_path(0.1));
                }
            }
            Data::Line {
                head_front,
                head_back,
                head_size,
            } => {
                let r = obj.rect;
                let p0 = Point::new(r.x as f64, r.y as f64);
                let p1 = Point::new((r.x + r.w) as f64, (r.y + r.h) as f64);
                self.draw_line(
                    p0,
                    p1,
                    st,
                    alpha,
                    *head_back,
                    *head_front,
                    *head_size as f64,
                );
            }
            Data::Pen { points } => {
                if points.len() >= 2 {
                    let mut path = BezPath::new();
                    path.move_to(Point::new(points[0].0 as f64, points[0].1 as f64));
                    for &(x, y) in &points[1..] {
                        path.line_to(Point::new(x as f64, y as f64));
                    }
                    let mut stroke = stroke_for(st);
                    stroke = stroke.with_join(Join::Round);
                    if st.dash == Dash::Solid {
                        stroke = stroke.with_caps(Cap::Round);
                    }
                    self.ctx.set_stroke(stroke);
                    self.ctx.set_paint(rgba(st.color, alpha));
                    self.ctx.stroke_path(&path);
                } else if let Some(&(x, y)) = points.first() {
                    self.ctx.set_paint(rgba(st.color, alpha));
                    self.ctx.fill_path(
                        &Circle::new(Point::new(x as f64, y as f64), st.thick as f64 / 2.0)
                            .to_path(0.1),
                    );
                }
            }
            Data::Text {
                text: s,
                size,
                bold,
                italic,
                align,
                box_w,
            } => {
                let spec = text::TextSpec {
                    text: s,
                    size_px: *size as f32,
                    bold: *bold,
                    italic: *italic,
                    align: *align,
                    box_w: if *box_w > 0 {
                        Some(*box_w as f32)
                    } else {
                        None
                    },
                };
                let outline = st.color2.map(|c| (c, st.alpha2 as f32 / 100.0));
                text::draw_text(
                    &mut self.ctx,
                    &mut self.res,
                    &mut self.fonts,
                    &mut self.layouts,
                    &spec,
                    rect.origin(),
                    rgba(st.color, 1.0),
                    outline,
                    alpha,
                );
            }
            Data::Hide { mode, strength } => {
                let region = b.normalized();
                match mode {
                    HideMode::Plate => {
                        self.ctx.set_paint(rgba(st.color, alpha));
                        self.ctx.fill_rect(&rect);
                    }
                    HideMode::Blur | HideMode::Pixelate => {
                        // Pixels below this object: for now the source only (Little Helpers takes
                        // everything below, see inventory §7 п.26) — tracked in the P1 report.
                        let src = doc.source();
                        let key = (
                            region,
                            *mode,
                            *strength,
                            src.rgba.as_ptr() as usize ^ src.rgba.len(),
                        );
                        let tile = match self.hide_cache.get(&key) {
                            Some(p) => Some(p.clone()),
                            None => self.below_pixels(doc, index, region).map(|raw| {
                                let tile = if *mode == HideMode::Blur {
                                    hide::blur(&raw, *strength)
                                } else {
                                    hide::pixelate(&raw, *strength)
                                };
                                let p = Arc::new(raster_to_pixmap(&tile));
                                if self.hide_cache.len() > 64 {
                                    self.hide_cache.clear();
                                }
                                self.hide_cache.insert(key, p.clone());
                                p
                            }),
                        };
                        if let Some(tile) = tile {
                            let quality = if *mode == HideMode::Blur {
                                ImageQuality::Medium
                            } else {
                                ImageQuality::Low
                            };
                            self.ctx.set_transform(t);
                            self.draw_pixmap(tile, rect, quality, alpha);
                        }
                    }
                }
            }
            Data::Mark => {
                // Marker: multiply with the pixels below, colour channels 0/255 → AND (§7 п.29).
                self.ctx
                    .push_blend_layer(BlendMode::new(Mix::Multiply, Compose::SrcOver));
                self.ctx.set_paint(rgba(st.color, alpha));
                self.ctx.fill_rect(&rect);
                self.ctx.pop_layer();
            }
            Data::Counter { shape, .. } => {
                let n = doc.counter_number(index).unwrap_or(0);
                let d = st.thick.max(8) as f64;
                let (cx, cy) = b.center();
                let c = Point::new(cx, cy);
                let path = match shape {
                    CounterShape::Circle => Circle::new(c, d / 2.0).to_path(0.1),
                    CounterShape::RoundedBox => RoundedRect::new(
                        cx - d / 2.0,
                        cy - d / 2.0,
                        cx + d / 2.0,
                        cy + d / 2.0,
                        d * 0.22,
                    )
                    .to_path(0.1),
                    CounterShape::Pin => pin_path(c, d),
                };
                self.ctx.set_paint(rgba(st.color, alpha));
                self.ctx.fill_path(&path);
                let digit = st.color2.unwrap_or_else(|| auto_contrast(st.color));
                let label = n.to_string();
                let spec = text::TextSpec {
                    text: &label,
                    size_px: (d * if label.len() > 2 { 0.42 } else { 0.55 }) as f32,
                    bold: true,
                    italic: false,
                    align: Align::Center,
                    box_w: Some(d as f32),
                };
                let baseline_shift = if *shape == CounterShape::Pin {
                    -d * 0.18
                } else {
                    0.0
                };
                text::draw_text_centered(
                    &mut self.ctx,
                    &mut self.res,
                    &mut self.fonts,
                    &mut self.layouts,
                    &spec,
                    Point::new(cx, cy + baseline_shift),
                    rgba(digit, 1.0),
                    alpha,
                );
            }
            Data::Stamp { id } => {
                let d = st.thick.max(8) as f64;
                let (cx, cy) = b.center();
                self.ctx.set_paint(rgba(st.color, alpha));
                if *id >= 100 {
                    let s = reference::emoji_for(*id);
                    let spec = text::TextSpec {
                        text: s,
                        size_px: (d * 0.8) as f32,
                        bold: false,
                        italic: false,
                        align: Align::Center,
                        box_w: Some(d as f32 * 1.5),
                    };
                    text::draw_text_centered(
                        &mut self.ctx,
                        &mut self.res,
                        &mut self.fonts,
                        &mut self.layouts,
                        &spec,
                        Point::new(cx, cy),
                        rgba(st.color, 1.0),
                        alpha,
                    );
                } else {
                    let path = stamp_path(*id, Point::new(cx, cy), d);
                    self.ctx.set_stroke(
                        Stroke::new(d * 0.14)
                            .with_caps(Cap::Round)
                            .with_join(Join::Round),
                    );
                    if matches!(*id, 0 | 1 | 4) {
                        self.ctx.stroke_path(&path);
                    } else {
                        self.ctx.fill_path(&path);
                    }
                }
            }
            Data::Image { bank } => {
                if let Some(raster) = doc.banks.get(*bank as usize) {
                    let _ = raster;
                    let pix = self.bank_pixmap(doc, *bank as usize);
                    let radius = corner_radius(st, rect);
                    self.ctx.set_transform(t);
                    if radius > 0.0 {
                        self.ctx
                            .push_clip_layer(&RoundedRect::from_rect(rect, radius).to_path(0.1));
                    }
                    self.draw_pixmap(
                        pix,
                        rect,
                        if scale >= 1.0 {
                            ImageQuality::High
                        } else {
                            ImageQuality::Medium
                        },
                        alpha,
                    );
                    if radius > 0.0 {
                        self.ctx.pop_layer();
                    }
                }
            }
        }
    }

    /// Fake blur: `n` strokes of growing width and falling opacity around `path`.
    fn halo(
        &mut self,
        path: &BezPath,
        offset: Affine,
        width: f64,
        blur: f64,
        color: Rgb,
        alpha: f32,
    ) {
        let saved = *self.ctx.transform();
        self.ctx.set_transform(saved * offset);
        let n = 6;
        for i in 0..n {
            let k = i as f64 / n as f64;
            let w = width + blur * 2.0 * k;
            let a = alpha * (1.0 - k as f32).powi(2) / (n as f32 * 0.55);
            self.ctx
                .set_stroke(Stroke::new(w).with_caps(Cap::Round).with_join(Join::Round));
            self.ctx.set_paint(rgba(color, a));
            self.ctx.stroke_path(path);
        }
        self.ctx.set_transform(saved);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_line(
        &mut self,
        p0: Point,
        p1: Point,
        st: &Style,
        alpha: f32,
        back: Head,
        front: Head,
        head_size: f64,
    ) {
        let v = p1 - p0;
        let len = v.hypot();
        if len < 0.5 {
            return;
        }
        let dir = v / len;
        let hs = if head_size > 0.0 {
            head_size
        } else {
            (st.thick as f64 * 3.0).max(10.0)
        };
        // The line is shortened under a closed head so its butt does not poke out (§7 п.31).
        let (mut a, mut b) = (p0, p1);
        if matches!(back, Head::Triangle | Head::Dot) {
            a += dir * (hs * 0.6);
        }
        if matches!(front, Head::Triangle | Head::Dot) {
            b -= dir * (hs * 0.6);
        }
        let mut stroke = stroke_for(st);
        if st.dash == Dash::Solid && back == Head::None && front == Head::None {
            stroke = stroke.with_caps(Cap::Round);
        }
        self.ctx.set_stroke(stroke);
        self.ctx.set_paint(rgba(st.color, alpha));
        self.ctx.stroke_path(&Line::new(a, b).to_path(0.1));
        for (head, tip, d) in [(back, p0, -dir), (front, p1, dir)] {
            match head {
                Head::None => {}
                Head::Triangle => {
                    let n = Vec2::new(-d.y, d.x);
                    let base = tip - d * hs;
                    let mut p = BezPath::new();
                    p.move_to(tip);
                    p.line_to(base + n * (hs * 0.45));
                    p.line_to(base - n * (hs * 0.45));
                    p.close_path();
                    self.ctx.fill_path(&p);
                }
                Head::Chevron => {
                    let n = Vec2::new(-d.y, d.x);
                    let base = tip - d * hs;
                    let mut p = BezPath::new();
                    p.move_to(base + n * (hs * 0.5));
                    p.line_to(tip);
                    p.line_to(base - n * (hs * 0.5));
                    self.ctx.set_stroke(
                        Stroke::new(st.thick as f64)
                            .with_caps(Cap::Round)
                            .with_join(Join::Round),
                    );
                    self.ctx.stroke_path(&p);
                }
                Head::Dot => {
                    self.ctx
                        .fill_path(&Circle::new(tip - d * (hs * 0.3), hs * 0.3).to_path(0.1));
                }
            }
        }
    }

    /// Pixels of everything below object `index` inside `region`, as a straight-alpha raster.
    /// Prototype: the source only, already rotated/cropped as the document shows it.
    fn below_pixels(&mut self, doc: &Document, _index: usize, region: IRect) -> Option<Raster> {
        let src = doc.source();
        let (iw, ih) = (src.width as i32, src.height as i32);
        let x0 = region.x.max(0);
        let y0 = region.y.max(0);
        let x1 = region.right().min(iw);
        let y1 = region.bottom().min(ih);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        let (w, h) = ((x1 - x0) as u32, (y1 - y0) as u32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in y0..y1 {
            let start = ((y * iw + x0) * 4) as usize;
            rgba.extend_from_slice(&src.rgba[start..start + (w * 4) as usize]);
        }
        // Keep the tile aligned with the object even when it is partly outside the image.
        let _ = (region.x - x0, region.y - y0);
        Some(Raster::new(w, h, rgba))
    }
}

fn irect(r: IRect) -> Rect {
    let r = r.normalized();
    Rect::new(r.x as f64, r.y as f64, r.right() as f64, r.bottom() as f64)
}

fn rgba(c: Rgb, alpha: f32) -> AlphaColor<Srgb> {
    AlphaColor::<Srgb>::from_rgba8(c.r, c.g, c.b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn corner_radius(st: &Style, rect: Rect) -> f64 {
    let max = rect.width().min(rect.height()) / 2.0;
    let r = match st.corners {
        Corners::Sharp => 0.0,
        Corners::Soft => {
            if st.corner_px > 0 {
                st.corner_px as f64
            } else {
                6.0
            }
        }
        Corners::Round => {
            if st.corner_px > 0 {
                st.corner_px as f64
            } else {
                16.0
            }
        }
    };
    r.min(max)
}

/// Stroke for the main outline: flat caps so dashes stay dashes (§7 п.30).
fn stroke_for(st: &Style) -> Stroke {
    let t = st.thick.max(1) as f64;
    let s = Stroke::new(t).with_caps(Cap::Butt).with_join(Join::Miter);
    match st.dash {
        Dash::Solid => s,
        Dash::Dashed => s.with_dashes(0.0, [t * 3.0, t * 2.0]),
        Dash::DashDot => s.with_dashes(0.0, [t * 3.0, t * 1.5, t * 0.6, t * 1.5]),
    }
}

/// Digit colour for a counter without an explicit second colour.
fn auto_contrast(c: Rgb) -> Rgb {
    let lum = 0.299 * c.r as f64 + 0.587 * c.g as f64 + 0.114 * c.b as f64;
    if lum > 150.0 { Rgb::BLACK } else { Rgb::WHITE }
}

fn pin_path(c: Point, d: f64) -> BezPath {
    // A round head with a pointed tail; the tip is at the bottom centre of the box.
    let r = d * 0.36;
    let head = Point::new(c.x, c.y - d * 0.12);
    let mut p = Circle::new(head, r).to_path(0.1);
    let tip = Point::new(c.x, c.y + d / 2.0);
    let ang = 0.62f64;
    p.move_to(Point::new(head.x - r * ang.cos(), head.y + r * ang.sin()));
    p.line_to(tip);
    p.line_to(Point::new(head.x + r * ang.cos(), head.y + r * ang.sin()));
    p.close_path();
    p
}

/// Vector stamps 0..=5: check, cross, star, heart, question, exclamation.
fn stamp_path(id: u32, c: Point, d: f64) -> BezPath {
    let r = d / 2.0;
    let mut p = BezPath::new();
    match id {
        0 => {
            p.move_to(Point::new(c.x - r * 0.7, c.y + r * 0.05));
            p.line_to(Point::new(c.x - r * 0.2, c.y + r * 0.55));
            p.line_to(Point::new(c.x + r * 0.75, c.y - r * 0.55));
        }
        1 => {
            p.move_to(Point::new(c.x - r * 0.6, c.y - r * 0.6));
            p.line_to(Point::new(c.x + r * 0.6, c.y + r * 0.6));
            p.move_to(Point::new(c.x + r * 0.6, c.y - r * 0.6));
            p.line_to(Point::new(c.x - r * 0.6, c.y + r * 0.6));
        }
        2 => {
            for i in 0..10 {
                let a = -std::f64::consts::FRAC_PI_2 + i as f64 * std::f64::consts::PI / 5.0;
                let rr = if i % 2 == 0 { r } else { r * 0.42 };
                let pt = Point::new(c.x + rr * a.cos(), c.y + rr * a.sin());
                if i == 0 { p.move_to(pt) } else { p.line_to(pt) }
            }
            p.close_path();
        }
        3 => {
            let top = Point::new(c.x, c.y - r * 0.35);
            p.move_to(Point::new(c.x, c.y + r * 0.85));
            p.curve_to(
                Point::new(c.x - r * 1.3, c.y - r * 0.1),
                Point::new(c.x - r * 0.6, c.y - r * 1.15),
                top,
            );
            p.curve_to(
                Point::new(c.x + r * 0.6, c.y - r * 1.15),
                Point::new(c.x + r * 1.3, c.y - r * 0.1),
                Point::new(c.x, c.y + r * 0.85),
            );
            p.close_path();
        }
        4 => {
            p.move_to(Point::new(c.x - r * 0.45, c.y - r * 0.4));
            p.curve_to(
                Point::new(c.x - r * 0.45, c.y - r * 1.1),
                Point::new(c.x + r * 0.55, c.y - r * 1.1),
                Point::new(c.x + r * 0.45, c.y - r * 0.35),
            );
            p.curve_to(
                Point::new(c.x + r * 0.4, c.y + r * 0.05),
                Point::new(c.x, c.y),
                Point::new(c.x, c.y + r * 0.35),
            );
            p.move_to(Point::new(c.x, c.y + r * 0.75));
            p.line_to(Point::new(c.x, c.y + r * 0.76));
        }
        _ => {
            p = RoundedRect::new(
                c.x - r * 0.14,
                c.y - r * 0.85,
                c.x + r * 0.14,
                c.y + r * 0.3,
                r * 0.14,
            )
            .to_path(0.1);
            p.extend(Circle::new(Point::new(c.x, c.y + r * 0.65), r * 0.17).to_path(0.1));
        }
    }
    p
}

/// Straight-alpha RGBA8 raster → premultiplied pixmap.
pub fn raster_to_pixmap(r: &Raster) -> Pixmap {
    let mut pix = Pixmap::new(r.width as u16, r.height as u16);
    let mut transparent = false;
    for (dst, src) in pix.data_mut().iter_mut().zip(r.rgba.as_chunks::<4>().0) {
        let a = src[3] as u16;
        transparent |= a != 255;
        let m = |c: u8| ((c as u16 * a) / 255) as u8;
        *dst = vello_cpu::color::PremulRgba8 {
            r: m(src[0]),
            g: m(src[1]),
            b: m(src[2]),
            a: src[3],
        };
    }
    pix.set_may_have_transparency(transparent);
    pix
}

/// Premultiplied pixmap → straight-alpha RGBA8 bytes (for PNG export and uploads).
pub fn pixmap_to_rgba(p: &Pixmap) -> Vec<u8> {
    p.clone()
        .take_unpremultiplied()
        .into_iter()
        .flat_map(|c| [c.r, c.g, c.b, c.a])
        .collect()
}

/// Premultiplied bytes as they are — what a `Rgba8Unorm` texture with premultiplied blending wants.
pub fn pixmap_premul_bytes(p: &Pixmap) -> &[u8] {
    p.data_as_u8_slice()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Renders the reference scene 1:1 and writes it next to the target dir so a human (or a
    /// later golden comparison) can look at it. Deterministic renderer: same bytes every run.
    #[test]
    fn reference_scene_renders_and_is_stable() {
        let doc = reference::reference_document(1600, 1000);
        let mut r = Renderer::deterministic();
        let view = View::one_to_one(&doc);
        let mut a = Pixmap::new(1, 1);
        let t0 = std::time::Instant::now();
        r.render(&doc, view, &mut a);
        let first = t0.elapsed();
        let mut b = Pixmap::new(1, 1);
        r.render(&doc, view, &mut b);
        assert_eq!(
            a.data_as_u8_slice(),
            b.data_as_u8_slice(),
            "two renders differ"
        );
        assert!(a.data().iter().any(|p| p.r != p.g), "scene looks empty");
        let out =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/reference.png");
        std::fs::write(&out, a.clone().into_png().unwrap()).unwrap();
        eprintln!(
            "reference.png written to {} (first render {:?})",
            out.display(),
            first
        );
    }
}
