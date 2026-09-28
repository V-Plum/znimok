use std::cell::RefCell;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSEvent};
use objc2_service_management::{SMAppService, SMAppServiceStatus};
use screencapturekit::content_sharing_picker::{
    SCContentSharingPicker, SCContentSharingPickerConfiguration, SCContentSharingPickerMode,
    SCPickerOutcome,
};
use screencapturekit::prelude::*;
use screencapturekit::screenshot_manager::{CGImage, CGImageExt, SCScreenshotManager};
use screencapturekit::shareable_content::SCShareableContentInfo;
use slint::ComponentHandle;

slint::include_modules!();

const INBOX: &str = "/Users/Shared/znimok-builds/inbox";

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

// ---- log: a file in the shared inbox + an in-window tail -------------------------------------

static LOG_TAIL: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn log_path() -> PathBuf {
    let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
    PathBuf::from(INBOX).join(format!("p3-{user}.log"))
}

fn stamp() -> String {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{}.{:03}", t / 1000, t % 1000)
}

fn log(msg: impl AsRef<str>) {
    let line = format!("{} {}", stamp(), msg.as_ref());
    eprintln!("{line}");
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
    {
        let _ = writeln!(f, "{line}");
    }
    if let Ok(mut tail) = LOG_TAIL.lock() {
        tail.push(line);
        let n = tail.len();
        if n > 40 {
            tail.drain(..n - 40);
        }
    }
}

fn log_text() -> String {
    LOG_TAIL.lock().map(|t| t.join("\n")).unwrap_or_default()
}

// ---- capture ----------------------------------------------------------------------------------

