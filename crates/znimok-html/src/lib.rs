//! One HTML file for a screenshot with annotations (ZK-66): opens offline in any browser, no
//! program needed. The basis of the developer report (v2).
//!
//! What goes in: the **rendered** picture (annotations and Hide already baked in, PNG as a data
//! URI), the list of annotations (kind, text, counter number, name) with their areas highlighted on
//! hover, and the document metadata. What never goes in: the original picture — it would reveal
//! what Hide covers — and local file paths.
//!
//! Safety of the page itself: every text is HTML-escaped; `Content-Security-Policy` allows only the
//! inline style and script of the page and `data:` images, nothing is fetched from anywhere.

use std::fmt::Write as _;

use znimok_core::{Data, Document, Kind};
use znimok_i18n::{Localizer, args};

/// Escape text for HTML content and attribute values.
pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut o = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            o.push(if i <= c.len() {
                T[((n >> shift) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    o
}

/// One row of the annotation list.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub label: String,
    /// Area in picture pixels (relative to the crop), for the hover highlight.
    pub area: (i32, i32, i32, i32),
}

fn kind_name(tr: &Localizer, k: Kind) -> String {
    tr.tr(match k {
        Kind::Rect => "tool-rect-name",
        Kind::Ellipse => "tool-ellipse-name",
        Kind::Line => "tool-line-name",
        Kind::Pen => "tool-pen-name",
        Kind::Text => "tool-text-name",
        Kind::Hide => "tool-hide-name",
        Kind::Mark => "tool-highlighter-name",
        Kind::Counter => "tool-counter-name",
        Kind::Stamp => "tool-stamp-name",
        Kind::Image => "tool-image-name",
    })
}

/// The visible annotations, top of the z-order first, as the Layers panel lists them.
pub fn entries(doc: &Document, tr: &Localizer) -> Vec<Entry> {
    let f = doc.frame();
    let mut out = Vec::new();
    for (i, o) in doc.objects.iter().enumerate().rev() {
        if o.hidden {
            continue;
        }
        let mut label = match &o.data {
            Data::Counter { .. } => doc
                .counter_number(i)
                .map(|n| tr.tr_args("counter-name", &args!(n = n)))
                .unwrap_or_else(|| kind_name(tr, o.kind())),
            // Only what is visible on the picture: a Hide mark never tells what it hides.
            Data::Text { text, .. } => format!("{} «{}»", kind_name(tr, Kind::Text), text.trim()),
            d => kind_name(tr, d.kind()),
        };
        if let Some(n) = o.name.as_deref().filter(|n| !n.trim().is_empty()) {
            label = format!("{label} — {n}");
        }
        let b = o.bounds();
        out.push(Entry {
            label,
            area: (b.x - f.x, b.y - f.y, b.w, b.h),
        });
    }
    out
}

/// The page. `png` is the rendered picture (crop, annotations baked in), `width`×`height` its size.
pub fn page(doc: &Document, png: &[u8], width: u32, height: u32, tr: &Localizer) -> String {
    let title = if doc.name.trim().is_empty() {
        tr.tr("doc-kind-screenshot")
    } else {
        doc.name.clone()
    };
    let entries = entries(doc, tr);
    let m = &doc.meta;
    let mut facts = String::new();
    let mut fact = |k: &str, v: &str| {
        if !v.trim().is_empty() {
            let _ = write!(facts, "<dt>{}</dt><dd>{}</dd>", esc(&tr.tr(k)), esc(v));
        }
    };
    if m.created_ms > 0
        && let Some(t) = chrono::DateTime::from_timestamp_millis(m.created_ms)
    {
        fact("meta-taken", &t.format("%Y-%m-%d %H:%M UTC").to_string());
    }
    fact("meta-author", &m.author);
    fact("meta-rights", &m.copyright);
    fact("meta-tags", &m.tags.join(", "));
    fact(
        "export-size",
        &tr.tr_args("common-size-by", &args!(width = width, height = height)),
    );
    let mut list = String::new();
    for (i, e) in entries.iter().enumerate() {
        let _ = write!(
            list,
            "<li data-i=\"{i}\" tabindex=\"0\"><span class=\"n\">{}</span>{}</li>",
            i + 1,
            esc(&e.label)
        );
    }
    let mut boxes = String::new();
    for (i, e) in entries.iter().enumerate() {
        let (x, y, w, h) = e.area;
        let pct = |v: i32, of: u32| format!("{:.3}%", f64::from(v) * 100.0 / f64::from(of.max(1)));
        let _ = write!(
            boxes,
            "<div class=\"box\" data-i=\"{i}\" style=\"left:{};top:{};width:{};height:{}\"></div>",
            pct(x, width),
            pct(y, height),
            pct(w.max(1), width),
            pct(h.max(1), height)
        );
    }
    let count = tr.tr_args("common-marks", &args!(count = entries.len() as i64));
    let desc = if m.description.trim().is_empty() {
        String::new()
    } else {
        format!("<p class=\"desc\">{}</p>", esc(&m.description))
    };
    let made = tr.tr_args(
        "html-made-with",
        &args!(version = env!("CARGO_PKG_VERSION")),
    );
    format!(
        r#"<!doctype html>
<html lang="{lang}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src data:; style-src 'unsafe-inline'; script-src 'unsafe-inline'">
<meta name="generator" content="Znimok {ver}">
<title>{title}</title>
<style>
:root{{--bg:#f4f5f8;--panel:#fff;--text:#14161a;--muted:#545b66;--line:#e2e5eb;--accent:#2f6fea}}
@media (prefers-color-scheme:dark){{:root{{--bg:#0f1115;--panel:#171a20;--text:#e8eaee;--muted:#a7adb8;--line:#22262e;--accent:#6aa1ff}}}}
*{{box-sizing:border-box}}body{{margin:0;font:14px/1.5 system-ui,-apple-system,"Segoe UI",sans-serif;background:var(--bg);color:var(--text)}}
header{{padding:16px 20px;border-bottom:1px solid var(--line);background:var(--panel)}}h1{{margin:0;font-size:18px}}.desc{{margin:6px 0 0;color:var(--muted)}}
main{{display:flex;gap:16px;padding:16px 20px;align-items:flex-start}}@media (max-width:800px){{main{{flex-direction:column}}}}
.view{{flex:1;min-width:0;overflow:auto;background:var(--panel);border:1px solid var(--line);border-radius:10px;padding:10px}}
.stage{{position:relative;display:inline-block;max-width:100%}}.stage.full{{max-width:none}}
.stage img{{display:block;max-width:100%;height:auto}}.stage.full img{{max-width:none}}
.box{{position:absolute;outline:3px solid var(--accent);outline-offset:2px;border-radius:3px;opacity:0;transition:opacity .12s;pointer-events:none}}.box.on{{opacity:1}}
aside{{width:300px;flex:none;background:var(--panel);border:1px solid var(--line);border-radius:10px;padding:12px 14px}}@media (max-width:800px){{aside{{width:100%}}}}
h2{{font-size:13px;margin:0 0 8px;color:var(--muted);font-weight:600}}ol{{list-style:none;margin:0 0 14px;padding:0}}
li{{padding:6px 8px;border-radius:6px;cursor:default;display:flex;gap:8px}}li:hover,li:focus{{background:var(--bg);outline:none}}
.n{{min-width:1.6em;color:var(--muted);font-variant-numeric:tabular-nums}}dl{{margin:0;display:grid;grid-template-columns:auto 1fr;gap:4px 12px}}dt{{color:var(--muted)}}dd{{margin:0;overflow-wrap:anywhere}}
.tools{{margin-bottom:8px;display:flex;gap:6px}}button{{font:inherit;color:inherit;background:var(--bg);border:1px solid var(--line);border-radius:6px;padding:3px 10px;cursor:pointer}}
footer{{padding:0 20px 16px;color:var(--muted);font-size:12px}}
</style>
</head>
<body>
<header><h1>{title}</h1>{desc}</header>
<main>
<section class="view">
<div class="tools"><button type="button" id="fit">{fit}</button><button type="button" id="full">{full}</button></div>
<div class="stage" id="stage"><img alt="{title}" src="data:image/png;base64,{png}" width="{w}" height="{h}">{boxes}</div>
</section>
<aside>
<h2>{count}</h2>
<ol id="list">{list}</ol>
<dl>{facts}</dl>
</aside>
</main>
<footer>{made}</footer>
<script>
(function(){{var s=document.getElementById('stage');
document.getElementById('fit').onclick=function(){{s.classList.remove('full')}};
document.getElementById('full').onclick=function(){{s.classList.add('full')}};
function on(i,v){{var b=s.querySelector('.box[data-i="'+i+'"]');if(b)b.classList.toggle('on',v)}}
Array.prototype.forEach.call(document.querySelectorAll('#list li'),function(li){{var i=li.getAttribute('data-i');
li.onmouseenter=li.onfocus=function(){{on(i,true)}};li.onmouseleave=li.onblur=function(){{on(i,false)}}}});}})();
</script>
</body>
</html>
"#,
        lang = tr.lang(),
        ver = env!("CARGO_PKG_VERSION"),
        title = esc(&title),
        desc = desc,
        fit = esc(&tr.tr("canvas-fit")),
        full = esc(&tr.tr("canvas-zoom-100")),
        png = base64(png),
        w = width,
        h = height,
        boxes = boxes,
        count = esc(&count),
        list = list,
        facts = facts,
        made = esc(&made),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use znimok_core::{IRect, Object, Raster};

    fn doc() -> Document {
        let mut d = Document::from_raster(
            "Налаштування <друку>",
            Raster::new(200, 100, vec![255; 200 * 100 * 4]),
        );
        d.meta.author = "Vadym & Co".into();
        d.meta.tags = vec!["друк".into(), "баг".into()];
        d.meta.created_ms = 1_790_000_000_000;
        d.crop = Some(IRect::new(10, 10, 180, 80));
        d.objects
            .push(Object::new(IRect::new(20, 20, 40, 30), Data::Rect));
        let text = Data::Text {
            text: "<script>alert(1)</script>".into(),
            size: 24,
            bold: false,
            italic: false,
            align: Default::default(),
            box_w: 0,
        };
        let mut t = Object::new(IRect::new(60, 20, 80, 20), text);
        t.name = Some("Кнопка «Далі»".into());
        d.objects.push(t);
        let mut h = Object::new(IRect::new(100, 50, 50, 20), Data::Rect);
        h.hidden = true;
        d.objects.push(h);
        d
    }

    #[test]
    fn base64_matches_known_values() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn page_is_escaped_offline_and_lists_visible_marks() {
        let tr = Localizer::new("uk");
        let html = page(&doc(), &[137, 80, 78, 71], 180, 80, &tr);
        assert!(html.contains("<title>Налаштування &lt;друку&gt;</title>"));
        assert!(
            !html.contains("<script>alert(1)"),
            "user text must be escaped"
        );
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(html.contains("Vadym &amp; Co"));
        assert!(html.contains("Content-Security-Policy") && html.contains("default-src 'none'"));
        assert!(
            !html.contains("http://") && !html.contains("src=\"https"),
            "nothing is fetched"
        );
        // Two visible marks (the hidden one is left out), top of z-order first.
        let e = entries(&doc(), &tr);
        assert_eq!(e.len(), 2);
        assert!(e[0].label.starts_with("Напис") && e[0].label.ends_with("— Кнопка «Далі»"));
        assert_eq!(e[1].area, (10, 10, 40, 30), "area relative to the crop");
        assert!(html.contains("2 позначки"));
        assert!(html.contains("data:image/png;base64,iVBORw=="));
    }

    #[test]
    fn english_page() {
        let html = page(&doc(), b"x", 180, 80, &Localizer::new("en"));
        assert!(
            html.contains("<html lang=\"en\">")
                && html.contains("2 annotations")
                && html.contains("Made with Znimok")
        );
    }
}
