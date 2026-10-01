//! Renders a [`Document`] to pixels. The CPU path (`vello_cpu`) is the reference: it is
//! deterministic, runs in tests without a GPU and produces the golden images. A GPU path can
//! be added behind the same [`Renderer::render`] call later.
//!
//! Geometry and effect formulas are ported from Little Helpers (`EdDrawObjectRaw`, `EdDrawHead`,
//! `EdHideRadius`/`EdHideBlock`, `EdFxTile`, `EdCounterPath`, `EdStampShape`) so the marks look
//! the same; where this file deviates on purpose it says so.
//!
//! Coordinates: the document lives in screenshot pixels; a [`View`] maps them to output pixels
//! (zoom and pan). Thicknesses and font sizes scale with the view, handles do not — handles are
//! not drawn here, the UI layer draws them on top.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use parley::{FontContext, LayoutContext};
use vello_cpu::color::{AlphaColor, PremulRgba8, Srgb};
use vello_cpu::kurbo::{
    Affine, Arc as KArc, BezPath, Cap, Circle, Ellipse, Join, Line, Point, Rect, RoundedRect,
    Shape, Stroke, Vec2,
};
use vello_cpu::peniko::{BlendMode, Compose, ImageQuality, ImageSampler, Mix};
use vello_cpu::{Image, ImageSource, Pixmap, RenderContext, RenderSettings, Resources};
use znimok_core::{
    Align, Corners, CounterShape, Dash, Data, Document, Effect, Head, HideMode, IRect, Kind,
    Object, Raster, Rgb, Style,
};

mod damage;
pub mod develop;
pub mod hide;
pub mod reference;
mod text;

pub use damage::{Repaint, Tracker, merge as merge_rects};

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

/// Shadow/glow presets from Little Helpers: offset and blur radius in screenshot pixels,
/// alpha 0..255, boost in % applied after the blur.
#[derive(Clone, Copy)]
struct FxPreset {
    off: f64,
    blur: f64,
    alpha: u32,
    boost: u32,
}

const SHADOW: [FxPreset; 3] = [
    FxPreset {
        off: 0.0,
        blur: 0.0,
        alpha: 0,
        boost: 100,
    },
    FxPreset {
        off: 2.0,
        blur: 2.0,
        alpha: 120,
        boost: 100,
    },
    FxPreset {
        off: 5.0,
        blur: 5.0,
        alpha: 170,
        boost: 100,
    },
];
const GLOW: [FxPreset; 3] = [
    FxPreset {
        off: 0.0,
        blur: 0.0,
        alpha: 0,
        boost: 100,
    },
    FxPreset {
        off: 0.0,
        blur: 3.0,
        alpha: 235,
        boost: 260,
    },
    FxPreset {
        off: 0.0,
        blur: 6.0,
        alpha: 255,
        boost: 340,
    },
];

fn preset(set: &[FxPreset; 3], e: Effect) -> FxPreset {
    set[match e {
        Effect::None => 0,
        Effect::Light => 1,
        Effect::Strong => 2,
    }]
}

/// Holds the render context, fonts and cached tiles between frames.
pub struct Renderer {
    ctx: RenderContext,
    /// Second context for rendering a single mark into an effects tile.
    fx_ctx: RenderContext,
    res: Resources,
    fonts: FontContext,
    layouts: LayoutContext<text::Brush>,
    /// Cached uploads of `Document::banks`, keyed by bank index and generation.
    bank_cache: Vec<Option<(usize, Arc<Pixmap>)>>,
    /// The source developed by the recipe (mirror, turns, tone), keyed by `develop::key`.
    developed: Option<(u64, Arc<znimok_core::Raster>, Arc<Pixmap>)>,
    /// Hide tiles are computed in screenshot resolution and reused while nothing under them
    /// changes: key = (region, mode, strength, source generation, the marks below).
    hide_cache: HashMap<(IRect, HideMode, u8, usize, u64), Arc<Pixmap>>,
    /// Renders what lies below a Hide (the picture and the marks under it), made on first use.
    below: Option<Box<Renderer>>,
    settings: RenderSettings,
    /// Shadow/glow layers in output resolution: key = (mark hash, scale ×1000, sub-pixel
    /// offset in quarters).
    fx_cache: HashMap<(u64, i64, u8, u8), Arc<Pixmap>>,
    /// A tile for partial repaints (ZK-130), kept between frames.
    scratch: Pixmap,
    threads: u16,
    /// Draw the picture under the marks (false: a video shows through, ZK-92).
    picture: bool,
    /// Effects that need the pixels below as plates (ZK-94, a playing video): a Hide is a hatched
    /// plate, a marker a translucent one — there are no pixels below on the CPU then.
    plain_effects: bool,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Self {
        Self::with_settings(RenderSettings::default())
    }

    /// Single-threaded renderer: byte-identical output between runs, used for golden images.
    pub fn deterministic() -> Self {
        Self::with_settings(RenderSettings {
            num_threads: 0,
            ..RenderSettings::default()
        })
    }

    fn with_settings(settings: RenderSettings) -> Self {
        let threads = settings.num_threads;
        let mut fonts = FontContext::new();
        text::register_bundled(&mut fonts);
        Self {
            ctx: RenderContext::new_with(1, 1, settings),
            fx_ctx: RenderContext::new_with(1, 1, settings),
            res: Resources::default(),
            fonts,
            layouts: LayoutContext::new(),
            bank_cache: Vec::new(),
            developed: None,
            hide_cache: HashMap::new(),
            fx_cache: HashMap::new(),
            below: None,
            settings,
            scratch: Pixmap::new(1, 1),
            threads,
            picture: true,
            plain_effects: false,
        }
    }