fn shots_dir() -> PathBuf {
    let d = PathBuf::from(INBOX).join("shots");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn save_png(img: &CGImage, tag: &str) -> Result<PathBuf, String> {
    let (w, h) = (img.width() as u32, img.height() as u32);
    let rgba = img.rgba_data().map_err(|e| format!("rgba_data: {e}"))?;
    let path = shots_dir().join(format!("{}-{tag}-{w}x{h}.png", stamp().replace('.', "-")));
    image::save_buffer(&path, &rgba, w, h, image::ExtendedColorType::Rgba8)
        .map_err(|e| format!("png: {e}"))?;
    Ok(path)
}

/// Mouse position in global CoreGraphics coordinates (origin at the top-left of the primary
/// display). Must be called on the main thread.
fn mouse_cg(primary_height: f64) -> (f64, f64) {
    #[allow(unused_unsafe)]
    let p = unsafe { NSEvent::mouseLocation() };
    (p.x, primary_height - p.y)
}

/// Screenshot of the display under `mouse` (CG coordinates), excluding this app's own windows.
fn capture_display_at(mouse: Option<(f64, f64)>) -> Result<String, String> {
    let t0 = Instant::now();
    let content = SCShareableContent::get().map_err(|e| format!("SCShareableContent: {e}"))?;
    let displays = content.displays();
    if displays.is_empty() {
        return Err("жодного дисплея (дозвіл на запис екрана не надано?)".into());
    }
    let display = mouse
        .and_then(|(x, y)| {
            displays.iter().find(|d| {
                let f = d.frame();
                x >= f.origin.x
                    && y >= f.origin.y
                    && x < f.origin.x + f.size.width
                    && y < f.origin.y + f.size.height
            })
        })
        .unwrap_or(&displays[0]);
    let me = std::process::id() as i32;
    let own: Vec<SCWindow> = content
        .windows()
        .into_iter()
        .filter(|w| {
            w.owning_application()
                .map(|a| a.process_id() == me)
                .unwrap_or(false)
        })
        .collect();
    let own_refs: Vec<&SCWindow> = own.iter().collect();
    let filter = SCContentFilter::create()
        .with_display(display)
        .with_excluding_windows(&own_refs)
        .build()
        .map_err(|e| format!("filter: {e}"))?;
    let (pw, ph) = SCShareableContentInfo::for_filter(&filter)
        .map(|i| i.pixel_size())
        .unwrap_or((display.width() * 2, display.height() * 2));
    let cfg = SCStreamConfiguration::new()
        .with_width(pw)
        .with_height(ph)
        .with_shows_cursor(false);
    let t1 = Instant::now();
    let img = SCScreenshotManager::capture_image(&filter, &cfg)
        .map_err(|e| format!("capture_image: {e}"))?;
    let t2 = Instant::now();
    let path = save_png(&img, &format!("display{}", display.display_id()))?;
    Ok(format!(
        "дисплей {} ({}×{} pt → {}×{} px): контент {:.0} мс, кадр {:.0} мс, PNG {:.0} мс → {}",
        display.display_id(),
        display.width(),
        display.height(),
        img.width(),
        img.height(),
        (t1 - t0).as_secs_f64() * 1e3,
        (t2 - t1).as_secs_f64() * 1e3,
        t2.elapsed().as_secs_f64() * 1e3,
        path.display()
    ))
}

/// Opens the system content picker; the pick is captured on the callback thread.
fn capture_via_picker(done: impl Fn(Result<String, String>) + Send + 'static) {
    let mut cfg = match SCContentSharingPickerConfiguration::new() {
        Ok(c) => c,
        Err(e) => return done(Err(format!("picker config: {e}"))),
    };
    cfg.set_allowed_picker_modes(&[
        SCContentSharingPickerMode::SingleWindow,
        SCContentSharingPickerMode::SingleDisplay,
        SCContentSharingPickerMode::SingleApplication,
    ]);
    let t0 = Instant::now();
    SCContentSharingPicker::show(&cfg, move |outcome| {
        let r = match outcome {
            SCPickerOutcome::Picked(result) => {
                let picked = t0.elapsed();
                let filter = result.filter();
                let (pw, ph) = result.pixel_size();
                let what = format!(
                    "{} вікон, {} дисплеїв",
                    result.windows().len(),
                    result.displays().len()
                );
                let cfg = SCStreamConfiguration::new()
                    .with_width(pw.max(1))
                    .with_height(ph.max(1))
                    .with_shows_cursor(false);
                let t1 = Instant::now();
                match SCScreenshotManager::capture_image(&filter, &cfg) {
                    Ok(img) => save_png(&img, "picker").map(|p| {
                        format!(
                            "пікер ({what}): вибір {:.1} с, кадр {:.0} мс, {}×{} px → {}",
                            picked.as_secs_f64(),
                            t1.elapsed().as_secs_f64() * 1e3,
                            img.width(),
                            img.height(),
                            p.display()
                        )
                    }),
                    Err(e) => Err(format!("capture_image після пікера: {e}")),
                }
            }
            SCPickerOutcome::Cancelled => Err("пікер скасовано".into()),
            SCPickerOutcome::Error(m) => Err(format!("пікер: {m}")),
        };
        done(r);
    });
}

// ---- shortcuts --------------------------------------------------------------------------------

/// Whether the system screenshot shortcuts ⌘⇧3 / ⌘⇧4 / ⌘⇧5 are still enabled. Keys absent
/// from the user's `com.apple.symbolichotkeys` mean "default", which is enabled.
fn system_shortcuts_enabled() -> Result<Vec<(u32, bool)>, String> {
    let out = std::process::Command::new("/usr/bin/defaults")
        .args(["export", "com.apple.symbolichotkeys", "-"])
        .output()
        .map_err(|e| format!("defaults: {e}"))?;
    let v = plist::Value::from_reader_xml(&out.stdout[..]).map_err(|e| format!("plist: {e}"))?;
    let hk = v
        .as_dictionary()
        .and_then(|d| d.get("AppleSymbolicHotKeys"))
        .and_then(|x| x.as_dictionary());
    let mut r = Vec::new();
    for id in [28u32, 30, 184] {
        let enabled = hk
            .and_then(|d| d.get(&id.to_string()))
            .and_then(|x| x.as_dictionary())
            .and_then(|d| d.get("enabled"))
            .map(|e| {
                e.as_boolean()
                    .unwrap_or_else(|| e.as_signed_integer().unwrap_or(1) != 0)
            })
            .unwrap_or(true);
        r.push((id, enabled));
    }
    Ok(r)
}

struct Keys {
    manager: GlobalHotKeyManager,
    registered: Vec<HotKey>,
    use_cmd: bool,
    screen: Option<u32>,
    window: Option<u32>,
}

impl Keys {
    fn apply(&mut self, use_cmd: bool) -> Result<(), String> {
        if !self.registered.is_empty() && self.use_cmd == use_cmd {
            return Ok(());
        }
        let _ = self.manager.unregister_all(&self.registered);
        self.registered.clear();
        let mods = if use_cmd {
            Modifiers::SUPER | Modifiers::SHIFT
        } else {
            Modifiers::CONTROL | Modifiers::SHIFT
        };
        let screen = HotKey::new(Some(mods), Code::Digit3);
        let window = HotKey::new(Some(mods), Code::Digit4);
        let mut errs = Vec::new();
        for k in [screen, window] {
            match self.manager.register(k) {
                Ok(()) => self.registered.push(k),
                Err(e) => errs.push(format!("{k:?}: {e}")),
            }
        }
        self.use_cmd = use_cmd;
        self.screen = Some(screen.id());
        self.window = Some(window.id());
        if errs.is_empty() {
            Ok(())
        } else {
            Err(errs.join("; "))
        }
    }

    fn labels(&self) -> (&'static str, &'static str) {
        if self.use_cmd {
            ("⌘⇧3", "⌘⇧4")
        } else {
            ("⌃⇧3", "⌃⇧4")
        }
    }
}

// ---- login item -------------------------------------------------------------------------------

fn autostart_status() -> (bool, String) {
    let s = unsafe { SMAppService::mainAppService().status() };
    let text = match s {
        SMAppServiceStatus::Enabled => "увімкнено — Znimok P3 запуститься після входу в систему",
        SMAppServiceStatus::RequiresApproval => {
            "потрібне підтвердження: Системні налаштування → Загальні → Об'єкти входу"
        }
        SMAppServiceStatus::NotFound => "служба не знайдена (програма не в пакеті .app?)",
        _ => "вимкнено",
    };
    (s == SMAppServiceStatus::Enabled, text.to_string())
}

fn toggle_autostart() -> String {
    let svc = unsafe { SMAppService::mainAppService() };
    let enabled = unsafe { svc.status() } == SMAppServiceStatus::Enabled;
    let r = if enabled {
        unsafe { svc.unregisterAndReturnError() }
    } else {
        unsafe { svc.registerAndReturnError() }
    };
    match r {
        Ok(()) => format!(
            "автозапуск: {}",
            if enabled {
                "вимкнено"
            } else {
                "увімкнено"
            }
        ),
        Err(e) => format!("автозапуск: помилка {}", e.localizedDescription()),
    }
}

// ---- tray icon: the ZK mark on a dark rounded square, drawn with vello_cpu --------------------

fn tray_icon() -> slint::Image {
    use vello_cpu::color::{AlphaColor, Srgb};
    use vello_cpu::kurbo::{Affine, BezPath, Cap, Join, Point, RoundedRect, Shape, Stroke};
    let size = 44u16;
    let mut ctx = vello_cpu::RenderContext::new(size, size);
    let k = size as f64 / 24.0;
    ctx.set_transform(Affine::scale(k));
    ctx.set_paint(AlphaColor::<Srgb>::from_rgba8(0x1E, 0x22, 0x29, 255));
    ctx.fill_path(&RoundedRect::new(0.5, 0.5, 23.5, 23.5, 5.5).to_path(0.05));
    ctx.set_stroke(
        Stroke::new(2.4)
            .with_caps(Cap::Round)
            .with_join(Join::Round),
    );
    let mut z = BezPath::new();
    z.move_to(Point::new(3.5, 13.5));
    z.line_to(Point::new(11.5, 13.5));
    z.line_to(Point::new(3.5, 21.0));
    z.line_to(Point::new(11.5, 21.0));
    ctx.set_paint(AlphaColor::<Srgb>::from_rgba8(0x3D, 0x7B, 0xF5, 255));
    ctx.stroke_path(&z);
    let mut kk = BezPath::new();
    kk.move_to(Point::new(14.5, 3.0));
    kk.line_to(Point::new(14.5, 21.0));
    kk.move_to(Point::new(21.0, 4.5));
    kk.line_to(Point::new(14.5, 12.0));
    kk.line_to(Point::new(21.0, 20.0));
    ctx.set_paint(AlphaColor::<Srgb>::from_rgba8(0xFF, 0xD2, 0x3F, 255));
    ctx.stroke_path(&kk);
    ctx.flush();
    let mut pix = vello_cpu::Pixmap::new(size, size);
    let mut res = vello_cpu::Resources::default();
    ctx.render(pix.as_mut(), &mut res);
    let rgba: Vec<u8> = pix
        .take_unpremultiplied()
        .into_iter()
        .flat_map(|c| [c.r, c.g, c.b, c.a])
        .collect();
    let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        &rgba,
        size as u32,
        size as u32,
    );
    slint::Image::from_rgba8(buf)
}

