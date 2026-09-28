//! Text marks: Parley lays the text out, vello_cpu fills the glyphs. The outline of a text mark
//! is the glyphs' own contour stroked in the second colour under the text (ZK-49; LH drew twelve
//! shifted copies — same thickness, `size/11` clamped to 1–8 px, but round and even all round).

use std::borrow::Cow;

use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontStyle, FontWeight, Layout,
    LayoutContext, PositionedLayoutItem, StyleProperty,
};
use vello_cpu::color::{AlphaColor, Srgb};
use vello_cpu::kurbo::{Affine, Join, Point, Rect, Stroke};
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
        let saved = ctx.stroke().clone();
        // A stroke of 2r centred on the contour reaches r outside it; the fill covers the inside.
        ctx.set_stroke(Stroke::new(2.0 * r).with_join(Join::Round));
        glyphs(ctx, res, &l, origin, paint, true);
        ctx.set_stroke(saved);
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
    glyphs(ctx, res, l, origin, paint, false);
}

/// Where the text cursor and the selection of a text being edited are (ZK-49), relative to the
/// block's top-left corner: the caret as a thin box, the selection as one box per line piece.
pub fn caret(
    fonts: &mut FontContext,
    layouts: &mut LayoutContext<Brush>,
    spec: &TextSpec<'_>,
    cursor: usize,
    anchor: usize,
) -> (Rect, Vec<Rect>) {
    let l = layout(fonts, layouts, spec);
    let len = spec.text.len();
    let (cursor, anchor) = (cursor.min(len), anchor.min(len));
    let focus = parley::Cursor::from_byte_index(&l, cursor, parley::Affinity::Downstream);
    let bb = focus.geometry(&l, 1.0);
    let caret = Rect::new(bb.x0, bb.y0, bb.x1, bb.y1);
    let sel = if cursor == anchor {
        Vec::new()
    } else {
        let a = parley::Cursor::from_byte_index(&l, anchor, parley::Affinity::Downstream);
        parley::Selection::new(a, focus)
            .geometry(&l)
            .into_iter()
            .map(|(b, _)| Rect::new(b.x0, b.y0, b.x1, b.y1))
            .collect()
    };
    (caret, sel)
}

/// The byte offset in the text nearest to a point relative to the block's top-left corner.
pub fn hit(
    fonts: &mut FontContext,
    layouts: &mut LayoutContext<Brush>,
    spec: &TextSpec<'_>,
    x: f32,
    y: f32,
) -> usize {
    let l = layout(fonts, layouts, spec);
    parley::Cursor::from_point(&l, x, y).index()
}

fn glyphs(
    ctx: &mut RenderContext,
    res: &mut Resources,
    l: &Layout<Brush>,
    origin: Point,
    paint: AlphaColor<Srgb>,
    stroke: bool,
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
            let run = ctx
                .glyph_run(res, r.font())
                .font_size(r.font_size())
                .normalized_coords(r.normalized_coords())
                .hint(false);
            if stroke {
                run.stroke_glyphs(glyphs.into_iter());
            } else {
                run.fill_glyphs(glyphs.into_iter());
            }
        }
    }
    ctx.set_transform(base);
}