    pub fn threads(&self) -> u16 {
        self.threads
    }

    /// Whether the picture is drawn under the marks. Off for a playing video (ZK-92): its frames
    /// are shown on the GPU under a transparent canvas, and the marks are drawn over them. Hide
    /// marks still sample the document's source bank.
    pub fn set_picture(&mut self, on: bool) {
        self.picture = on;
    }

    pub fn picture(&self) -> bool {
        self.picture
    }

    /// Hide and marker as plates instead of effects on the pixels below (ZK-94).
    pub fn set_plain_effects(&mut self, on: bool) {
        self.plain_effects = on;
    }

    pub fn plain_effects(&self) -> bool {
        self.plain_effects
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

    /// Caret and selection of a text mark being edited, in document pixels (ZK-49).
    pub fn text_caret(
        &mut self,
        obj: &Object,
        cursor: usize,
        anchor: usize,
    ) -> Option<(Rect, Vec<Rect>)> {
        let spec = text_spec(obj)?;
        let (c, sel) = text::caret(&mut self.fonts, &mut self.layouts, &spec, cursor, anchor);
        let o = Point::new(obj.rect.x as f64, obj.rect.y as f64).to_vec2();
        Some((c + o, sel.into_iter().map(|r| r + o).collect()))
    }

    /// Byte offset in a text mark under a document point.
    pub fn text_hit(&mut self, obj: &Object, x: f64, y: f64) -> Option<usize> {
        let spec = text_spec(obj)?;
        Some(text::hit(
            &mut self.fonts,
            &mut self.layouts,
            &spec,
            (x - obj.rect.x as f64) as f32,
            (y - obj.rect.y as f64) as f32,
        ))
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
        self.ctx.set_transform(base);
        self.ctx.push_clip_layer(&irect(frame).to_path(0.1));
        if self.picture {
            let (_, src) = self.developed(doc);
            let (iw, ih) = doc.image_size();
            self.draw_pixmap(
                src,
                Rect::new(0.0, 0.0, iw as f64, ih as f64),
                ImageQuality::Medium,
                1.0,
            );
        }

        for (i, obj) in doc.objects.iter().enumerate() {
            // Hidden, or a video's mark whose time does not cover the frame shown (ZK-94).
            if obj.hidden || !doc.live(obj) {
                continue;
            }
            if fx_on(obj) {
                self.draw_fx(doc, i, obj, base, view.scale);
            }
            let t = base * rotation(obj);
            self.ctx.set_transform(t);
            self.draw_raw(doc, i, obj, t, view.scale);
        }
        self.ctx.pop_layer();
        self.ctx.flush();
        self.ctx.render(out.as_mut(), &mut self.res);
        if self.fx_cache.len() > 256 {
            self.fx_cache.clear();
        }
    }

    /// The picture as the document shows it (see [`develop`]); the identity recipe reuses the
    /// source bank without a copy.
    pub fn developed(&mut self, doc: &Document) -> (Arc<znimok_core::Raster>, Arc<Pixmap>) {
        let src = doc.source();
        let r = &doc.recipe;
        let key = develop::key(src, r);
        if let Some((k, raster, pix)) = &self.developed
            && *k == key
        {
            return (raster.clone(), pix.clone());
        }
        let (raster, pix) = if develop::is_identity(r) {
            (
                doc.banks[doc.source as usize].clone(),
                self.bank_pixmap(doc, doc.source as usize),
            )
        } else {
            let d = Arc::new(develop::develop(src, r));
            let p = Arc::new(raster_to_pixmap(&d));
            (d, p)
        };
        self.developed = Some((key, raster.clone(), pix.clone()));
        (raster, pix)
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
        if alpha <= 0.0 {
            return;
        }
        // vello_common 0.2 panics on an image paint with alpha («Applying opacity to image
        // commands» is unimplemented — ZK-204): the image goes opaque into an opacity layer.
        let translucent = alpha < 1.0;
        if translucent {
            self.ctx.push_opacity_layer(alpha);
        }
        let (w, h) = (pix.width() as f64, pix.height() as f64);
        let img = Image {
            image: ImageSource::Pixmap(pix),
            sampler: ImageSampler::default().with_quality(quality),
        };
        // The image paint is sampled in the paint's own space: scale it to `dest`.
        let scale = Affine::translate(dest.origin().to_vec2())
            * Affine::scale_non_uniform(dest.width() / w, dest.height() / h);
        self.ctx.set_paint_transform(scale);
        self.ctx.set_paint(img);
        self.ctx.fill_rect(&dest);
        self.ctx.reset_paint_transform();
        if translucent {
            self.ctx.pop_layer();
        }
    }

    // ---- effects: the mark rendered alone into a tile, its alpha shifted, blurred, boosted
    // and tinted (black shadow, white glow), laid under the mark (LH `EdFxTile`).

    fn draw_fx(&mut self, doc: &Document, index: usize, obj: &Object, base: Affine, scale: f64) {
        let sh = preset(&SHADOW, obj.style.shadow);
        let gl = preset(&GLOW, obj.style.glow);
        let extent = (sh.off + sh.blur * 3.0).max(gl.blur * 3.0) + 2.0;
        let rot = rotation(obj);
        // Output-space bounds of the rotated mark, padded like LH `EdFxBounds`.
        let b = obj.bounds();
        let mut pad = obj.style.thick as f64 + extent;
        if obj.kind().is_segment() {
            pad += head_len(obj) * 1.2;
        }
        if obj.kind() == Kind::Text {
            pad += 10.0;
        }
        if obj.kind() == Kind::Counter {
            pad += b.w as f64 * 0.1;
        }
        let r = irect(b).inflate(pad, pad);
        let t = base * rot;
        let corners = [
            t * Point::new(r.x0, r.y0),
            t * Point::new(r.x1, r.y0),
            t * Point::new(r.x0, r.y1),
            t * Point::new(r.x1, r.y1),
        ];
        let (x0, y0) = corners
            .iter()
            .fold((f64::MAX, f64::MAX), |a, p| (a.0.min(p.x), a.1.min(p.y)));
        let (x1, y1) = corners
            .iter()
            .fold((f64::MIN, f64::MIN), |a, p| (a.0.max(p.x), a.1.max(p.y)));
        let (ox, oy) = (x0.floor(), y0.floor());
        let (bw, bh) = ((x1 - ox).ceil() as usize + 1, (y1 - oy).ceil() as usize + 1);
        if bw == 0 || bh == 0 || bw * bh > 48 * 1024 * 1024 || bw > 65535 || bh > 65535 {
            return;
        }
        let fx = ((x0 - ox) * 4.0).round() as u8;
        let fy = ((y0 - oy) * 4.0).round() as u8;
        // The tile does not depend on where the mark is: moved to the origin for the key, so
        // dragging a mark with a shadow reuses its tile (ZK-130) — the sub-pixel offset below
        // tells the placements apart.
        let mut hasher = std::hash::DefaultHasher::new();
        let mut at_origin = obj.clone();
        at_origin.id = 0;
        at_origin.translate(-b.x, -b.y);
        at_origin.hash(&mut hasher);
        doc.counter_number(index).hash(&mut hasher);
        (bw, bh).hash(&mut hasher);
        let key = (hasher.finish(), (scale * 1000.0).round() as i64, fx, fy);
        let tile = match self.fx_cache.get(&key) {
            Some(p) => p.clone(),
            None => {
                let local = Affine::translate((-ox, -oy)) * t;
                let alpha =
                    self.render_alpha_tile(doc, index, obj, local, scale, bw as u16, bh as u16);
                let mut out = vec![
                    PremulRgba8 {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 0
                    };
                    bw * bh
                ];
                for (p, shadow) in [(sh, true), (gl, false)] {
                    if p.alpha == 0 {
                        continue;
                    }
                    let off = (p.off * scale + 0.5).floor() as usize;
                    let rad = (p.blur * scale + 0.5).floor() as usize;
                    let mut m = vec![0u8; bw * bh];
                    for y in off..bh {
                        for x in off..bw {
                            m[y * bw + x] = alpha[(y - off) * bw + (x - off)];
                        }
                    }
                    if rad > 0 {
                        hide::box_blur_alpha(&mut m, bw, bh, rad);
                    }
                    for (dst, &a) in out.iter_mut().zip(&m) {
                        if a == 0 {
                            continue;
                        }
                        let a = ((a as u32 * p.boost / 100).min(255) * p.alpha / 255) as u8;
                        let ia = 255 - a as u32;
                        let over = |d: u8, s: u8| (s as u32 + (d as u32 * ia + 127) / 255) as u8;
                        let s = if shadow { 0 } else { a };
                        *dst = PremulRgba8 {
                            r: over(dst.r, s),
                            g: over(dst.g, s),
                            b: over(dst.b, s),
                            a: over(dst.a, a),
                        };
                    }
                }
                let p = Arc::new(Pixmap::from_parts_with_opacity(
                    out, bw as u16, bh as u16, true,
                ));
                self.fx_cache.insert(key, p.clone());
                p
            }
        };
        self.ctx.set_transform(Affine::IDENTITY);
        self.draw_pixmap(
            tile,
            Rect::new(ox, oy, ox + bw as f64, oy + bh as f64),
            ImageQuality::Low,
            1.0,
        );
    }

    /// Alpha channel of the mark drawn alone with `local` as its transform.
    #[allow(clippy::too_many_arguments)]
    fn render_alpha_tile(
        &mut self,
        doc: &Document,
        index: usize,
        obj: &Object,
        local: Affine,
        scale: f64,
        w: u16,
        h: u16,
    ) -> Vec<u8> {
        self.fx_ctx.reset_and_resize(w, h);
        std::mem::swap(&mut self.ctx, &mut self.fx_ctx);
        self.ctx.set_transform(local);
        self.draw_raw(doc, index, obj, local, scale);
        self.ctx.flush();
        let mut pix = Pixmap::new(w, h);
        self.ctx.render(pix.as_mut(), &mut self.res);
        std::mem::swap(&mut self.ctx, &mut self.fx_ctx);
        pix.data().iter().map(|p| p.a).collect()
    }

    // ---- the mark itself (LH `EdDrawObjectRaw`), drawn with `self.ctx`'s current transform.

    fn draw_raw(&mut self, doc: &Document, index: usize, obj: &Object, t: Affine, scale: f64) {
        let b = obj.bounds();
        let rect = irect(b);
        let st = &obj.style;
        let alpha = st.alpha as f32 / 100.0;
        let pw = (st.thick as f64).max(1.0 / scale);
        let half = pw / 2.0;

        match &obj.data {
            Data::Rect => {
                let stroke = !st.no_main;
                let inset = if stroke { half } else { 0.0 };
                let radius = corner_radius(st, rect);
                if let Some(c2) = st.color2 {
                    self.ctx.set_paint(rgba(c2, st.alpha2 as f32 / 100.0));
                    self.ctx
                        .fill_path(&round_rect(rect.inset(inset), radius - inset).to_path(0.1));
                }
                if stroke {
                    self.ctx.set_stroke(stroke_for(st, pw));
                    self.ctx.set_paint(rgba(st.color, alpha));
                    self.ctx
                        .stroke_path(&round_rect(rect.inset(half), radius - half).to_path(0.1));
                } else {
                    self.ctx.set_paint(rgba(st.color, alpha));
                    self.ctx.fill_path(&round_rect(rect, radius).to_path(0.1));
                }
            }
            Data::Ellipse => {
                let stroke = !st.no_main;
                let inset = if stroke { half } else { 0.0 };
                if let Some(c2) = st.color2 {
                    self.ctx.set_paint(rgba(c2, st.alpha2 as f32 / 100.0));
                    self.ctx
                        .fill_path(&Ellipse::from_rect(rect.inset(inset)).to_path(0.1));
                }
                if stroke {
                    self.ctx.set_stroke(stroke_for(st, pw));
                    self.ctx.set_paint(rgba(st.color, alpha));
                    self.ctx
                        .stroke_path(&Ellipse::from_rect(rect.inset(half)).to_path(0.1));
                } else {
                    self.ctx.set_paint(rgba(st.color, alpha));
                    self.ctx.fill_path(&Ellipse::from_rect(rect).to_path(0.1));
                }
            }
            Data::Line {
                head_front,
                head_back,
                ..
            } => {
                let r = obj.rect;
                let p0 = Point::new(r.x as f64, r.y as f64);
                let p1 = Point::new((r.x + r.w) as f64, (r.y + r.h) as f64);
                let v = p1 - p0;
                let len = v.hypot();
                if len < 0.5 {
                    return;
                }
                let dir = v / len;
                let hl = head_len(obj);
                // The line is shortened by the part the head covers (LH: `hl * 0.85`).
                let back_e = if *head_front != Head::None {
                    (hl * 0.85).min(len * 0.5)
                } else {
                    0.0
                };
                let back_s = if *head_back != Head::None {
                    (hl * 0.85).min(len * 0.5)
                } else {
                    0.0
                };
                let mut stroke = stroke_for(st, pw);
                if st.dash == Dash::Solid {
                    stroke = stroke.with_caps(Cap::Round);
                }
                self.ctx.set_stroke(stroke);
                self.ctx.set_paint(rgba(st.color, alpha));
                self.ctx
                    .stroke_path(&Line::new(p0 + dir * back_s, p1 - dir * back_e).to_path(0.1));
                if *head_front != Head::None {
                    self.draw_head(p1, dir, hl, pw, *head_front);
                }
                if *head_back != Head::None {
                    self.draw_head(p0, -dir, hl, pw, *head_back);
                }
            }
            Data::Pen {
                points,
                head_front,
                head_back,
            } => {
                self.ctx.set_paint(rgba(st.color, alpha));
                if points.len() >= 2 && (*head_front != Head::None || *head_back != Head::None) {
                    self.draw_pen_with_heads(obj, points, *head_front, *head_back, pw);
                    return;
                }
                if points.len() == 1 {
                    let (x, y) = (points[0].0 as f64, points[0].1 as f64);
                    self.ctx
                        .fill_path(&Circle::new(Point::new(x, y), half).to_path(0.1));
                } else if points.len() >= 2 {
                    // A curve through the trail, not a polyline: the raw mouse trail is angular
                    // (LH `DrawCurve` with tension 0.3).
                    let pts: Vec<Point> = points
                        .iter()
                        .map(|&(x, y)| Point::new(x as f64, y as f64))
                        .collect();
                    self.ctx
                        .set_stroke(Stroke::new(pw).with_caps(Cap::Round).with_join(Join::Round));
                    self.ctx.stroke_path(&cardinal_spline(&pts, 0.3));
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
                    // A playing video (ZK-94): a hatched plate says «hidden here» — no live blur.
                    HideMode::Blur | HideMode::Pixelate if self.plain_effects => {
                        self.ctx.set_paint(rgba(Rgb::new(78, 82, 92), alpha));
                        self.ctx.fill_rect(&rect);
                        self.ctx.push_clip_layer(&rect.to_path(0.1));
                        self.ctx.set_stroke(Stroke::new(3.0));
                        self.ctx.set_paint(rgba(Rgb::new(120, 126, 138), alpha));
                        let (w, h) = (rect.width(), rect.height());
                        let mut d = -h;
                        while d < w {
                            let line = znimok_render_line(
                                Point::new(rect.x0 + d, rect.y1),
                                Point::new(rect.x0 + d + h, rect.y0),
                            );
                            self.ctx.stroke_path(&line);
                            d += 12.0;
                        }
                        self.ctx.pop_layer();
                    }
                    HideMode::Plate => {
                        self.ctx.set_paint(rgba(st.color, alpha));
                        self.ctx.fill_rect(&rect);
                    }
                    HideMode::Blur | HideMode::Pixelate => {
                        let key = (
                            region,
                            *mode,
                            *strength,
                            develop::key(doc.source(), &doc.recipe) as usize,
                            self.below_key(doc, index, region).1,
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
                            // Pixels stay pixels when scaled (LH: no smoothing for pixelate).
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
                // Over a playing video there are no pixels below: a translucent plate (ZK-94).
                if self.plain_effects {
                    self.ctx.set_paint(rgba(st.color, alpha * 0.45));
                    self.ctx.fill_rect(&rect);
                } else {
                    self.ctx
                        .push_blend_layer(BlendMode::new(Mix::Multiply, Compose::SrcOver));
                    self.ctx.set_paint(rgba(st.color, alpha));
                    self.ctx.fill_rect(&rect);
                    self.ctx.pop_layer();
                }
            }
            Data::Counter { shape, .. } => {
                let n = doc.counter_number(index).unwrap_or(0);
                let w = rect.width();
                // Thin light rim slightly inside: without it the mark is lost on a mark of the
                // same colour (LH CAPS-68).
                let bw = w / 16.0;
                let inset = bw / 2.0;
                self.ctx.set_paint(rgba(st.color, alpha));
                self.ctx.fill_path(&counter_path(rect, *shape));
                self.ctx.set_stroke(Stroke::new(bw).with_join(Join::Round));
                self.ctx.set_paint(rgba(Rgb::WHITE, alpha));
                self.ctx
                    .stroke_path(&counter_path(rect.inset(inset), *shape));
                let digit = st.color2.unwrap_or_else(|| on_color(st.color));
                let label = n.to_string();
                // The number grows with the counter (ZK-167): from its width, not a fixed size.
                let fs = if *shape == CounterShape::Pin {
                    w * 0.42
                } else {
                    w * 0.52
                };
                let spec = text::TextSpec {
                    text: &label,
                    size_px: fs.floor().max(4.0) as f32,
                    bold: true,
                    italic: false,
                    align: Align::Center,
                    box_w: Some(w as f32),
                };
                let head = if *shape == CounterShape::Pin {
                    Point::new(rect.x0 + w / 2.0, rect.y0 + w * 0.37)
                } else {
                    rect.center()
                };
                // The number stays upright however the counter turns (ZK-166): turned back
                // about its own centre, it still sits where the turned head is.
                let upright = obj.kind().can_rotate() && obj.rot != 0;
                if upright {
                    self.ctx.set_transform(
                        t * Affine::rotate_about(-(obj.rot as f64).to_radians(), head),
                    );
                }
                text::draw_text_centered(
                    &mut self.ctx,
                    &mut self.res,
                    &mut self.fonts,
                    &mut self.layouts,
                    &spec,
                    head,
                    rgba(digit, 1.0),
                    alpha,
                );
                if upright {
                    self.ctx.set_transform(t);
                }
            }
            Data::Stamp { id } => {
                if *id >= 100 {
                    let s = reference::emoji_for(*id);
                    let spec = text::TextSpec {
                        text: s,
                        size_px: rect.width().max(4.0) as f32,
                        bold: false,
                        italic: false,
                        align: Align::Center,
                        box_w: Some(rect.width() as f32 * 1.5),
                    };
                    text::draw_text_centered(
                        &mut self.ctx,
                        &mut self.res,
                        &mut self.fonts,
                        &mut self.layouts,
                        &spec,
                        rect.center(),
                        rgba(st.color, 1.0),
                        alpha,
                    );
                } else {
                    self.draw_stamp(*id, rect.origin(), rect.width(), rgba(st.color, alpha));
                }
            }
            Data::Image { bank } => {
                if doc.banks.get(*bank as usize).is_some() {
                    let pix = self.bank_pixmap(doc, *bank as usize);
                    let radius = corner_radius(st, rect);
                    if radius > 0.0 {
                        self.ctx
                            .push_clip_layer(&round_rect(rect, radius).to_path(0.1));
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

    /// A pen trail with heads: each end points along the trail's last stretch about one head
    /// long (the raw last segment is a jitter of the mouse), and the trail stops short under a
    /// head as a line does.
    fn draw_pen_with_heads(
        &mut self,
        obj: &Object,
        points: &[(i32, i32)],
        front: Head,
        back: Head,
        pw: f64,
    ) {
        let mut pts: Vec<Point> = points
            .iter()
            .map(|&(x, y)| Point::new(x as f64, y as f64))
            .collect();
        pts.dedup();
        if pts.len() < 2 {
            return;
        }
        let hl = head_len(obj);
        let total: f64 = pts.windows(2).map(|w| (w[1] - w[0]).hypot()).sum();
        // Tip and direction at the end of `pts` (reversed for the start).
        let end = |pts: &[Point]| -> (Point, Vec2) {
            let tip = *pts.last().unwrap();
            let from = pts
                .iter()
                .rev()
                .find(|p| (tip - **p).hypot() >= hl)
                .copied()
                .unwrap_or(pts[0]);
            let v = tip - from;
            let len = v.hypot().max(1e-6);
            (tip, v / len)
        };
        // Cuts the trail back by `d` from its end.
        let cut = |pts: &mut Vec<Point>, d: f64| {
            let tip = *pts.last().unwrap();
            while pts.len() > 2 && (tip - pts[pts.len() - 2]).hypot() < d {
                pts.remove(pts.len() - 2);
            }
            let n = pts.len();
            let (a, b) = (pts[n - 2], pts[n - 1]);
            let seg = (b - a).hypot();
            if seg > 1e-6 {
                pts[n - 1] = b - (b - a) * (d.min(seg * 0.9) / seg);
            }
        };
        let short = (hl * 0.85).min(total * 0.4);
        let f = (front != Head::None).then(|| end(&pts));
        let mut rev: Vec<Point> = pts.iter().rev().copied().collect();
        let b = (back != Head::None).then(|| end(&rev));
        if f.is_some() {
            cut(&mut pts, short);
        }
        if b.is_some() {
            rev = pts.iter().rev().copied().collect();
            cut(&mut rev, short);
            pts = rev.into_iter().rev().collect();
        }
        self.ctx
            .set_stroke(Stroke::new(pw).with_caps(Cap::Round).with_join(Join::Round));
        self.ctx.stroke_path(&cardinal_spline(&pts, 0.3));
        if let Some((tip, dir)) = f {
            self.draw_head(tip, dir, hl, pw, front);
        }
        if let Some((tip, dir)) = b {
            self.draw_head(tip, dir, hl, pw, back);
        }
    }

    /// Arrow head at `tip` pointing along `dir` (LH `EdDrawHead`): filled triangle, open
    /// chevron of two strokes, or a dot centred on the tip.
    fn draw_head(&mut self, tip: Point, dir: Vec2, len: f64, pw: f64, head: Head) {
        let n = Vec2::new(-dir.y, dir.x);
        let half_w = len * 0.42;
        match head {
            Head::None => {}
            Head::Dot => {
                self.ctx
                    .fill_path(&Circle::new(tip, len * 0.36).to_path(0.1));
            }
            Head::Chevron => {
                let base = tip - dir * len;
                let mut p = BezPath::new();
                p.move_to(base + n * half_w);
                p.line_to(tip);
                p.line_to(base - n * half_w);
                self.ctx
                    .set_stroke(Stroke::new(pw).with_caps(Cap::Butt).with_join(Join::Miter));
                self.ctx.stroke_path(&p);
            }
            Head::Triangle => {
                let base = tip - dir * len;
                let mut p = BezPath::new();
                p.move_to(tip);
                p.line_to(base + n * half_w);
                p.line_to(base - n * half_w);
                p.close_path();
                self.ctx.fill_path(&p);
            }
        }
    }

    /// Vector stamps in a 20×20 grid scaled to `side` (LH `EdStampShape`).
    fn draw_stamp(&mut self, id: u32, origin: Point, side: f64, paint: AlphaColor<Srgb>) {
        let saved = *self.ctx.transform();
        let k = side / 20.0;
        self.ctx
            .set_transform(saved * Affine::translate(origin.to_vec2()) * Affine::scale(k));
        self.ctx.set_paint(paint);
        self.ctx.set_stroke(
            Stroke::new(2.6)
                .with_caps(Cap::Round)
                .with_join(Join::Round),
        );
        let pt = |x: f64, y: f64| Point::new(x, y);
        let dot = |cx: f64, cy: f64, d: f64| {
            Circle::new(pt(cx + d / 2.0, cy + d / 2.0), d / 2.0).to_path(0.05)
        };
        match id {
            0 => {
                let mut p = BezPath::new();
                p.move_to(pt(3.4, 10.8));
                p.line_to(pt(8.0, 15.6));
                p.line_to(pt(16.6, 4.8));
                self.ctx.stroke_path(&p);
            }
            1 => {
                let mut p = BezPath::new();
                p.move_to(pt(4.2, 4.2));
                p.line_to(pt(15.8, 15.8));
                p.move_to(pt(15.8, 4.2));
                p.line_to(pt(4.2, 15.8));
                self.ctx.stroke_path(&p);
            }
            2 => {
                // Question mark: hook arc, smooth stem, separate dot.
                let mut p = KArc::new(
                    pt(10.0, 6.8),
                    Vec2::new(4.4, 4.4),
                    180f64.to_radians(),
                    200f64.to_radians(),
                    0.0,
                )
                .to_path(0.05);
                p.move_to(pt(14.14, 8.3));
                p.curve_to(pt(13.2, 11.2), pt(10.0, 11.0), pt(10.0, 13.6));
                self.ctx.stroke_path(&p);
                self.ctx.fill_path(&dot(8.7, 15.4, 2.6));
            }
            3 => {
                self.ctx
                    .stroke_path(&Line::new(pt(10.0, 3.0), pt(10.0, 12.6)).to_path(0.05));
                self.ctx.fill_path(&dot(8.7, 15.0, 2.6));
            }
            4 => {
                let mut p = BezPath::new();
                for i in 0..10 {
                    let a = -std::f64::consts::FRAC_PI_2 + i as f64 * std::f64::consts::PI / 5.0;
                    let r = if i % 2 == 0 { 8.6 } else { 3.6 };
                    let q = pt(10.0 + a.cos() * r, 10.0 + a.sin() * r);
                    if i == 0 { p.move_to(q) } else { p.line_to(q) }
                }
                p.close_path();
                self.ctx.fill_path(&p);
            }
            _ => {
                // Warning triangle.
                let mut p = BezPath::new();
                p.move_to(pt(10.0, 2.6));
                p.line_to(pt(18.4, 16.8));
                p.line_to(pt(1.6, 16.8));
                p.close_path();
                p.move_to(pt(10.0, 7.6));
                p.line_to(pt(10.0, 12.0));
                self.ctx.stroke_path(&p);
                self.ctx.fill_path(&dot(8.9, 13.6, 2.2));
            }
        }
        self.ctx.set_transform(saved);
    }

    /// Where a mark draws, for the question «is it under this Hide»: its bounds, or for a text
    /// laid out by itself (no box) the measured text around its origin, generously.
    fn covered_box(&mut self, o: &Object) -> IRect {
        let b = o.bounds();
        if let Data::Text {
            text,
            size,
            bold,
            italic,
            box_w,
            ..
        } = &o.data
            && (b.w == 0 || b.h == 0)
        {
            let (w, h) = self.measure_text(text, *size, *bold, *italic, *box_w);
            let (w, h) = (w.ceil() as i32 + 2, h.ceil() as i32 + 2);
            // The alignment may put it left of the origin too.
            return IRect::new(b.x - w, b.y, 3 * w, h);
        }
        b
    }

    /// Whether any visible mark below object `index` lies in `region`, and what those marks
    /// look like (for the Hide tile's key: a caption edited or a mark moved in or out under the
    /// Hide makes a new tile).
    fn below_key(&mut self, doc: &Document, index: usize, region: IRect) -> (bool, u64) {
        let mut h = std::hash::DefaultHasher::new();
        let mut any = false;
        for (i, o) in doc.objects[..index.min(doc.objects.len())]
            .iter()
            .enumerate()
        {
            if !o.hidden && doc.live(o) && meets(self.covered_box(o), region) {
                any = true;
                i.hash(&mut h);
                o.hash(&mut h);
                doc.counter_number(i).hash(&mut h);
            }
        }
        (any, h.finish())
    }

    /// Pixels of everything below object `index` inside `region`, as a straight-alpha raster, in
    /// screenshot resolution (ZK-50): the developed picture, and the marks under the Hide drawn
    /// on it — a caption under a blur is blurred too (LH: the effect works on «what is below»).
    fn below_pixels(&mut self, doc: &Document, index: usize, region: IRect) -> Option<Raster> {
        if self.below_key(doc, index, region).0 {
            let (iw, ih) = doc.image_size();
            let x0 = region.x.max(0);
            let y0 = region.y.max(0);
            let x1 = region.right().min(iw as i32);
            let y1 = region.bottom().min(ih as i32);
            if x1 <= x0 || y1 <= y0 || x1 - x0 > 16384 || y1 - y0 > 16384 {
                return None;
            }
            let mut under = doc.clone();
            under.objects.truncate(index);
            // The whole picture: a Hide outside the crop still hides what it covers.
            under.crop = None;
            let view = View {
                scale: 1.0,
                origin: Point::new(x0 as f64, y0 as f64),
                width: (x1 - x0) as u16,
                height: (y1 - y0) as u16,
            };
            let settings = self.settings;
            let sub = self
                .below
                .get_or_insert_with(|| Box::new(Renderer::with_settings(settings)));
            let mut pix = Pixmap::new(1, 1);
            sub.render(&under, view, &mut pix);
            return Some(Raster::new(
                pix.width() as u32,
                pix.height() as u32,
                pixmap_to_rgba(&pix),
            ));
        }
        let (src, _) = self.developed(doc);
        let src = &*src;
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
        Some(Raster::new(w, h, rgba))
    }
}

/// The rectangles overlap.
fn meets(a: IRect, b: IRect) -> bool {
    let (a, b) = (a.normalized(), b.normalized());
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}

fn fx_on(o: &Object) -> bool {
    (o.style.shadow != Effect::None || o.style.glow != Effect::None) && o.kind().fx_allowed()
}

fn rotation(o: &Object) -> Affine {
    if o.kind().can_rotate() && o.rot != 0 {
        Affine::rotate_about((o.rot as f64).to_radians(), irect(o.bounds()).center())
    } else {
        Affine::IDENTITY
    }
}

/// Head length in screenshot pixels (LH `EdHeadLen`): size step 0..2, thickness adds a little.
/// The layout request of a text mark, as `draw_raw` makes it.
fn text_spec(obj: &Object) -> Option<text::TextSpec<'_>> {
    let Data::Text {
        text,
        size,
        bold,
        italic,
        align,
        box_w,
    } = &obj.data
    else {
        return None;
    };
    Some(text::TextSpec {
        text,
        size_px: *size as f32,
        bold: *bold,
        italic: *italic,
        align: *align,
        box_w: if *box_w > 0 {
            Some(*box_w as f32)
        } else {
            None
        },
    })
}

pub fn head_len(o: &Object) -> f64 {
    let size = match &o.data {
        Data::Line { head_size, .. } => *head_size,
        // A pen trail has no size step: the middle one.
        Data::Pen { .. } => 1,
        _ => return 0.0,
    };
    const MUL: [f64; 3] = [3.0, 4.5, 6.5];
    let m = MUL[size.min(2) as usize];
    (o.style.thick as f64 * m + 4.0).max(6.0)
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

fn round_rect(rect: Rect, radius: f64) -> RoundedRect {
    let lim = rect.width().min(rect.height()) / 2.0;
    RoundedRect::from_rect(rect, radius.clamp(0.0, lim.max(0.0)))
}

/// Outline pen (LH `EdApplyDash`): GDI+ dash = 3:1 and dash-dot = 3:1:1:1 in pen widths, flat
/// caps so dashes stay dashes; solid outlines use round joins.
fn stroke_for(st: &Style, pw: f64) -> Stroke {
    let s = Stroke::new(pw).with_join(Join::Round).with_caps(Cap::Butt);
    match st.dash {
        Dash::Solid => s,
        Dash::Dashed => s.with_dashes(0.0, [pw * 3.0, pw]),
        Dash::DashDot => s.with_dashes(0.0, [pw * 3.0, pw, pw, pw]),
    }
}

/// Digit colour for a counter without an explicit second colour (LH `EdOnColor`).
pub fn on_color(c: Rgb) -> Rgb {
    let lum = (c.r as u32 * 299 + c.g as u32 * 587 + c.b as u32 * 114) / 1000;
    if lum > 140 {
        Rgb::new(24, 24, 28)
    } else {
        Rgb::WHITE
    }
}

/// Counter body (LH `EdCounterPath`): circle, rounded box (r = 0.28 w) or a pin whose head is
/// a circle of radius 0.37 w and whose tip is the bottom centre of the box.
fn counter_path(r: Rect, shape: CounterShape) -> BezPath {
    let (x, y, w, h) = (r.x0, r.y0, r.width(), r.height());
    match shape {
        CounterShape::Circle => Ellipse::from_rect(r).to_path(0.1),
        CounterShape::RoundedBox => round_rect(r, w * 0.28).to_path(0.1),
        CounterShape::Pin => {
            let rr = w * 0.37;
            let (hx, hy) = (x + w / 2.0, y + rr);
            let tip_y = y + h;
            let dist = (tip_y - hy).max(rr);
            let th = (rr / dist).clamp(-1.0, 1.0).acos();
            let a0 = std::f64::consts::FRAC_PI_2 + th;
            let mut p = BezPath::new();
            p.move_to(Point::new(hx, tip_y));
            p.line_to(Point::new(hx + rr * a0.cos(), hy + rr * a0.sin()));
            let arc = KArc::new(
                Point::new(hx, hy),
                Vec2::new(rr, rr),
                a0,
                std::f64::consts::TAU - 2.0 * th,
                0.0,
            );
            for el in arc.append_iter(0.1) {
                p.push(el);
            }
            p.close_path();
            p
        }
    }
}

/// Cardinal spline through `pts` (GDI+ `DrawCurve` semantics): control points are
/// `p ± (next - prev) * tension / 3`.
fn cardinal_spline(pts: &[Point], tension: f64) -> BezPath {
    let mut p = BezPath::new();
    p.move_to(pts[0]);
    if pts.len() == 2 {
        p.line_to(pts[1]);
        return p;
    }
    let k = tension / 3.0;
    let n = pts.len();
    for i in 0..n - 1 {
        let prev = if i == 0 { pts[0] } else { pts[i - 1] };
        let next = if i + 2 < n { pts[i + 2] } else { pts[n - 1] };
        let c1 = pts[i] + (pts[i + 1] - prev) * k;
        let c2 = pts[i + 1] - (next - pts[i]) * k;
        p.curve_to(c1, c2, pts[i + 1]);
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
        *dst = PremulRgba8 {
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

/// A straight segment as a path (the hatching of a Hide over a playing video).
fn znimok_render_line(a: Point, b: Point) -> BezPath {
    let mut p = BezPath::new();
    p.move_to(a);
    p.line_to(b);
    p
}

#[cfg(test)]
mod tests {

    /// ZK-204: a picture mark and a Hide at a see-through opacity rendered without a panic
    /// (vello_common 0.2 has no alpha on image paints), half-way between the picture and the mark.
    #[test]
    fn translucent_images_render() {
        use znimok_core::{Data, Document, HideMode, IRect, Object, Raster, Rgb};
        let mut d = Document::from_raster("t", Raster::solid(100, 60, Rgb::new(0, 0, 0)));
        let bank = d.add_bank(Raster::solid(20, 20, Rgb::new(255, 255, 255)));
        let mut img = Object::new(IRect::new(10, 10, 20, 20), Data::Image { bank });
        img.style.alpha = 50;
        d.push(img);
        let mut hide = Object::new(
            IRect::new(50, 10, 30, 30),
            Data::Hide {
                mode: HideMode::Blur,
                strength: 50,
            },
        );
        hide.style.alpha = 40;
        d.push(hide);
        let mut r = Renderer::deterministic();
        let mut out = Pixmap::new(1, 1);
        r.render(&d, View::one_to_one(&d), &mut out);
        let px = out.data_as_u8_slice();
        let g = px[(20 * 100 + 20) * 4] as i32;
        assert!(
            (g - 128).abs() <= 8,
            "half-transparent white over black: {g}"
        );
    }

    /// ZK-94: a mark outside its time is not drawn; with the plain effects a Hide still covers
    /// its box (a plate) where the picture is left out.
    #[test]
    fn marks_follow_the_frame_shown() {
        use znimok_core::{Data, Document, HideMode, IRect, Object, Raster, Rgb, Timeline};
        let mut d = Document::from_raster("v", Raster::solid(200, 100, Rgb::new(10, 200, 10)));
        let id = d.objects.len();
        d.push(Object::new(IRect::new(20, 20, 60, 40), Data::Rect));
        let rect_id = d.objects[id].id;
        let mut t = Timeline::whole(100);
        t.marks.insert(rect_id, (10, 20));
        d.timeline = Some(t);
        let view = View::one_to_one(&d);
        let mut r = Renderer::deterministic();
        let mut with = Pixmap::new(1, 1);
        let mut without = Pixmap::new(1, 1);
        d.shown_frame = Some(15);
        r.render(&d, view, &mut with);
        d.shown_frame = Some(50);
        r.render(&d, view, &mut without);
        assert_ne!(
            with.data_as_u8_slice(),
            without.data_as_u8_slice(),
            "drawn on frame 15"
        );
        let mut plain = Document::from_raster("v", Raster::solid(200, 100, Rgb::new(10, 200, 10)));
        plain.push(Object::new(
            IRect::new(20, 20, 60, 40),
            Data::Hide {
                mode: HideMode::Blur,
                strength: 50,
            },
        ));
        r.set_picture(false);
        r.set_plain_effects(true);
        let mut out = Pixmap::new(1, 1);
        r.render(&plain, view, &mut out);
        let px = |x: usize, y: usize| out.data_as_u8_slice()[(y * 200 + x) * 4 + 3];
        assert_eq!(px(5, 5), 0, "the picture is left out");
        assert!(px(50, 40) > 200, "the Hide is a plate over the video");
    }
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

    #[test]
    fn spline_passes_through_points() {
        let pts = [
            Point::new(0.0, 0.0),
            Point::new(10.0, 5.0),
            Point::new(20.0, 0.0),
        ];
        let p = cardinal_spline(&pts, 0.3);
        assert_eq!(p.elements().len(), 3);
    }
}