// ---- app --------------------------------------------------------------------------------------

struct App {
    window: P3Window,
    tray: P3Tray,
    keys: Keys,
    primary_height: f64,
}

impl App {
    fn refresh(&mut self) {
        let capture_ok = unsafe { CGPreflightScreenCaptureAccess() };
        self.window.set_capture_ok(capture_ok);
        self.window.set_capture_state(
            if capture_ok {
                "Надано. Знімки екрана й вікон працюють без запитів."
            } else {
                "Не надано. Без нього працює лише «Знімок вікна через пікер» — система сама показує вибір."
            }
            .into(),
        );
        match system_shortcuts_enabled() {
            Ok(list) => {
                let all_free = list.iter().all(|(_, on)| !on);
                if let Err(e) = self.keys.apply(all_free) {
                    log(format!("клавіші: не вдалося зареєструвати: {e}"));
                }
                let (s, w) = self.keys.labels();
                self.window.set_keys_ok(all_free);
                self.window
                    .set_keys_state(format!("Зараз: {s} — екран, {w} — вікно через пікер.").into());
                self.window.set_keys_hint(
                    if all_free {
                        "Системні ⌘⇧3/4/5 вимкнено — Znimok узяв їх собі."
                    } else {
                        "Щоб Znimok працював на звичних ⌘⇧3/4/5: Клавіатура → Скорочення клавіш → Знімки екрана — зніміть позначки. Znimok помітить це сам."
                    }
                    .into(),
                );
                self.tray.set_screen_key(s.into());
                self.tray.set_window_key(w.into());
            }
            Err(e) => {
                log(format!(
                    "клавіші: не вдалося прочитати системні скорочення: {e}"
                ));
                let _ = self.keys.apply(false);
            }
        }
        let (on, text) = autostart_status();
        self.window.set_autostart_ok(on);
        self.window.set_autostart_state(text.into());
        self.tray.set_autostart(on);
        self.window.set_log_text(log_text().into());
    }
}

