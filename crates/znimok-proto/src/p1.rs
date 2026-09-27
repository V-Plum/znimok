//! P1 — canvas prototype: document → vello_cpu pixmap → wgpu texture shared with Slint →
//! `Image`. Measures event-to-frame latency while dragging. See PLAN §6 for the criteria.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::wgpu_30::wgpu;
use slint::{ComponentHandle, SharedString};
use znimok_core::*;
use znimok_render::vello_cpu::Pixmap;
use znimok_render::vello_cpu::kurbo::Point;
use znimok_render::{Renderer, View, reference};

slint::include_modules!();

#[derive(Default)]
pub struct Options {
    pub headless: bool,
    pub size: (u32, u32),
    pub image: Option<String>,
    pub export: Option<String>,
    pub scale: Option<f64>,
}

const TOOLS: [&str; 10] = [
    "Select", "Rect", "Ellipse", "Line", "Pen", "Text", "Hide", "Mark", "Counter", "Stamp",
];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    Move {
        index: usize,
        start: Point,
        orig: IRect,
    },
    /// Handle 0..8: corners and edges clockwise from top-left; for lines 0 = start, 1 = end.
    Resize {
        index: usize,
        handle: usize,
        orig: IRect,
    },
    Create {
        index: usize,
        start: Point,
    },
    Pen {
        index: usize,
    },
    Pan {
        start_out: Point,
        orig: Point,
    },
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    texture: Option<(wgpu::Texture, u32, u32)>,
}

struct State {
    doc: Document,
    renderer: Renderer,
    view: View,
    pixmap: Pixmap,
    tool: usize,
    selection: Option<usize>,
    drag: Option<Drag>,
    gpu: Option<Gpu>,
    dirty: bool,
    /// Set when an input event asked for a frame; cleared after the frame is rendered.
    pending: Option<Instant>,
    latency_ms: Vec<f64>,
    cpu_ms: Vec<f64>,
    frames: u32,
    last_stats: Instant,
    editing: Option<usize>,
    dpr: f64,
    exports: u32,
    export_path: Option<String>,
}

impl State {
    fn canvas_size(&self, ui: &MainWindow) -> (u32, u32) {
        let w = (ui.get_canvas_width() as f64 * self.dpr).round().max(1.0) as u32;
        let h = (ui.get_canvas_height() as f64 * self.dpr).round().max(1.0) as u32;
        (w.min(16384), h.min(16384))
    }

    fn fit(&mut self, w: u32, h: u32) {
        let f = self.doc.frame();
        let s = (w as f64 / f.w as f64).min(h as f64 / f.h as f64).min(8.0);
        self.set_zoom(s, w, h, None);
    }

    fn set_zoom(&mut self, scale: f64, w: u32, h: u32, around_out: Option<Point>) {
        let scale = scale.clamp(0.05, 8.0);
        let f = self.doc.frame();
        match around_out {
            Some(p) => {
                let doc_pt = self.view.to_doc(p.x, p.y);
                self.view.scale = scale;
                self.view.origin = Point::new(doc_pt.x - p.x / scale, doc_pt.y - p.y / scale);
            }
            None => {
                self.view.scale = scale;
                let (cx, cy) = f.center();
                self.view.origin =
                    Point::new(cx - w as f64 / 2.0 / scale, cy - h as f64 / 2.0 / scale);
            }
        }
        self.dirty = true;
    }

    fn hit_handle(&self, out: Point) -> Option<usize> {
        let index = self.selection?;
        let obj = self.doc.objects.get(index)?;
        for (i, (x, y)) in handles(obj).into_iter().enumerate() {
            let p = self.view.to_out(Point::new(x, y));
            if (p.x - out.x).abs() <= 7.0 * self.dpr && (p.y - out.y).abs() <= 7.0 * self.dpr {
                return Some(i);
            }
        }
        None
    }

