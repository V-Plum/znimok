//! An OS in memory: implements every platform trait with knobs for tests (displays, windows,
//! taken hotkeys, permission answers, picker answer) and inspection of what the app did
//! (registered hotkeys, tray states, clipboard, shared files, notifications).
//!
//! ```
//! use znimok_platform::{synthetic::SyntheticOs, CaptureTarget, CaptureOptions};
//! let os = SyntheticOs::windows_like();
//! let p = os.platform();
//! let d = &p.capture.displays().unwrap()[1];
//! let f = p.capture.capture(&CaptureTarget::Display { id: d.id.clone() }, &CaptureOptions::default()).unwrap();
//! assert_eq!((f.width, f.height), d.pixel_size());
//! ```
//!
//! Pixels are a pure function of (display, pixel x, pixel y) — see [`SyntheticOs::expected_pixel`] —
//! so region and window crops can be checked exactly.

use crate::frame::{ColorInfo, Frame, PixelFormat, Transfer};
use crate::geom::{Point, Rect};
use crate::keys::KeyCombo;
use crate::traits::*;
use crate::{Platform, PlatformError, Result};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct State {
    displays: Vec<DisplayInfo>,
    windows: Vec<WindowInfo>,
    cursor: Point,
    captures: u32,
    capture_needs: Option<Permission>,
    permissions: HashMap<Permission, PermissionState>,
    on_request: HashMap<Permission, PermissionState>,
    settings_opened: Vec<Permission>,
    picker_answer: Option<CaptureTarget>,
    picked: HashMap<u64, CaptureTarget>,
    hotkeys: BTreeMap<HotkeyId, KeyCombo>,
    taken_by_others: Vec<KeyCombo>,
    taken_by_system: Vec<KeyCombo>,
    hotkeys_paused: bool,
    hotkey_handler: Option<Arc<HotkeyHandler>>,
    tray_states: Vec<TrayState>,
    tray_tooltip: String,
    tray_menu: Vec<MenuEntry>,
    tray_handler: Option<Arc<TrayHandler>>,
    clipboard: Vec<ClipItem>,
    shared: Vec<(Vec<PathBuf>, Option<Rect>)>,
    shell: Vec<(&'static str, PathBuf)>,
    notifications: Vec<(String, String)>,
    autostart: bool,
    assoc: HashMap<String, AssocState>,
}

/// See the module docs. Cheap to clone (shared state).
#[derive(Clone, Default)]
pub struct SyntheticOs {
    s: Arc<Mutex<State>>,
}

fn display(
    id: &str,
    name: &str,
    bounds: Rect,
    scale: f32,
    ppu: f32,
    primary: bool,
    color: ColorInfo,
) -> DisplayInfo {
    let bar = (48.0 * scale / ppu) as u32;
    DisplayInfo {
        id: DisplayId(id.into()),
        name: name.into(),
        bounds,
        work_area: Rect::new(
            bounds.x,
            bounds.y,
            bounds.width,
            bounds.height.saturating_sub(bar),
        ),
        scale_factor: scale,
        pixels_per_unit: ppu,
        primary,
        refresh_hz: Some(60.0),
        color,
    }
}

impl SyntheticOs {
    /// Two monitors like a Windows desk: primary 1920×1080 at 100 %, and 2560×1440 at 150 % in
    /// HDR (SDR white 240 nits) to its right; desktop units = pixels. No permissions needed.
    pub fn windows_like() -> Self {
        let os = Self::default();
        {
            let mut s = os.lock();
            s.displays = vec![
                display(
                    "\\\\.\\DISPLAY1",
                    "Primary",
                    Rect::new(0, 0, 1920, 1080),
                    1.0,
                    1.0,
                    true,
                    ColorInfo::SDR,
                ),
                display(
                    "\\\\.\\DISPLAY2",
                    "HDR 150 %",
                    Rect::new(1920, -200, 2560, 1440),
                    1.5,
                    1.0,
                    false,
                    ColorInfo {
                        transfer: Transfer::ScRgb,
                        sdr_white_nits: 240.0,
                        hdr: true,
                    },
                ),
            ];
            s.windows = vec![
                win(
                    11,
                    "Notepad",
                    "notepad.exe",
                    Rect::new(200, 150, 800, 600),
                    "\\\\.\\DISPLAY1",
                    1.0,
                ),
                win(
                    12,
                    "Browser",
                    "chrome.exe",
                    Rect::new(100, 100, 1600, 900),
                    "\\\\.\\DISPLAY1",
                    1.0,
                ),
                win(
                    13,
                    "Editor",
                    "code.exe",
                    Rect::new(2200, 0, 1800, 1100),
                    "\\\\.\\DISPLAY2",
                    1.5,
                ),
            ];
        }
        os
    }

    /// A MacBook with Retina (1512×982 points, 2 pixels per point) and an external 2560×1440 at
    /// 1×; capture needs Screen Recording, which is not yet decided.
    pub fn mac_like() -> Self {
        let os = Self::default();
        {
            let mut s = os.lock();
            s.displays = vec![
                display(
                    "1",
                    "Built-in Retina",
                    Rect::new(0, 0, 1512, 982),
                    2.0,
                    2.0,
                    true,
                    ColorInfo {
                        transfer: Transfer::ExtendedLinear,
                        sdr_white_nits: 500.0,
                        hdr: true,
                    },
                ),
                display(
                    "2",
                    "External",
                    Rect::new(1512, 0, 2560, 1440),
                    1.0,
                    1.0,
                    false,
                    ColorInfo::SDR,
                ),
            ];
            s.windows = vec![
                win(
                    21,
                    "Finder",
                    "Finder",
                    Rect::new(100, 80, 700, 500),
                    "1",
                    2.0,
                ),
                win(
                    22,
                    "Safari",
                    "Safari",
                    Rect::new(1600, 50, 1200, 900),
                    "2",
                    1.0,
                ),
            ];
            s.capture_needs = Some(Permission::ScreenRecording);
            s.permissions
                .insert(Permission::ScreenRecording, PermissionState::NotDetermined);
            s.on_request
                .insert(Permission::ScreenRecording, PermissionState::Granted);
        }
        os
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.s.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// All traits backed by this one in-memory OS.
    pub fn platform(&self) -> Platform {
        let a = Arc::new(self.clone());
        Platform {
            capture: a.clone(),
            windows: a.clone(),
            cursor: a.clone(),
            hotkeys: a.clone(),
            tray: a.clone(),
            clipboard: a.clone(),
            share: a.clone(),
            shell: a.clone(),
            notifications: a.clone(),
            permissions: a.clone(),
            autostart: a.clone(),
            file_assoc: a,
        }
    }

    // --- knobs -------------------------------------------------------------------------------

    pub fn set_windows(&self, w: Vec<WindowInfo>) {
        self.lock().windows = w;
    }
    pub fn set_cursor(&self, p: Point) {
        self.lock().cursor = p;
    }
    pub fn set_permission(&self, p: Permission, s: PermissionState) {
        self.lock().permissions.insert(p, s);
    }
    /// What `request(p)` will answer.
    pub fn answer_request(&self, p: Permission, s: PermissionState) {
        self.lock().on_request.insert(p, s);
    }
    /// What the picker returns next (`None` = the user cancels).
    pub fn answer_picker(&self, t: Option<CaptureTarget>) {
        self.lock().picker_answer = t;
    }
    /// Pretend another program holds this combination.
    pub fn take_hotkey(&self, c: KeyCombo) {
        self.lock().taken_by_others.push(c);
    }
    /// Pretend the OS uses this combination.
    pub fn take_hotkey_by_system(&self, c: KeyCombo) {
        self.lock().taken_by_system.push(c);
    }

    // --- acting as the user ------------------------------------------------------------------

    /// Press a key combination: fires the handler if one of our hotkeys matches and we are not
    /// paused. Returns whether it fired.
    pub fn press(&self, c: KeyCombo) -> bool {
        let (hit, handler) = {
            let s = self.lock();
            let hit = (!s.hotkeys_paused)
                .then(|| s.hotkeys.iter().find(|(_, v)| **v == c).map(|(k, _)| *k))
                .flatten();
            (hit, s.hotkey_handler.clone())
        };
        match (hit, handler) {
            (Some(id), Some(h)) => {
                h(HotkeyEvent { id, pressed: true });
                h(HotkeyEvent { id, pressed: false });
                true
            }
            _ => false,
        }
    }

    pub fn tray_event(&self, e: TrayEvent) {
        let h = self.lock().tray_handler.clone();
        if let Some(h) = h {
            h(e);
        }
    }

    // --- inspection --------------------------------------------------------------------------

    pub fn captures(&self) -> u32 {
        self.lock().captures
    }
    pub fn registered_hotkeys(&self) -> BTreeMap<HotkeyId, KeyCombo> {
        self.lock().hotkeys.clone()
    }
    pub fn hotkeys_paused(&self) -> bool {
        self.lock().hotkeys_paused
    }
    pub fn tray_states(&self) -> Vec<TrayState> {
        self.lock().tray_states.clone()
    }
    pub fn tray_tooltip(&self) -> String {
        self.lock().tray_tooltip.clone()
    }
    pub fn tray_menu(&self) -> Vec<MenuEntry> {
        self.lock().tray_menu.clone()
    }
    pub fn clipboard(&self) -> Vec<ClipItem> {
        self.lock().clipboard.clone()
    }
    pub fn shared(&self) -> Vec<(Vec<PathBuf>, Option<Rect>)> {
        self.lock().shared.clone()
    }
    /// ("open" | "reveal", path) in call order.
    pub fn shell_calls(&self) -> Vec<(&'static str, PathBuf)> {
        self.lock().shell.clone()
    }
    pub fn notifications(&self) -> Vec<(String, String)> {
        self.lock().notifications.clone()
    }
    pub fn settings_opened(&self) -> Vec<Permission> {
        self.lock().settings_opened.clone()
    }

    /// The BGRA pixel a capture of display `index` has at pixel `(x, y)` of that display.
    pub fn expected_pixel(index: usize, x: u32, y: u32) -> [u8; 4] {
        [
            (x % 251) as u8,
            (y % 241) as u8,
            (64 * (index as u32 + 1) % 256) as u8,
            255,
        ]
    }

    /// The BGRA colour a window capture is filled with.
    pub fn window_pixel(id: WindowId) -> [u8; 4] {
        [(id.0 * 37 % 256) as u8, (id.0 * 91 % 256) as u8, 200, 255]
    }
}

fn win(id: u64, title: &str, app: &str, bounds: Rect, display: &str, scale: f32) -> WindowInfo {
    WindowInfo {
        id: WindowId(id),
        title: title.into(),
        app: app.into(),
        pid: 1000 + id as u32,
        bounds,
        display: Some(DisplayId(display.into())),
        scale_factor: scale,
        minimized: false,
        own: false,
    }
}

/// f32 → IEEE half bits for the small positive values the synthetic HDR frames use.
fn f16_bits(v: f32) -> u16 {
    if v <= 0.0 {
        return 0;
    }
    let b = v.to_bits();
    let exp = ((b >> 23) & 0xff) as i32 - 127 + 15;
    if exp <= 0 {
        return 0;
    }
    if exp >= 31 {
        return 0x7c00;
    }
    ((exp as u16) << 10) | ((b >> 13) & 0x3ff) as u16
}

fn fill(
    width: u32,
    height: u32,
    fmt: PixelFormat,
    color: ColorInfo,
    source: Rect,
    ppu: f32,
    px: impl Fn(u32, u32) -> [u8; 4],
) -> Result<Frame> {
    let bpp = fmt.bytes_per_pixel();
    let stride = (width * bpp).div_ceil(64) * 64; // padded rows, as real APIs have
    let mut data = vec![0u8; (stride * height) as usize];
    for y in 0..height {
        for x in 0..width {
            let p = px(x, y);
            let o = (y * stride + x * bpp) as usize;
            match fmt {
                PixelFormat::Bgra8 => data[o..o + 4].copy_from_slice(&p),
                _ => {
                    // scRGB-ish: channel/255 scaled so SDR white sits at sdr_white/80.
                    let k = color.sdr_white_nits / 80.0;
                    for (c, v) in [p[2], p[1], p[0], 255].into_iter().enumerate() {
                        let f = if c == 3 {
                            1.0
                        } else {
                            f32::from(v) / 255.0 * k
                        };
                        data[o + 2 * c..o + 2 * c + 2].copy_from_slice(&f16_bits(f).to_le_bytes());
                    }
                }
            }
        }
    }
    Frame {
        width,
        height,
        stride,
        format: fmt,
        color,
        source,
        scale: ppu,
        data,
    }
    .validate()
}

impl Capture for SyntheticOs {
    fn caps(&self) -> CaptureCaps {
        let s = self.lock();
        CaptureCaps {
            borderless: true,
            hdr: true,
            window_capture: true,
            needs_permission: s.capture_needs,
            system_picker: s.capture_needs.is_some(),
        }
    }

    fn displays(&self) -> Result<Vec<DisplayInfo>> {
        Ok(self.lock().displays.clone())
    }

    fn capture(&self, target: &CaptureTarget, opts: &CaptureOptions) -> Result<Frame> {
        let mut s = self.lock();
        // The picker's result works without the permission (as ScreenCaptureKit allows).
        if let Some(p) = s.capture_needs
            && s.permissions.get(&p) != Some(&PermissionState::Granted)
            && !matches!(target, CaptureTarget::Picked { .. })
        {
            return Err(PlatformError::PermissionDenied(p));
        }
        s.captures += 1;
        let target = match target {
            CaptureTarget::Picked { token } => s
                .picked
                .get(token)
                .cloned()
                .ok_or(PlatformError::NotFound(format!("вибір {token}")))?,
            t => t.clone(),
        };
        let fmt_for = |d: &DisplayInfo| {
            if d.color.hdr && opts.keep_hdr {
                PixelFormat::Rgba16Float
            } else {
                PixelFormat::Bgra8
            }
        };
        let color_for = |d: &DisplayInfo| {
            if d.color.hdr && opts.keep_hdr {
                d.color.clone()
            } else {
                ColorInfo::SDR
            }
        };
        match target {
            CaptureTarget::Display { id } => {
                let (i, d) = s
                    .displays
                    .iter()
                    .enumerate()
                    .find(|(_, d)| d.id == id)
                    .ok_or(PlatformError::NotFound(id.0.clone()))?;
                let (w, h) = d.pixel_size();
                fill(
                    w,
                    h,
                    fmt_for(d),
                    color_for(d),
                    d.bounds,
                    d.pixels_per_unit,
                    |x, y| Self::expected_pixel(i, x, y),
                )
            }
            CaptureTarget::Region { rect } => {
                let (i, d) = s
                    .displays
                    .iter()
                    .enumerate()
                    .find(|(_, d)| d.bounds.intersect(&rect) == Some(rect))
                    .ok_or(PlatformError::Unsupported("ділянка на кількох дисплеях"))?;
                let px = rect
                    .relative_to(d.bounds.origin())
                    .scaled(d.pixels_per_unit);
                fill(
                    px.width,
                    px.height,
                    fmt_for(d),
                    color_for(d),
                    rect,
                    d.pixels_per_unit,
                    |x, y| Self::expected_pixel(i, px.x as u32 + x, px.y as u32 + y),
                )
            }
            CaptureTarget::Window { id } => {
                let w = s
                    .windows
                    .iter()
                    .find(|w| w.id == id)
                    .ok_or(PlatformError::NotFound(format!("вікно {}", id.0)))?;
                if w.minimized {
                    return Err(PlatformError::NotFound(format!("вікно {} згорнуте", id.0)));
                }
                let d = w
                    .display
                    .as_ref()
                    .and_then(|di| s.displays.iter().find(|d| &d.id == di))
                    .unwrap_or(&s.displays[0]);
                let px = Rect::new(0, 0, w.bounds.width, w.bounds.height).scaled(d.pixels_per_unit);
                let c = Self::window_pixel(id);
                fill(
                    px.width,
                    px.height,
                    fmt_for(d),
                    color_for(d),
                    w.bounds,
                    d.pixels_per_unit,
                    |_, _| c,
                )
            }
            CaptureTarget::Picked { .. } => unreachable!(),
        }
    }

    fn pick(&self, done: PickCallback) {
        let answer = {
            let mut s = self.lock();
            match s.picker_answer.take() {
                Some(t) => {
                    let token = s.picked.len() as u64 + 1;
                    s.picked.insert(token, t);
                    Ok(CaptureTarget::Picked { token })
                }
                None => Err(PlatformError::Cancelled),
            }
        };
        done(answer);
    }
}

impl WindowList for SyntheticOs {
    fn windows(&self) -> Result<Vec<WindowInfo>> {
        Ok(self.lock().windows.clone())
    }
}

impl Cursor for SyntheticOs {
    fn position(&self) -> Result<Point> {
        Ok(self.lock().cursor)
    }
}

impl Hotkeys for SyntheticOs {
    fn register(&self, id: HotkeyId, combo: KeyCombo) -> Result<()> {
        let mut s = self.lock();
        let ours_elsewhere = s.hotkeys.iter().any(|(k, v)| *v == combo && *k != id);
        if ours_elsewhere
            || s.taken_by_others.contains(&combo)
            || s.taken_by_system.contains(&combo)
        {
            return Err(PlatformError::Busy(combo.to_string()));
        }
        s.hotkeys.insert(id, combo);
        Ok(())
    }

    fn unregister(&self, id: HotkeyId) -> Result<()> {
        self.lock().hotkeys.remove(&id);
        Ok(())
    }

    fn probe(&self, combo: KeyCombo) -> Availability {
        let s = self.lock();
        if s.taken_by_system.contains(&combo) {
            Availability::TakenBySystem
        } else if s.taken_by_others.contains(&combo) || s.hotkeys.values().any(|v| *v == combo) {
            Availability::Taken
        } else {
            Availability::Free
        }
    }

    fn set_paused(&self, paused: bool) -> Result<()> {
        self.lock().hotkeys_paused = paused;
        Ok(())
    }

    fn set_handler(&self, handler: HotkeyHandler) {
        self.lock().hotkey_handler = Some(Arc::new(handler));
    }
}

impl Tray for SyntheticOs {
    fn set_state(&self, state: TrayState) {
        self.lock().tray_states.push(state);
    }
    fn set_tooltip(&self, text: &str) {
        self.lock().tray_tooltip = text.into();
    }
    fn set_menu(&self, menu: Vec<MenuEntry>) {
        self.lock().tray_menu = menu;
    }
    fn set_handler(&self, handler: TrayHandler) {
        self.lock().tray_handler = Some(Arc::new(handler));
    }
}

impl Clipboard for SyntheticOs {
    fn write(&self, items: &[ClipItem]) -> Result<()> {
        self.lock().clipboard = items.to_vec();
        Ok(())
    }
    fn read(&self) -> Result<Vec<ClipItem>> {
        Ok(self.lock().clipboard.clone())
    }
}

impl Share for SyntheticOs {
    fn available(&self) -> bool {
        true
    }
    fn share(&self, files: &[PathBuf], anchor: Option<Rect>) -> Result<()> {
        self.lock().shared.push((files.to_vec(), anchor));
        Ok(())
    }
}

impl Shell for SyntheticOs {
    fn open(&self, path: &Path) -> Result<()> {
        self.lock().shell.push(("open", path.into()));
        Ok(())
    }
    fn reveal(&self, path: &Path) -> Result<()> {
        self.lock().shell.push(("reveal", path.into()));
        Ok(())
    }
}

impl Notifications for SyntheticOs {
    fn notify(&self, title: &str, body: &str) -> Result<()> {
        self.lock().notifications.push((title.into(), body.into()));
        Ok(())
    }
}

impl Permissions for SyntheticOs {
    fn status(&self, p: Permission) -> PermissionState {
        *self
            .lock()
            .permissions
            .get(&p)
            .unwrap_or(&PermissionState::NotNeeded)
    }
    fn request(&self, p: Permission) -> PermissionState {
        let mut s = self.lock();
        let cur = *s.permissions.get(&p).unwrap_or(&PermissionState::NotNeeded);
        if cur != PermissionState::NotDetermined {
            return cur; // like macOS: the prompt appears only once
        }
        let ans = *s.on_request.get(&p).unwrap_or(&PermissionState::Denied);
        s.permissions.insert(p, ans);
        ans
    }
    fn open_settings(&self, p: Permission) -> Result<()> {
        self.lock().settings_opened.push(p);
        Ok(())
    }
}

impl Autostart for SyntheticOs {
    fn is_enabled(&self) -> Result<bool> {
        Ok(self.lock().autostart)
    }
    fn set_enabled(&self, on: bool) -> Result<()> {
        self.lock().autostart = on;
        Ok(())
    }
}

impl FileAssoc for SyntheticOs {
    fn state(&self, ext: &str) -> Result<AssocState> {
        Ok(self
            .lock()
            .assoc
            .get(ext)
            .cloned()
            .unwrap_or(AssocState::None))
    }
    fn register(&self, ext: &str) -> Result<()> {
        self.lock().assoc.insert(ext.into(), AssocState::Ours);
        Ok(())
    }
    fn unregister(&self, ext: &str) -> Result<()> {
        self.lock().assoc.remove(ext);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conformance;
    use crate::keys::{Key, Modifiers};
    use std::sync::atomic::{AtomicU32, Ordering};

    fn px(f: &Frame, x: u32, y: u32) -> [u8; 4] {
        let o = (y * f.stride + x * 4) as usize;
        f.data[o..o + 4].try_into().unwrap()
    }

    #[test]
    fn both_presets_pass_conformance() {
        let w = SyntheticOs::windows_like();
        let p = w.platform();
        conformance::capture(p.capture.as_ref()).unwrap();
        conformance::windows(p.windows.as_ref()).unwrap();
        conformance::hotkeys(
            p.hotkeys.as_ref(),
            KeyCombo::new(Modifiers::CTRL | Modifiers::ALT, Key::F13),
        )
        .unwrap();
        conformance::clipboard(p.clipboard.as_ref()).unwrap();

        let m = SyntheticOs::mac_like();
        m.set_permission(Permission::ScreenRecording, PermissionState::Granted);
        let p = m.platform();
        conformance::capture(p.capture.as_ref()).unwrap();
        conformance::windows(p.windows.as_ref()).unwrap();
    }

    #[test]
    fn region_pixels_are_the_display_pixels() {
        let os = SyntheticOs::windows_like();
        let c = os.platform().capture;
        let opts = CaptureOptions {
            keep_hdr: false,
            ..Default::default()
        };
        // Second display starts at (1920, -200): a region at (2000, -150) is pixel (80, 50) there.
        let f = c
            .capture(
                &CaptureTarget::Region {
                    rect: Rect::new(2000, -150, 30, 20),
                },
                &opts,
            )
            .unwrap();
        assert_eq!((f.width, f.height), (30, 20));
        assert_eq!(px(&f, 0, 0), SyntheticOs::expected_pixel(1, 80, 50));
        assert_eq!(px(&f, 29, 19), SyntheticOs::expected_pixel(1, 109, 69));
        // Spanning two displays is refused.
        let e = c.capture(
            &CaptureTarget::Region {
                rect: Rect::new(1900, 0, 40, 10),
            },
            &opts,
        );
        assert!(matches!(e, Err(PlatformError::Unsupported(_))));
    }

    #[test]
    fn retina_region_is_in_pixels() {
        let os = SyntheticOs::mac_like();
        os.set_permission(Permission::ScreenRecording, PermissionState::Granted);
        let f = os
            .platform()
            .capture
            .capture(
                &CaptureTarget::Region {
                    rect: Rect::new(10, 10, 100, 50),
                },
                &CaptureOptions {
                    keep_hdr: false,
                    cursor: false,
                },
            )
            .unwrap();
        assert_eq!((f.width, f.height, f.scale), (200, 100, 2.0));
        assert_eq!(px(&f, 0, 0), SyntheticOs::expected_pixel(0, 20, 20));
    }

    #[test]
    fn hdr_display_gives_fp16_unless_asked_for_sdr() {
        let os = SyntheticOs::windows_like();
        let c = os.platform().capture;
        let id = CaptureTarget::Display {
            id: DisplayId("\\\\.\\DISPLAY2".into()),
        };
        let hdr = c.capture(&id, &CaptureOptions::default()).unwrap();
        assert_eq!(
            (hdr.format, hdr.color.transfer, hdr.color.sdr_white_nits),
            (PixelFormat::Rgba16Float, Transfer::ScRgb, 240.0)
        );
        let sdr = c
            .capture(
                &id,
                &CaptureOptions {
                    keep_hdr: false,
                    cursor: false,
                },
            )
            .unwrap();
        assert_eq!(sdr.format, PixelFormat::Bgra8);
        assert!(sdr.to_rgba8().is_some() && hdr.to_rgba8().is_none());
        assert_eq!(f16_bits(1.0), 0x3c00);
        assert_eq!(f16_bits(3.0), 0x4200);
    }

    #[test]
    fn mac_permission_flow_and_picker() {
        let os = SyntheticOs::mac_like();
        let p = os.platform();
        let t = CaptureTarget::Display {
            id: DisplayId("1".into()),
        };
        assert_eq!(
            p.capture.capture(&t, &Default::default()),
            Err(PlatformError::PermissionDenied(Permission::ScreenRecording))
        );
        // The picker works without the permission.
        os.answer_picker(Some(CaptureTarget::Window { id: WindowId(22) }));
        let got = Arc::new(Mutex::new(None));
        let g = got.clone();
        p.capture
            .pick(Box::new(move |r| *g.lock().unwrap() = Some(r)));
        let picked = got.lock().unwrap().take().unwrap().unwrap();
        let f = p.capture.capture(&picked, &Default::default()).unwrap();
        assert_eq!(px(&f, 5, 5), SyntheticOs::window_pixel(WindowId(22)));
        // Cancel.
        p.capture
            .pick(Box::new(|r| assert_eq!(r, Err(PlatformError::Cancelled))));
        // Prompt once: granted, later requests don't prompt again.
        assert_eq!(
            p.permissions.request(Permission::ScreenRecording),
            PermissionState::Granted
        );
        os.answer_request(Permission::ScreenRecording, PermissionState::Denied);
        assert_eq!(
            p.permissions.request(Permission::ScreenRecording),
            PermissionState::Granted
        );
        assert!(p.capture.capture(&t, &Default::default()).is_ok());
    }

    #[test]
    fn window_at_skips_minimized_and_own() {
        let os = SyntheticOs::windows_like();
        let mut w = os.platform().windows.windows().unwrap();
        let at = Point { x: 300, y: 300 };
        assert_eq!(
            os.platform().windows.window_at(at).unwrap().unwrap().title,
            "Notepad"
        );
        w[0].minimized = true;
        os.set_windows(w.clone());
        assert_eq!(
            os.platform().windows.window_at(at).unwrap().unwrap().title,
            "Browser"
        );
        w[1].own = true;
        os.set_windows(w);
        assert_eq!(os.platform().windows.window_at(at).unwrap(), None);
    }

    #[test]
    fn hotkeys_press_pause_and_conflicts() {
        let os = SyntheticOs::windows_like();
        let h = os.platform().hotkeys;
        let fired = Arc::new(AtomicU32::new(0));
        let f = fired.clone();
        h.set_handler(Box::new(move |e| {
            if e.pressed {
                f.fetch_add(e.id.0, Ordering::SeqCst);
            }
        }));
        let shot: KeyCombo = "Alt+Shift+3".parse().unwrap();
        h.register(HotkeyId(7), shot).unwrap();
        assert!(os.press(shot));
        assert_eq!(fired.load(Ordering::SeqCst), 7);
        h.set_paused(true).unwrap();
        assert!(!os.press(shot));
        h.set_paused(false).unwrap();
        assert!(os.press(shot));
        let taken: KeyCombo = "Ctrl+Alt+4".parse().unwrap();
        os.take_hotkey(taken);
        assert_eq!(h.probe(taken), Availability::Taken);
        assert!(matches!(
            h.register(HotkeyId(8), taken),
            Err(PlatformError::Busy(_))
        ));
        let sys: KeyCombo = "Meta+Shift+3".parse().unwrap();
        os.take_hotkey_by_system(sys);
        assert_eq!(h.probe(sys), Availability::TakenBySystem);
    }

    #[test]
    fn tray_share_shell_notify_autostart_assoc() {
        let os = SyntheticOs::windows_like();
        let p = os.platform();
        let got = Arc::new(Mutex::new(Vec::new()));
        let g = got.clone();
        p.tray
            .set_handler(Box::new(move |e| g.lock().unwrap().push(e)));
        p.tray.set_menu(vec![MenuEntry::Item {
            id: "quit".into(),
            label: "Вийти".into(),
            enabled: true,
            checked: None,
        }]);
        p.tray.set_state(TrayState::Recording { seconds: 3 });
        os.tray_event(TrayEvent::Menu("quit".into()));
        assert_eq!(*got.lock().unwrap(), vec![TrayEvent::Menu("quit".into())]);
        assert_eq!(os.tray_states(), vec![TrayState::Recording { seconds: 3 }]);
        assert_eq!(os.tray_menu().len(), 1);

        p.share
            .share(&[PathBuf::from("a.png")], Some(Rect::new(1, 2, 3, 4)))
            .unwrap();
        p.shell.reveal(Path::new("a.png")).unwrap();
        p.notifications.notify("Znimok", "Збережено").unwrap();
        assert_eq!(os.shared().len(), 1);
        assert_eq!(os.shell_calls(), vec![("reveal", PathBuf::from("a.png"))]);
        assert_eq!(os.notifications()[0].1, "Збережено");

        assert!(!p.autostart.is_enabled().unwrap());
        p.autostart.set_enabled(true).unwrap();
        assert!(p.autostart.is_enabled().unwrap());
        assert_eq!(p.file_assoc.state("znimok").unwrap(), AssocState::None);
        p.file_assoc.register("znimok").unwrap();
        assert_eq!(p.file_assoc.state("znimok").unwrap(), AssocState::Ours);
    }

    #[test]
    fn display_info_serializes_for_agents() {
        let d = &SyntheticOs::windows_like()
            .platform()
            .capture
            .displays()
            .unwrap()[1];
        let j = serde_json::to_value(d).unwrap();
        assert_eq!(j["scale_factor"], 1.5);
        assert_eq!(j["color"]["transfer"], "sc_rgb");
        let t = serde_json::to_value(CaptureTarget::Window { id: WindowId(5) }).unwrap();
        assert_eq!(t, serde_json::json!({"kind": "window", "id": 5}));
    }
}