fn result_to_ui(app: &Rc<RefCell<App>>, r: Result<String, String>) {
    let text = match r {
        Ok(s) => {
            log(format!("OK {s}"));
            s
        }
        Err(e) => {
            log(format!("ПОМИЛКА {e}"));
            format!("Помилка: {e}")
        }
    };
    let a = app.borrow();
    a.window.set_last_result(text.into());
    a.window.set_log_text(log_text().into());
}

fn run_capture_display(app: &Rc<RefCell<App>>) {
    let mouse = Some(mouse_cg(app.borrow().primary_height));
    std::thread::spawn(move || {
        let r = capture_display_at(mouse);
        let _ = slint::invoke_from_event_loop(move || {
            APP.with(|a| {
                if let Some(app) = a.borrow().as_ref() {
                    result_to_ui(app, r);
                }
            })
        });
    });
}

fn run_capture_picker() {
    capture_via_picker(|r| {
        let _ = slint::invoke_from_event_loop(move || {
            APP.with(|a| {
                if let Some(app) = a.borrow().as_ref() {
                    result_to_ui(app, r);
                }
            })
        });
    });
}

thread_local! {
    static APP: RefCell<Option<Rc<RefCell<App>>>> = const { RefCell::new(None) };
}

fn activate() {
    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
    }
}

fn open_url(url: &str) {
    let _ = std::process::Command::new("/usr/bin/open").arg(url).spawn();
}