    /// Top-most object under a document point (§2.7: segments by distance, boxes by area +3 px).
    fn hit_object(&self, p: Point) -> Option<usize> {
        let slack = 3.0 / self.view.scale;
        for (i, o) in self.doc.objects.iter().enumerate().rev() {
            if o.hidden {
                continue;
            }
            let hit = match &o.data {
                Data::Line { .. } => {
                    let r = o.rect;
                    dist_to_segment(
                        p,
                        Point::new(r.x as f64, r.y as f64),
                        Point::new((r.x + r.w) as f64, (r.y + r.h) as f64),
                    ) <= 4.0 / self.view.scale + o.style.thick as f64 / 2.0
                }
                Data::Pen { points } => points.windows(2).any(|w| {
                    dist_to_segment(
                        p,
                        Point::new(w[0].0 as f64, w[0].1 as f64),
                        Point::new(w[1].0 as f64, w[1].1 as f64),
                    ) <= 4.0 / self.view.scale + o.style.thick as f64 / 2.0
                }),
                _ => {
                    let b = o.bounds();
                    let q = if o.kind().can_rotate() && o.rot != 0 {
                        unrotate(p, b, o.rot)
                    } else {
                        p
                    };
                    q.x >= b.x as f64 - slack
                        && q.y >= b.y as f64 - slack
                        && q.x <= b.right() as f64 + slack
                        && q.y <= b.bottom() as f64 + slack
                }
            };
            if hit {
                return Some(i);
            }
        }
        None
    }
}

fn unrotate(p: Point, b: IRect, rot: u16) -> Point {
    let (cx, cy) = b.center();
    let a = -(rot as f64).to_radians();
    let (dx, dy) = (p.x - cx, p.y - cy);
    Point::new(
        cx + dx * a.cos() - dy * a.sin(),
        cy + dx * a.sin() + dy * a.cos(),
    )
}

