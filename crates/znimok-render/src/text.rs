//! Text marks: Parley lays the text out, vello_cpu fills the glyphs. The outline of a text mark
//! is, as in Little Helpers, twelve copies of the text around a circle (radius `size/11`, 1–8 px)
//! in the second colour, drawn before the text itself.

use std::borrow::Cow;

use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontStyle, FontWeight, Layout,
    LayoutContext, PositionedLayoutItem, StyleProperty,
};
use vello_cpu::color::{AlphaColor, Srgb};
use vello_cpu::kurbo::{Affine, Point};
use vello_cpu::{Glyph, RenderContext, Resources};
use znimok_core::Align;

/// Parley's per-run brush; we only need one paint per layout, so it is a unit.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Brush;

/// Family used for text marks until bundled fonts arrive (Phase 2): the platform UI face.
pub const FAMILY: &str = "Segoe UI, Helvetica Neue, sans-serif";

pub struct TextSpec<'a> {
    pub text: &'a str,
    pub size_px: f32,
    pub bold: bool,
    pub italic: bool,
    pub align: Align,
    /// Wrap width in document pixels; `None` = single line, width from the text.
    pub box_w: Option<f32>,
}

pub fn layout(
    fonts: &mut FontContext,
    layouts: &mut LayoutContext<Brush>,
    spec: &TextSpec<'_>,
) -> Layout<Brush> {
    let mut b = layouts.ranged_builder(fonts, spec.text, 1.0, true);
    b.push_default(FontFamily::Source(Cow::Borrowed(FAMILY)));
    b.push_default(StyleProperty::FontSize(spec.size_px.max(1.0)));
    b.push_default(StyleProperty::FontWeight(if spec.bold {
        FontWeight::BOLD
    } else {
        FontWeight::NORMAL
    }));
    b.push_default(StyleProperty::FontStyle(if spec.italic {
        FontStyle::Italic
    } else {
        FontStyle::Normal
    }));
    let mut layout = b.build(spec.text);
    layout.break_all_lines(spec.box_w);
    let align = match spec.align {
        Align::Left => Alignment::Left,
        Align::Center => Alignment::Center,
        Align::Right => Alignment::Right,
    };
    layout.align(align, AlignmentOptions::default());
    layout
}

/// Size of the laid-out block in document pixels (width, height).
pub fn measure(
    fonts: &mut FontContext,
    layouts: &mut LayoutContext<Brush>,
    spec: &TextSpec<'_>,
) -> (f32, f32) {
    let l = layout(fonts, layouts, spec);
    // +2 px so a line never wraps from rounding when the box is later set to this width.
    (l.width().ceil() + 2.0, l.height().ceil())
}

#[allow(clippy::too_many_arguments)]
pub fn draw_text(
    ctx: &mut RenderContext,
    res: &mut Resources,
    fonts: &mut FontContext,
    layouts: &mut LayoutContext<Brush>,
    spec: &TextSpec<'_>,
    origin: Point,
    color: AlphaColor<Srgb>,
    outline: Option<(znimok_core::Rgb, f32)>,
    alpha: f32,
) {
    let l = layout(fonts, layouts, spec);
    if alpha < 1.0 {
        ctx.push_opacity_layer(alpha);
    }
    if let Some((c, a2)) = outline {
        let r = (spec.size_px / 11.0).clamp(1.0, 8.0) as f64;
        let paint = AlphaColor::<Srgb>::from_rgba8(c.r, c.g, c.b, 255);
        if a2 < 1.0 {
            ctx.push_opacity_layer(a2);
        }
        for i in 0..12 {
            let a = i as f64 * std::f64::consts::TAU / 12.0;
            fill_layout(ctx, res, &l, origin + (r * a.cos(), r * a.sin()), paint);
        }
        if a2 < 1.0 {
            ctx.pop_layer();
        }
    }
    fill_layout(ctx, res, &l, origin, color);
    if alpha < 1.0 {
        ctx.pop_layer();
    }
}

/// Draws the block centred on `center` (counter digits, emoji stamps).
#[allow(clippy::too_many_arguments)]
pub fn draw_text_centered(
    ctx: &mut RenderContext,
    res: &mut Resources,
    fonts: &mut FontContext,
    layouts: &mut LayoutContext<Brush>,
    spec: &TextSpec<'_>,
    center: Point,
    color: AlphaColor<Srgb>,
    alpha: f32,
) {
    let l = layout(fonts, layouts, spec);
    let w = spec.box_w.unwrap_or(l.width()) as f64;
    let origin = Point::new(center.x - w / 2.0, center.y - l.height() as f64 / 2.0);
    if alpha < 1.0 {
        ctx.push_opacity_layer(alpha);
    }
    fill_layout(ctx, res, &l, origin, color);
    if alpha < 1.0 {
        ctx.pop_layer();
    }
}

fn fill_layout(
    ctx: &mut RenderContext,
    res: &mut Resources,
    l: &Layout<Brush>,
    origin: Point,
    paint: AlphaColor<Srgb>,
) {
    let base = *ctx.transform();
    ctx.set_paint(paint);
    for line in l.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else {
                continue;
            };
            let r = run.run();
            let glyphs: Vec<Glyph> = run
                .positioned_glyphs()
                .map(|g| Glyph {
                    id: g.id,
                    x: g.x,
                    y: g.y,
                })
                .collect();
            ctx.set_transform(base * Affine::translate(origin.to_vec2()));
            ctx.glyph_run(res, r.font())
                .font_size(r.font_size())
                .normalized_coords(r.normalized_coords())
                .hint(false)
                .fill_glyphs(glyphs.into_iter());
        }
    }
    ctx.set_transform(base);
}