pub fn run() {
    let _ = std::fs::create_dir_all(INBOX);
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let build = option_env!("ZNIMOK_BUILD").unwrap_or("dev");
    log(format!(
        "старт: збірка {build}, {exe}, користувач {}",
        std::env::var("USER").unwrap_or_default()
    ));

    let mut settings = slint::wgpu_30::WGPUSettings::default();
    settings.backends = slint::wgpu_30::wgpu::Backends::METAL;
    if let Err(e) = slint::BackendSelector::new()
        .require_wgpu_30(slint::wgpu_30::WGPUConfiguration::Automatic(settings))
        .select()
    {
        log(format!("бекенд Slint: {e}"));
    }

    let window = P3Window::new().expect("window");
    let tray = P3Tray::new().expect("tray");
    tray.set_tray_icon(tray_icon());
    window.set_build_info(format!("збірка {build} · {}", std::env::consts::ARCH).into());

    let manager = GlobalHotKeyManager::new().expect("hotkey manager");
    let keys = Keys {
        manager,
        registered: Vec::new(),
        use_cmd: false,
        screen: None,
        window: None,
    };

    // Primary display height converts AppKit's bottom-left mouse position to CG coordinates.
    let primary_height = SCShareableContent::get()
        .ok()
        .and_then(|c| {
            c.displays()
                .into_iter()
                .find(|d| d.frame().origin.x == 0.0 && d.frame().origin.y == 0.0)
                .map(|d| d.frame().size.height)
        })
        .unwrap_or_else(|| {
            MainThreadMarker::new()
                .and_then(|mtm| {
                    objc2_app_kit::NSScreen::mainScreen(mtm).map(|s| s.frame().size.height)
                })
                .unwrap_or(0.0)
        });

    let app = Rc::new(RefCell::new(App {
        window: window.clone_strong(),
        tray: tray.clone_strong(),
        keys,
        primary_height,
    }));
    APP.with(|a| *a.borrow_mut() = Some(app.clone()));
    app.borrow_mut().refresh();

    // Shortcut events arrive from Carbon on the main thread; hop through the event loop anyway.
    GlobalHotKeyEvent::set_event_handler(Some(|e: GlobalHotKeyEvent| {
        if e.state() != HotKeyState::Pressed {
            return;
        }
        let id = e.id();
        let _ = slint::invoke_from_event_loop(move || {
            APP.with(|a| {
                let Some(app) = a.borrow().as_ref().cloned() else {
                    return;
                };
                let (screen, window) = {
                    let b = app.borrow();
                    (b.keys.screen, b.keys.window)
                };
                if Some(id) == screen {
                    log("клавіша: знімок екрана");
                    run_capture_display(&app);
                } else if Some(id) == window {
                    log("клавіша: пікер");
                    run_capture_picker();
                }
            })
        });
    }));

    {
        let a = app.clone();
        window.on_request_capture(move || {
            let granted = unsafe { CGRequestScreenCaptureAccess() };
            log(format!("запит дозволу на запис екрана → {granted}"));
            if !granted {
                open_url(
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
                );
            }
            a.borrow_mut().refresh();
        });
    }
    window.on_open_capture_settings(|| {
        open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
    });
    window.on_open_keyboard_settings(|| {
        open_url("x-apple.systempreferences:com.apple.Keyboard-Settings.extension")
    });
    {
        let a = app.clone();
        window.on_recheck(move || {
            log("перевірка дозволів і клавіш");
            a.borrow_mut().refresh();
        });
    }
    {
        let a = app.clone();
        window.on_toggle_autostart(move || {
            log(toggle_autostart());
            a.borrow_mut().refresh();
        });
    }
    {
        let a = app.clone();
        tray.on_toggle_autostart(move || {
            log(toggle_autostart());
            a.borrow_mut().refresh();
        });
    }
    {
        let a = app.clone();
        window.on_shot_display(move || run_capture_display(&a));
    }
    {
        let a = app.clone();
        tray.on_shot_display(move || run_capture_display(&a));
    }
    window.on_shot_picker(run_capture_picker);
    tray.on_shot_picker(run_capture_picker);
    {
        let w = window.as_weak();
        let a = app.clone();
        tray.on_show_window(move || {
            a.borrow_mut().refresh();
            if let Some(w) = w.upgrade() {
                let _ = w.show();
                activate();
            }
        });
    }
    tray.on_quit(|| {
        log("вихід");
        let _ = slint::quit_event_loop();
    });

    // While the window is open, notice the user flipping system shortcuts or the permission.
    let poll = slint::Timer::default();
    {
        let a = app.clone();
        let w = window.as_weak();
        poll.start(
            slint::TimerMode::Repeated,
            Duration::from_secs(3),
            move || {
                if w.upgrade()
                    .map(|w| w.window().is_visible())
                    .unwrap_or(false)
                {
                    a.borrow_mut().refresh();
                }
            },
        );
    }

    let _ = tray.show();
    let _ = window.show();
    activate();
    log("готово: значок у рядку меню, вікно дозволів відкрито");
    if let Err(e) = slint::run_event_loop_until_quit() {
        log(format!("цикл подій: {e}"));
    }
}