fn dist_to_segment(p: Point, a: Point, b: Point) -> f64 {
    let ab = b - a;
    let len2 = ab.hypot2();
    if len2 == 0.0 {
        return (p - a).hypot();
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    (p - (a + ab * t)).hypot()
}

/// Handle positions in document coordinates.
fn handles(o: &Object) -> Vec<(f64, f64)> {
    match o.kind() {
        Kind::Line => {
            let r = o.rect;
            vec![
                (r.x as f64, r.y as f64),
                ((r.x + r.w) as f64, (r.y + r.h) as f64),
            ]
        }
        Kind::Text | Kind::Mark => {
            let b = o.bounds();
            let (_, cy) = b.center();
            vec![(b.x as f64, cy), (b.right() as f64, cy)]
        }
        Kind::Counter | Kind::Stamp => vec![],
        _ => {
            let b = o.bounds();
            let (x0, y0, x1, y1) = (b.x as f64, b.y as f64, b.right() as f64, b.bottom() as f64);
            let (cx, cy) = b.center();
            vec![
                (x0, y0),
                (cx, y0),
                (x1, y0),
                (x1, cy),
                (x1, y1),
                (cx, y1),
                (x0, y1),
                (x0, cy),
            ]
        }
    }
}

fn apply_resize(o: &mut Object, handle: usize, orig: IRect, dx: i32, dy: i32) {
    match o.kind() {
        Kind::Line => {
            if handle == 0 {
                o.rect = IRect::new(orig.x + dx, orig.y + dy, orig.w - dx, orig.h - dy);
            } else {
                o.rect = IRect::new(orig.x, orig.y, orig.w + dx, orig.h + dy);
            }
        }
        Kind::Text => {
            if let Data::Text { box_w, .. } = &mut o.data {
                let n = orig.normalized();
                let w = if handle == 0 { n.w - dx } else { n.w + dx };
                *box_w = w.max(24);
                o.rect = if handle == 0 {
                    IRect::new(n.x + dx, n.y, w.max(24), n.h)
                } else {
                    IRect::new(n.x, n.y, w.max(24), n.h)
                };
            }
        }
        _ => {
            let n = orig.normalized();
            let (mut x0, mut y0, mut x1, mut y1) = (n.x, n.y, n.right(), n.bottom());
            match handle {
                0 => {
                    x0 += dx;
                    y0 += dy;
                }
                1 => {
                    y0 += dy;
                }
                2 => {
                    x1 += dx;
                    y0 += dy;
                }
                3 => {
                    x1 += dx;
                }
                4 => {
                    x1 += dx;
                    y1 += dy;
                }
                5 => {
                    y1 += dy;
                }
                6 => {
                    x0 += dx;
                    y1 += dy;
                }
                _ => {
                    x0 += dx;
                }
            }
            o.rect = IRect::new(x0, y0, x1 - x0, y1 - y0);
        }
    }
}

/// Selection outline and handles drawn straight into the pixmap, in screen pixels.
fn draw_selection(pix: &mut Pixmap, view: &View, o: &Object, dpr: f64) {
    let (w, h) = (pix.width() as i64, pix.height() as i64);
    let mut put = |x: i64, y: i64, c: [u8; 4]| {
        if x >= 0 && y >= 0 && x < w && y < h {
            pix.data_mut()[(y * w + x) as usize] = znimok_render::vello_cpu::color::PremulRgba8 {
                r: c[0],
                g: c[1],
                b: c[2],
                a: c[3],
            };
        }
    };
    let blue = [0x3D, 0x7B, 0xF5, 0xFF];
    let white = [0xFF, 0xFF, 0xFF, 0xFF];
    if !o.kind().is_segment() {
        let b = o.bounds();
        let p0 = view.to_out(Point::new(b.x as f64, b.y as f64));
        let p1 = view.to_out(Point::new(b.right() as f64, b.bottom() as f64));
        let (x0, y0, x1, y1) = (
            p0.x.round() as i64,
            p0.y.round() as i64,
            p1.x.round() as i64,
            p1.y.round() as i64,
        );
        for x in x0..=x1 {
            if (x - x0) % 6 < 3 {
                put(x, y0, blue);
                put(x, y1, blue);
            }
        }
        for y in y0..=y1 {
            if (y - y0) % 6 < 3 {
                put(x0, y, blue);
                put(x1, y, blue);
            }
        }
    }
    let hs = (4.0 * dpr).round() as i64;
    for (hx, hy) in handles(o) {
        let p = view.to_out(Point::new(hx, hy));
        let (cx, cy) = (p.x.round() as i64, p.y.round() as i64);
        for y in -hs..=hs {
            for x in -hs..=hs {
                let edge = x.abs() == hs || y.abs() == hs;
                put(cx + x, cy + y, if edge { blue } else { white });
            }
        }
    }
}

fn load_document(opts: &Options) -> Result<Document, Box<dyn std::error::Error>> {
    if let Some(path) = &opts.image {
        let img = image::open(path)?.to_rgba8();
        let (w, h) = img.dimensions();
        let name = std::path::Path::new(path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut doc = Document::from_raster(name, Raster::new(w, h, img.into_raw()));
        // A few marks so there is something to drag.
        let r = reference::reference_document(w.max(400), h.max(300));
        doc.objects = r.objects.into_iter().take(6).collect();
        Ok(doc)
    } else {
        let (w, h) = if opts.size == (0, 0) {
            (1600, 1000)
        } else {
            opts.size
        };
        Ok(reference::reference_document(w, h))
    }
}

fn save_png(pix: &Pixmap, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, pix.clone().into_png()?)?;
    Ok(())
}

pub fn run(opts: Options) -> Result<(), Box<dyn std::error::Error>> {
    let doc = load_document(&opts)?;
    if opts.headless {
        let mut r = Renderer::deterministic();
        let mut pix = Pixmap::new(1, 1);
        let t = Instant::now();
        r.render(&doc, View::one_to_one(&doc), &mut pix);
        let cold = t.elapsed();
        let t = Instant::now();
        r.render(&doc, View::one_to_one(&doc), &mut pix);
        let warm = t.elapsed();
        let out = opts
            .export
            .clone()
            .unwrap_or_else(|| "target/p1.png".into());
        save_png(&pix, std::path::Path::new(&out))?;
        println!(
            "{}x{} rendered: cold {:.1} ms, warm {:.1} ms → {out}",
            pix.width(),
            pix.height(),
            cold.as_secs_f64() * 1e3,
            warm.as_secs_f64() * 1e3
        );
        return Ok(());
    }

    // Default backend per OS. Letting wgpu probe every backend crashed natively (no panic) on the
    // agent's machine (Intel UHD 630 under RDP) — with an explicit backend it runs. WGPU_BACKEND
    // still overrides for experiments.
    let mut settings = slint::wgpu_30::WGPUSettings::default();
    if wgpu::Backends::from_env().is_none() {
        settings.backends = if cfg!(target_os = "macos") {
            wgpu::Backends::METAL
        } else {
            wgpu::Backends::DX12
        };
    }
    slint::BackendSelector::new()
        .require_wgpu_30(slint::wgpu_30::WGPUConfiguration::Automatic(settings))
        .select()?;
    let ui = MainWindow::new()?;
    let title = format!(
        "{} · {}×{}",
        doc.name,
        doc.image_size().0,
        doc.image_size().1
    );
    ui.set_doc_title(title.into());

    let st = Rc::new(RefCell::new(State {
        doc,
        renderer: Renderer::new(),
        view: View {
            scale: 1.0,
            origin: Point::ZERO,
            width: 1,
            height: 1,
        },
        pixmap: Pixmap::new(1, 1),
        tool: 0,
        selection: None,
        drag: None,
        gpu: None,
        dirty: true,
        pending: None,
        latency_ms: Vec::new(),
        cpu_ms: Vec::new(),
        frames: 0,
        last_stats: Instant::now(),
        editing: None,
        dpr: 1.0,
        exports: 0,
        export_path: opts.export.clone(),
    }));
    let initial_scale = opts.scale;

    // --- rendering: texture upload before Slint draws, latency sample after.
    {
        let st = st.clone();
        let ui_weak = ui.as_weak();
        ui.window().set_rendering_notifier(move |state, api| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let mut s = st.borrow_mut();
            match state {
                slint::RenderingState::RenderingSetup => {
                    if let slint::GraphicsAPI::WGPU30 { device, queue, .. } = api {
                        s.gpu = Some(Gpu {
                            device: device.clone(),
                            queue: queue.clone(),
                            texture: None,
                        });
                        s.dpr = ui.window().scale_factor() as f64;
                        s.dirty = true;
                    }
                }
                slint::RenderingState::BeforeRendering => {
                    s.dpr = ui.window().scale_factor() as f64;
                    let (w, h) = s.canvas_size(&ui);
                    let size_changed = s
                        .gpu
                        .as_ref()
                        .and_then(|g| g.texture.as_ref())
                        .map(|(_, tw, th)| (*tw, *th) != (w, h))
                        .unwrap_or(true);
                    if size_changed {
                        let first = s.view.width == 1;
                        s.view.width = w.min(65535) as u16;
                        s.view.height = h.min(65535) as u16;
                        if first {
                            match initial_scale {
                                Some(z) => s.set_zoom(z, w, h, None),
                                None => s.fit(w, h),
                            }
                        }
                        s.dirty = true;
                    }
                    if !s.dirty {
                        return;
                    }
                    let t0 = Instant::now();
                    let State {
                        doc,
                        renderer,
                        view,
                        pixmap,
                        ..
                    } = &mut *s;
                    renderer.render(doc, *view, pixmap);
                    if let Some(i) = s.selection.and_then(|i| s.doc.objects.get(i).map(|_| i)) {
                        let State {
                            pixmap,
                            view,
                            doc,
                            dpr,
                            ..
                        } = &mut *s;
                        draw_selection(pixmap, view, &doc.objects[i], *dpr);
                    }
                    let cpu = t0.elapsed().as_secs_f64() * 1e3;
                    s.cpu_ms.push(cpu);
                    if s.cpu_ms.len() > 240 {
                        s.cpu_ms.drain(..120);
                    }
                    let State { gpu, pixmap, .. } = &mut *s;
                    let Some(gpu) = gpu.as_mut() else { return };
                    if size_changed || gpu.texture.is_none() {
                        let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
                            label: Some("znimok canvas"),
                            size: wgpu::Extent3d {
                                width: w,
                                height: h,
                                depth_or_array_layers: 1,
                            },
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: wgpu::TextureFormat::Rgba8Unorm,
                            usage: wgpu::TextureUsages::TEXTURE_BINDING
                                | wgpu::TextureUsages::RENDER_ATTACHMENT
                                | wgpu::TextureUsages::COPY_DST,
                            view_formats: &[],
                        });
                        ui.set_canvas(slint::Image::try_from(tex.clone()).expect("texture import"));
                        gpu.texture = Some((tex, w, h));
                    }
                    let (tex, _, _) = gpu.texture.as_ref().unwrap();
                    gpu.queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: tex,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        pixmap.data_as_u8_slice(),
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(w * 4),
                            rows_per_image: Some(h),
                        },
                        wgpu::Extent3d {
                            width: w,
                            height: h,
                            depth_or_array_layers: 1,
                        },
                    );
                    s.dirty = false;
                    s.frames += 1;
                }
                slint::RenderingState::AfterRendering => {
                    if let Some(t) = s.pending.take() {
                        let ms = t.elapsed().as_secs_f64() * 1e3;
                        s.latency_ms.push(ms);
                        if s.latency_ms.len() > 400 {
                            s.latency_ms.drain(..200);
                        }
                    }
                }
                slint::RenderingState::RenderingTeardown => {
                    s.gpu = None;
                }
                _ => {}
            }
        })?;
    }

    // --- stats every 250 ms, without forcing frames on its own.
    let stats_timer = slint::Timer::default();
    {
        let st = st.clone();
        let ui_weak = ui.as_weak();
        stats_timer.start(slint::TimerMode::Repeated, Duration::from_millis(250), move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let mut s = st.borrow_mut();
            let dt = s.last_stats.elapsed().as_secs_f64();
            let fps = s.frames as f64 / dt.max(1e-3);
            s.frames = 0;
            s.last_stats = Instant::now();
            let pct = |v: &[f64], p: f64| -> f64 {
                if v.is_empty() {
                    return 0.0;
                }
                let mut c = v.to_vec();
                c.sort_by(|a, b| a.partial_cmp(b).unwrap());
                c[((c.len() - 1) as f64 * p).round() as usize]
            };
            let lat = &s.latency_ms;
            let cpu = &s.cpu_ms;
            let text = format!(
                "подія→кадр p50 {:.1} p95 {:.1} max {:.1} мс · CPU p50 {:.1} p95 {:.1} мс · {:.0} к/с · {} потоків",
                pct(lat, 0.5), pct(lat, 0.95), pct(lat, 1.0), pct(cpu, 0.5), pct(cpu, 0.95), fps, s.renderer.threads()
            );
            ui.set_stats(text.into());
            let (iw, ih) = s.doc.image_size();
            let status = format!("{}×{} · {} позначок · {:.0}% · {}", iw, ih, s.doc.objects.len(), s.view.scale * 100.0, TOOLS[s.tool]);
            ui.set_status(status.into());
        });
    }

    // --- tools
    {
        let st = st.clone();
        let ui_weak = ui.as_weak();
        ui.on_tool_chosen(move |t| {
            let Some(ui) = ui_weak.upgrade() else { return };
            st.borrow_mut().tool = t as usize;
            ui.set_tool(t);
        });
    }

    // --- pointer
    {
        let st = st.clone();
        let ui_weak = ui.as_weak();
        ui.on_pointer(move |kind, x, y, button, shift, _alt| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let mut s = st.borrow_mut();
            let out = Point::new(x as f64 * s.dpr, y as f64 * s.dpr);
            let p = s.view.to_doc(out.x, out.y);
            let pi = Point::new(p.x.round(), p.y.round());
            match kind {
                0 => {
                    if button == 2 {
                        s.drag = Some(Drag::Pan {
                            start_out: out,
                            orig: s.view.origin,
                        });
                        return;
                    }
                    if button == 1 {
                        return;
                    }
                    // Handles of the selection are checked before the tool (§2.7).
                    if let Some(h) = s.hit_handle(out) {
                        let index = s.selection.unwrap();
                        let orig = s.doc.objects[index].rect;
                        s.drag = Some(Drag::Resize {
                            index,
                            handle: h,
                            orig,
                        });
                        s.renderer_measure_text(index);
                        return;
                    }
                    let tool = s.tool;
                    if tool == 0 {
                        if let Some(i) = s.hit_object(p) {
                            s.selection = Some(i);
                            let orig = s.doc.objects[i].rect;
                            s.drag = Some(Drag::Move {
                                index: i,
                                start: pi,
                                orig,
                            });
                        } else {
                            s.selection = None;
                        }
                        s.dirty = true;
                        s.pending = Some(Instant::now());
                        ui.window().request_redraw();
                        return;
                    }
                    let style = Style::default();
                    let (data, rect, stamped) = match tool {
                        1 => (
                            Data::Rect,
                            IRect::new(pi.x as i32, pi.y as i32, 0, 0),
                            false,
                        ),
                        2 => (
                            Data::Ellipse,
                            IRect::new(pi.x as i32, pi.y as i32, 0, 0),
                            false,
                        ),
                        3 => (
                            Data::Line {
                                head_front: Head::Triangle,
                                head_back: Head::None,
                                head_size: 0,
                            },
                            IRect::new(pi.x as i32, pi.y as i32, 0, 0),
                            false,
                        ),
                        4 => (
                            Data::Pen {
                                points: vec![(pi.x as i32, pi.y as i32)],
                            },
                            IRect::new(pi.x as i32, pi.y as i32, 0, 0),
                            false,
                        ),
                        5 => (
                            Data::Text {
                                text: "Текст".into(),
                                size: 24,
                                bold: false,
                                italic: false,
                                align: Align::Left,
                                box_w: 0,
                            },
                            IRect::new(pi.x as i32, pi.y as i32, 0, 0),
                            true,
                        ),
                        6 => (
                            Data::Hide {
                                mode: HideMode::Pixelate,
                                strength: 50,
                            },
                            IRect::new(pi.x as i32, pi.y as i32, 0, 0),
                            false,
                        ),
                        7 => (
                            Data::Mark,
                            IRect::new(pi.x as i32, pi.y as i32, 0, 0),
                            false,
                        ),
                        8 => {
                            let seq = s.doc.next_counter_seq();
                            (
                                Data::Counter {
                                    seq,
                                    group: 1,
                                    start: 1,
                                    shape: CounterShape::Circle,
                                },
                                IRect::new(pi.x as i32 - 18, pi.y as i32 - 18, 36, 36),
                                true,
                            )
                        }
                        _ => (
                            Data::Stamp { id: 0 },
                            IRect::new(pi.x as i32 - 22, pi.y as i32 - 22, 44, 44),
                            true,
                        ),
                    };
                    let mut obj = Object::new(rect, data).with_style(style);
                    match tool {
                        7 => {
                            obj.style = Style {
                                color: Rgb::YELLOW,
                                thick: 24,
                                ..style
                            }
                        }
                        8 => obj.style = Style { thick: 36, ..style },
                        9 => {
                            obj.style = Style {
                                color: Rgb::GREEN,
                                thick: 44,
                                ..style
                            }
                        }
                        _ => {}
                    }
                    let index = s.doc.push(obj);
                    s.selection = Some(index);
                    if tool == 5 {
                        s.renderer_measure_text(index);
                        s.editing = Some(index);
                        let o = s.view.to_out(Point::new(pi.x, pi.y));
                        ui.set_edit_x((o.x / s.dpr) as f32);
                        ui.set_edit_y((o.y / s.dpr) as f32);
                        ui.set_edit_w(200.0);
                        ui.set_edit_text("Текст".into());
                        ui.set_editing(true);
                    } else if tool == 4 {
                        s.drag = Some(Drag::Pen { index });
                    } else if !stamped {
                        s.drag = Some(Drag::Create { index, start: pi });
                    }
                    s.dirty = true;
                    s.pending = Some(Instant::now());
                    ui.window().request_redraw();
                }
                1 => {
                    let Some(drag) = s.drag else { return };
                    match drag {
                        Drag::Move { index, start, orig } => {
                            let (dx, dy) = ((pi.x - start.x) as i32, (pi.y - start.y) as i32);
                            let o = &mut s.doc.objects[index];
                            if let Data::Pen { points } = &mut o.data {
                                let (odx, ody) = (o.rect.x - orig.x, o.rect.y - orig.y);
                                for pt in points.iter_mut() {
                                    pt.0 += dx - odx;
                                    pt.1 += dy - ody;
                                }
                            }
                            o.rect = orig.translated(dx, dy);
                        }
                        Drag::Resize {
                            index,
                            handle,
                            orig,
                        } => {
                            let n = orig;
                            let anchor = match handle {
                                0 | 1 | 2
                                    if !matches!(
                                        s.doc.objects[index].kind(),
                                        Kind::Line | Kind::Text
                                    ) =>
                                {
                                    (n.normalized().x, n.normalized().y)
                                }
                                _ => (n.x, n.y),
                            };
                            let _ = anchor;
                            let hp = handles(&Object::new(orig, s.doc.objects[index].data.clone()))
                                [handle];
                            let (dx, dy) = ((pi.x - hp.0) as i32, (pi.y - hp.1) as i32);
                            apply_resize(&mut s.doc.objects[index], handle, orig, dx, dy);
                            s.renderer_measure_text(index);
                        }
                        Drag::Create { index, start } => {
                            let o = &mut s.doc.objects[index];
                            let (mut w, mut h) = ((pi.x - start.x) as i32, (pi.y - start.y) as i32);
                            if shift && o.kind() != Kind::Line {
                                let m = w.abs().max(h.abs());
                                w = m * w.signum().max(if w == 0 { 1 } else { w.signum() });
                                h = m * if h == 0 { 1 } else { h.signum() };
                            }
                            o.rect = IRect::new(start.x as i32, start.y as i32, w, h);
                        }
                        Drag::Pen { index } => {
                            if let Data::Pen { points } = &mut s.doc.objects[index].data {
                                let last = *points.last().unwrap();
                                if (last.0 - pi.x as i32).abs() + (last.1 - pi.y as i32).abs() >= 1
                                {
                                    points.push((pi.x as i32, pi.y as i32));
                                }
                            }
                            let b = s.doc.objects[index].bounds();
                            s.doc.objects[index].rect = b;
                        }
                        Drag::Pan { start_out, orig } => {
                            let sc = s.view.scale;
                            s.view.origin = Point::new(
                                orig.x - (out.x - start_out.x) / sc,
                                orig.y - (out.y - start_out.y) / sc,
                            );
                        }
                    }
                    s.dirty = true;
                    s.pending = Some(Instant::now());
                    ui.window().request_redraw();
                }
                _ => {
                    if let Some(drag) = s.drag.take() {
                        match drag {
                            Drag::Create { index, .. } => {
                                let o = &s.doc.objects[index];
                                let b = o.bounds();
                                if b.w.max(b.h) < 3 {
                                    s.doc.objects.remove(index);
                                    s.selection = None;
                                }
                            }
                            Drag::Resize { index, .. } => {
                                let o = &mut s.doc.objects[index];
                                if o.kind() != Kind::Line {
                                    o.rect = o.rect.normalized();
                                }
                            }
                            _ => {}
                        }
                        s.dirty = true;
                        s.pending = Some(Instant::now());
                        ui.window().request_redraw();
                    }
                }
            }
        });
    }

    // --- wheel: Alt = zoom around the cursor, plain = scroll (§7 п.37).
    {
        let st = st.clone();
        let ui_weak = ui.as_weak();
        ui.on_wheel(move |x, y, dy, alt| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let mut s = st.borrow_mut();
            let out = Point::new(x as f64 * s.dpr, y as f64 * s.dpr);
            if alt {
                let factor = if dy > 0.0 { 1.15 } else { 1.0 / 1.15 };
                let z = s.view.scale * factor;
                let (w, h) = s.canvas_size(&ui);
                s.set_zoom(z, w, h, Some(out));
            } else {
                let sc = s.view.scale;
                s.view.origin.y -= dy as f64 * s.dpr / sc;
                s.dirty = true;
            }
            s.pending = Some(Instant::now());
            ui.window().request_redraw();
        });
    }

    // --- keys
    {
        let st = st.clone();
        let ui_weak = ui.as_weak();
        ui.on_key(move |text, ctrl, _shift| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let mut s = st.borrow_mut();
            let t = text.as_str();
            let tool_keys = ["v", "r", "e", "l", "p", "t", "h", "m", "n", "s"];
            if !ctrl && let Some(i) = tool_keys.iter().position(|k| *k == t.to_ascii_lowercase()) {
                s.tool = i;
                ui.set_tool(i as i32);
                return;
            }
            match t {
                "\u{1b}" => {
                    if s.drag.is_some() {
                        s.drag = None;
                    } else if s.selection.is_some() {
                        s.selection = None;
                    } else if s.tool != 0 {
                        s.tool = 0;
                        ui.set_tool(0);
                    }
                }
                "\u{7f}" | "\u{8}" => {
                    if let Some(i) = s.selection.take()
                        && i < s.doc.objects.len()
                    {
                        s.doc.objects.remove(i);
                    }
                }
                "0" if ctrl => {
                    let (w, h) = s.canvas_size(&ui);
                    s.fit(w, h);
                }
                "1" if ctrl => {
                    let (w, h) = s.canvas_size(&ui);
                    s.set_zoom(1.0, w, h, None);
                }
                "e" | "E" if ctrl => {
                    drop(s);
                    ui.invoke_export_clicked();
                    return;
                }
                _ => return,
            }
            s.dirty = true;
            ui.window().request_redraw();
        });
    }

    // --- zoom buttons
    {
        let st = st.clone();
        let ui_weak = ui.as_weak();
        ui.on_zoom_fit(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let mut s = st.borrow_mut();
            let (w, h) = s.canvas_size(&ui);
            s.fit(w, h);
            ui.window().request_redraw();
        });
    }
    {
        let st = st.clone();
        let ui_weak = ui.as_weak();
        ui.on_zoom_100(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let mut s = st.borrow_mut();
            let (w, h) = s.canvas_size(&ui);
            s.set_zoom(1.0, w, h, None);
            ui.window().request_redraw();
        });
    }

    // --- text editing overlay
    {
        let st = st.clone();
        let ui_weak = ui.as_weak();
        ui.on_commit_text(move |t| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let mut s = st.borrow_mut();
            if let Some(i) = s.editing.take() {
                if let Data::Text { text, .. } = &mut s.doc.objects[i].data {
                    *text = t.to_string();
                }
                if t.trim().is_empty() {
                    s.doc.objects.remove(i);
                    s.selection = None;
                } else {
                    s.renderer_measure_text(i);
                }
            }
            ui.set_editing(false);
            s.dirty = true;
            ui.window().request_redraw();
        });
    }
    {
        let st = st.clone();
        let ui_weak = ui.as_weak();
        ui.on_cancel_text(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let mut s = st.borrow_mut();
            if let Some(i) = s.editing.take() {
                s.doc.objects.remove(i);
                s.selection = None;
            }
            ui.set_editing(false);
            s.dirty = true;
            ui.window().request_redraw();
        });
    }

    // --- export 1:1
    {
        let st = st.clone();
        let ui_weak = ui.as_weak();
        ui.on_export_clicked(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let mut s = st.borrow_mut();
            let mut pix = Pixmap::new(1, 1);
            let t = Instant::now();
            let State { doc, renderer, .. } = &mut *s;
            renderer.render(doc, View::one_to_one(doc), &mut pix);
            let ms = t.elapsed().as_secs_f64() * 1e3;
            s.exports += 1;
            let path = match &s.export_path {
                Some(p) => std::path::PathBuf::from(p),
                None => std::path::PathBuf::from(format!("target/p1-export-{}.png", s.exports)),
            };
            let msg = match save_png(&pix, &path) {
                Ok(()) => format!(
                    "Експортовано {}×{} за {ms:.0} мс → {}",
                    pix.width(),
                    pix.height(),
                    path.display()
                ),
                Err(e) => format!("Експорт не вдався: {e}"),
            };
            ui.set_status(SharedString::from(msg));
        });
    }

    ui.run()?;
    drop(stats_timer);
    Ok(())
}

impl State {
    /// Text objects size themselves from their text (§6: +2 px so nothing wraps by rounding).
    fn renderer_measure_text(&mut self, index: usize) {
        let Some(o) = self.doc.objects.get(index) else {
            return;
        };
        let Data::Text {
            text,
            size,
            bold,
            italic,
            box_w,
            ..
        } = &o.data
        else {
            return;
        };
        let (w, h) = self
            .renderer
            .measure_text(text, *size, *bold, *italic, *box_w);
        let bw = *box_w;
        let o = &mut self.doc.objects[index];
        o.rect.w = if bw > 0 { bw } else { w.ceil() as i32 };
        o.rect.h = h.ceil() as i32;
    }
}
