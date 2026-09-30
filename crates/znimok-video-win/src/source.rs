//! Frame sources — what the screen or the window looks like right now, on the GPU.
//!
//! Every source copies its newest picture into a slot of the shared [`FramePool`] (a small ring
//! of textures wgpu can read) and hands the recorder a [`GpuFrame`]: which slot, the fence
//! value the copy signalled, how the slot maps onto the video and what to draw on top. The sink
//! renders from the slot and tells the pool when the GPU has read it, so a copy never lands on a
//! slot still being read.
//!
//! - [`WgcSource`] — Windows.Graphics.Capture, a display or a window (frames arrive on the
//!   compositor's schedule; a still screen sends none — the recorder repeats the last one);
//!   FP16 scRGB on an HDR display or for a window (it may cross displays), BGRA8 on SDR.
//! - [`DdaSource`] — DXGI Desktop Duplication of a display (the other API: works where WGC does
//!   not and gives 10-bit PQ frames on HDR10 desktops); refused in RDP sessions.
//! - [`crate::synthetic::SyntheticSource`] — a pattern with the slot number, for the tests.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_SHADER_RESOURCE, D3D11_BOX, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020, DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM,
    DXGI_FORMAT_R10G10B10A2_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO,
    IDXGIFactory1, IDXGIOutput6, IDXGIOutputDuplication, IDXGIResource,
};
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::core::{IInspectable, Interface};
use znimok_platform::{DisplayId, Rect, WindowId};
use znimok_video::settings::{PixelRect, even_size, plan_region};
use znimok_video::traits::{FrameSource, Pulled};
use znimok_video::{Result, VideoError};

use crate::interop::{Bridge, Gpu, SharedTexture};
use crate::mf::err;
use crate::shader::{FrameGeometry, Overlay};

/// What to record.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// A display, or a rectangle of it (desktop units = physical pixels on Windows).
    Display { id: DisplayId, region: Option<Rect> },
    /// A window followed as it moves; the video size is its size at the start.
    Window { id: WindowId },
}

/// The recording plan for a target (`VidFitRect` and friends, LH §2.6).
#[derive(Clone, Debug)]
pub struct Plan {
    pub monitor: HMONITOR,
    pub monitor_bounds: Rect,
    pub hdr: bool,
    pub white: f32,
    /// Source rectangle relative to the monitor (display targets).
    pub crop: (i32, i32, u32, u32),
    /// Video size (even, ≥ 64).
    pub out: (u32, u32),
    /// The whole display (no frame indicator, no border — decided before the parity fit).
    pub whole: bool,
    pub hwnd: Option<HWND>,
}

/// Resolve a target against the current desktop.
pub fn plan(target: &Target) -> Result<Plan> {
    znimok_win::raw::com_thread();
    let monitors = znimok_win::raw::monitors();
    match target {
        Target::Display { id, region } => {
            let m = monitors
                .iter()
                .find(|m| &m.info.id == id)
                .ok_or_else(|| VideoError::Screen(format!("немає дисплея {}", id.0)))?;
            let b = m.info.bounds;
            let mon = PixelRect::new(b.x, b.y, b.right(), b.bottom());
            let sel = match region {
                Some(r) => PixelRect::new(r.x, r.y, r.right(), r.bottom()),
                None => mon,
            };
            let (r, whole) = plan_region(sel, mon);
            if r.width() < 2 || r.height() < 2 {
                return Err(VideoError::Invalid("ділянка поза дисплеєм".into()));
            }
            Ok(Plan {
                monitor: m.handle,
                monitor_bounds: b,
                hdr: m.info.color.hdr,
                white: m.info.color.sdr_white_nits,
                crop: (
                    r.left - b.x,
                    r.top - b.y,
                    r.width() as u32,
                    r.height() as u32,
                ),
                out: (r.width() as u32, r.height() as u32),
                whole,
                hwnd: None,
            })
        }
        Target::Window { id } => {
            let h = znimok_win::raw::hwnd(*id);
            if !znimok_win::raw::is_alive(h) {
                return Err(VideoError::Screen(format!("вікно 0x{:X} зникло", id.0)));
            }
            if znimok_win::raw::is_minimized(h) {
                return Err(VideoError::Screen(format!("вікно 0x{:X} згорнуте", id.0)));
            }
            let b = znimok_win::raw::dwm_bounds(h)
                .ok_or_else(|| VideoError::Screen("вікно без меж".into()))?;
            let did = znimok_win::raw::display_of(h);
            let m = monitors
                .iter()
                .find(|m| Some(&m.info.id) == did.as_ref())
                .or_else(|| monitors.first())
                .ok_or_else(|| VideoError::Screen("немає дисплеїв".into()))?;
            let (w, h2) = even_size(b.width, b.height);
            let (w, h2) = (w.max(64), h2.max(64));
            Ok(Plan {
                monitor: m.handle,
                monitor_bounds: m.info.bounds,
                hdr: m.info.color.hdr,
                white: m.info.color.sdr_white_nits,
                crop: (0, 0, b.width, b.height),
                out: (w, h2),
                whole: false,
                hwnd: Some(h),
            })
        }
    }
}

/// Pixel format of the pool slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoolFormat {
    Bgra8,
    Rgba16F,
    Rgb10A2,
}

impl PoolFormat {
    pub fn dxgi(self) -> DXGI_FORMAT {
        match self {
            Self::Bgra8 => DXGI_FORMAT_B8G8R8A8_UNORM,
            Self::Rgba16F => DXGI_FORMAT_R16G16B16A16_FLOAT,
            Self::Rgb10A2 => DXGI_FORMAT_R10G10B10A2_UNORM,
        }
    }
    pub fn wgpu(self) -> wgpu::TextureFormat {
        match self {
            Self::Bgra8 => wgpu::TextureFormat::Bgra8Unorm,
            Self::Rgba16F => wgpu::TextureFormat::Rgba16Float,
            Self::Rgb10A2 => wgpu::TextureFormat::Rgb10a2Unorm,
        }
    }
    pub fn bytes_per_pixel(self) -> u32 {
        match self {
            Self::Rgba16F => 8,
            _ => 4,
        }
    }
    fn from_dxgi(f: DXGI_FORMAT) -> Self {
        if f == DXGI_FORMAT_R16G16B16A16_FLOAT {
            Self::Rgba16F
        } else if f == DXGI_FORMAT_R10G10B10A2_UNORM {
            Self::Rgb10A2
        } else {
            Self::Bgra8
        }
    }
}

/// One frame for the sink.
#[derive(Clone, Debug)]
pub struct GpuFrame {
    pub slot: usize,
    /// D3D11 fence value after the copy into the slot.
    pub ready: u64,
    pub geometry: FrameGeometry,
    pub overlay: Overlay,
}

pub struct PoolSlot {
    pub tex: SharedTexture,
    /// wgpu fence value after the last read of this slot.
    pub last_read: u64,
}

/// The ring of shared textures the sources copy into.
pub struct FramePool {
    pub slots: Vec<PoolSlot>,
    next: usize,
    pub width: u32,
    pub height: u32,
    pub format: PoolFormat,
    gpu: Rc<Gpu>,
    bridge: Rc<Bridge>,
}

pub const POOL_SLOTS: usize = 3;
pub type SharedPool = Rc<RefCell<FramePool>>;

impl FramePool {
    pub fn new(
        gpu: Rc<Gpu>,
        bridge: Rc<Bridge>,
        width: u32,
        height: u32,
        format: PoolFormat,
    ) -> Result<SharedPool> {
        let mut p = Self {
            slots: Vec::new(),
            next: 0,
            width: 0,
            height: 0,
            format,
            gpu,
            bridge,
        };
        p.ensure(width, height)?;
        Ok(Rc::new(RefCell::new(p)))
    }

    /// Slots of at least this size (a window grew): all of them are made anew.
    pub fn ensure(&mut self, width: u32, height: u32) -> Result<()> {
        if width <= self.width && height <= self.height {
            return Ok(());
        }
        let (w, h) = (width.max(self.width), height.max(self.height));
        let mut slots = Vec::with_capacity(POOL_SLOTS);
        for _ in 0..POOL_SLOTS {
            let tex = self
                .bridge
                .shared_texture(
                    &self.gpu,
                    w,
                    h,
                    self.format.dxgi(),
                    D3D11_BIND_SHADER_RESOURCE.0 as u32,
                    self.format.wgpu(),
                    wgpu::TextureUsages::TEXTURE_BINDING,
                    "frame slot",
                )
                .map_err(VideoError::Screen)?;
            slots.push(PoolSlot { tex, last_read: 0 });
        }
        // The old slots may still be read by the GPU: wgpu keeps them alive until it is done.
        self.slots = slots;
        self.next = 0;
        self.width = w;
        self.height = h;
        Ok(())
    }

    fn take_slot(&mut self) -> usize {
        let i = self.next;
        self.next = (i + 1) % self.slots.len();
        self.bridge.wait_d3d11(self.slots[i].last_read);
        i
    }

    /// Copy the top-left `w × h` of a D3D11 texture into the next slot.
    pub fn put(&mut self, src: &ID3D11Texture2D, sub: u32, w: u32, h: u32) -> (usize, u64) {
        let i = self.take_slot();
        let bx = D3D11_BOX {
            left: 0,
            top: 0,
            front: 0,
            right: w.min(self.width),
            bottom: h.min(self.height),
            back: 1,
        };
        // SAFETY: both textures belong to the bridge's device; the box is inside both.
        unsafe {
            self.bridge.ctx.CopySubresourceRegion(
                &self.slots[i].tex.tex11,
                0,
                0,
                0,
                0,
                src,
                sub,
                Some(&bx),
            );
        }
        (i, self.bridge.signal_d3d11())
    }

    /// Upload CPU pixels (rows `pitch` bytes apart) into the next slot — the synthetic source.
    pub fn put_cpu(&mut self, data: &[u8], pitch: u32, w: u32, h: u32) -> (usize, u64) {
        let i = self.take_slot();
        let (w, h) = (w.min(self.width), h.min(self.height));
        let bx = D3D11_BOX {
            left: 0,
            top: 0,
            front: 0,
            right: w,
            bottom: h,
            back: 1,
        };
        // SAFETY: `data` holds `h` rows of `pitch` bytes; the box is inside the slot.
        unsafe {
            self.bridge.ctx.UpdateSubresource(
                &self.slots[i].tex.tex11,
                0,
                Some(&bx),
                data.as_ptr().cast(),
                pitch,
                0,
            );
        }
        (i, self.bridge.signal_d3d11())
    }

    /// The sink read slot `slot` in a submit that signals `done`.
    pub fn mark_read(&mut self, slot: usize, done: u64) {
        if let Some(s) = self.slots.get_mut(slot) {
            s.last_read = s.last_read.max(done);
        }
    }
}

/// Supplies the overlay for a slot (the cursor and the clicks at that moment) — ZK-90.
/// Called with the slot and where the desktop is in the video at that moment.
pub type OverlayFn = Box<dyn FnMut(i64, &crate::shader::DesktopMap) -> Overlay + Send>;

// ---------------------------------------------------------------------------------------------
// WGC

/// How often a window's display (white level) and liveness are checked.
const WINDOW_CHECK: Duration = Duration::from_millis(500);

/// A window that sent no frame this long after the start gets one from `PrintWindow` (ZK-193):
/// WGC sends a window's frame only when it presents, and a still window never does. The
/// recorder repeats that frame until the window changes.
const FIRST_FRAME_WAIT: Duration = Duration::from_millis(100);

pub struct WgcSource {
    pool: SharedPool,
    item: GraphicsCaptureItem,
    frames: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    d3d: IDirect3DDevice,
    arrived: Arc<(Mutex<bool>, Condvar)>,
    closed: Arc<AtomicBool>,
    token: i64,
    size: SizeInt32,
    format: DirectXPixelFormat,
    plan: Plan,
    mode: u32,
    white: f32,
    have: bool,
    current: Option<GpuFrame>,
    last_check: Instant,
    overlay: Option<OverlayFn>,
    opened: Instant,
    /// The `PrintWindow` frame was tried (once, ZK-193).
    printed: bool,
    /// Tests only (`ZNIMOK_TEST_WGC_SILENT`): WGC's frames are ignored, as from a window that
    /// never presents, so the `PrintWindow` path is what records.
    silent: bool,
}

impl WgcSource {
    pub fn open(_gpu: &Gpu, bridge: &Bridge, pool: SharedPool, plan: Plan) -> Result<Self> {
        znimok_win::raw::com_thread();
        if !znimok_win::raw::wgc_supported() {
            return Err(VideoError::Screen("WGC недоступний у цьому сеансі".into()));
        }
        let _ = znimok_win::raw::borderless();
        let s = |e: String| VideoError::Screen(e);
        let dxgi: IDXGIDevice = bridge
            .device
            .cast()
            .map_err(err("IDXGIDevice"))
            .map_err(s)?;
        // SAFETY: a valid DXGI device of the bridge.
        let d3d: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }
            .map_err(err("CreateDirect3D11DeviceFromDXGIDevice"))
            .map_err(s)?
            .cast()
            .map_err(err("IDirect3DDevice"))
            .map_err(s)?;
        let interop = windows_core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
            .map_err(err("IGraphicsCaptureItemInterop"))
            .map_err(s)?;
        // SAFETY: handles from the monitor / window lists; a stale one fails the call.
        let item: GraphicsCaptureItem = unsafe {
            match plan.hwnd {
                Some(h) => interop.CreateForWindow(h),
                None => interop.CreateForMonitor(plan.monitor),
            }
        }
        .map_err(err("GraphicsCaptureItem"))
        .map_err(s)?;
        let size = item.Size().map_err(err("Size")).map_err(s)?;
        // A window may wander between SDR and HDR displays: FP16 always (as LH); a display:
        // FP16 only when it is HDR (BGRA8 is exact and half the bandwidth on SDR).
        let fp16 = plan.hwnd.is_some() || plan.hdr;
        let format = if fp16 {
            DirectXPixelFormat::R16G16B16A16Float
        } else {
            DirectXPixelFormat::B8G8R8A8UIntNormalized
        };
        {
            let mut p = pool.borrow_mut();
            if p.format
                != if fp16 {
                    PoolFormat::Rgba16F
                } else {
                    PoolFormat::Bgra8
                }
            {
                return Err(VideoError::Invalid(
                    "формат пулу не збігається з WGC".into(),
                ));
            }
            p.ensure(size.Width.max(1) as u32, size.Height.max(1) as u32)?;
        }
        let frames = Direct3D11CaptureFramePool::CreateFreeThreaded(&d3d, format, 2, size)
            .map_err(err("CreateFreeThreaded"))
            .map_err(s)?;
        let session = frames
            .CreateCaptureSession(&item)
            .map_err(err("CreateCaptureSession"))
            .map_err(s)?;
        let _ = session.SetIsBorderRequired(false);
        let _ = session.SetIsCursorCaptureEnabled(false);
        let arrived = Arc::new((Mutex::new(false), Condvar::new()));
        let a2 = arrived.clone();
        let token = frames
            .FrameArrived(
                &TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new(move |_, _| {
                    let (m, cv) = &*a2;
                    *m.lock().unwrap_or_else(|e| e.into_inner()) = true;
                    cv.notify_one();
                    Ok(())
                }),
            )
            .map_err(err("FrameArrived"))
            .map_err(s)?;
        let closed = Arc::new(AtomicBool::new(false));
        let c2 = closed.clone();
        let _ = item.Closed(
            &TypedEventHandler::<GraphicsCaptureItem, IInspectable>::new(move |_, _| {
                c2.store(true, Ordering::SeqCst);
                Ok(())
            }),
        );
        session
            .StartCapture()
            .map_err(err("StartCapture"))
            .map_err(s)?;
        let mode = if fp16 { 1 } else { 0 };
        Ok(Self {
            pool,
            item,
            frames,
            session,
            d3d,
            arrived,
            closed,
            token,
            size,
            format,
            white: plan.white,
            plan,
            mode,
            have: false,
            current: None,
            last_check: Instant::now(),
            overlay: None,
            opened: Instant::now(),
            printed: false,
            silent: std::env::var_os("ZNIMOK_TEST_WGC_SILENT").is_some(),
        })
    }

    pub fn with_overlay(mut self, f: OverlayFn) -> Self {
        self.overlay = Some(f);
        self
    }

    pub fn api(&self) -> &'static str {
        "WGC"
    }

    /// The newest frame of the pool, older ones closed.
    fn drain(&self) -> Option<windows::Graphics::Capture::Direct3D11CaptureFrame> {
        if self.silent {
            while let Ok(f) = self.frames.TryGetNextFrame() {
                let _ = f.Close();
            }
            return None;
        }
        let mut last = None;
        while let Ok(f) = self.frames.TryGetNextFrame() {
            if let Some(prev) = last.replace(f) {
                let _ = prev.Close();
            }
        }
        last
    }

    fn take(&mut self, frame: &windows::Graphics::Capture::Direct3D11CaptureFrame) -> Result<()> {
        let s = |e: String| VideoError::Screen(e);
        let content = frame.ContentSize().map_err(err("ContentSize")).map_err(s)?;
        let (cw, ch) = (content.Width.max(1) as u32, content.Height.max(1) as u32);
        let access: IDirect3DDxgiInterfaceAccess = frame
            .Surface()
            .map_err(err("Surface"))
            .map_err(s)?
            .cast()
            .map_err(err("IDirect3DDxgiInterfaceAccess"))
            .map_err(s)?;
        // SAFETY: the frame surface is a D3D11 texture on the bridge's device.
        let tex: ID3D11Texture2D = unsafe { access.GetInterface() }
            .map_err(err("GetInterface"))
            .map_err(s)?;
        let mut pool = self.pool.borrow_mut();
        pool.ensure(cw, ch)?;
        let (slot, ready) = pool.put(&tex, 0, cw, ch);
        drop(pool);
        // The item changed size (a window resized): the pool follows it (LH §7 item 32 — the
        // "have a frame" flag is not reset).
        if (content.Width != self.size.Width || content.Height != self.size.Height)
            && self
                .frames
                .Recreate(&self.d3d, self.format, 2, content)
                .is_ok()
        {
            self.size = content;
        }
        let crop = match self.plan.hwnd {
            Some(_) => (0, 0, cw, ch),
            None => self.plan.crop,
        };
        self.set_current(slot, ready, crop);
        Ok(())
    }

    /// The window's picture from `PrintWindow` as the first frame (ZK-193), in the pool's
    /// format: a window's slots are FP16 scRGB, where SDR white is `white / 80`.
    fn print(&mut self) -> Result<()> {
        let h = self
            .plan
            .hwnd
            .ok_or_else(|| VideoError::Screen("не вікно".into()))?;
        let (w, hh, bgra) = znimok_win::raw::print_window(h)
            .ok_or_else(|| VideoError::Screen("PrintWindow не дав картинки".into()))?;
        let mut pool = self.pool.borrow_mut();
        pool.ensure(w, hh)?;
        let (data, bpp) = match pool.format {
            PoolFormat::Rgba16F => {
                let k = self.white / 80.0;
                let lut: Vec<[u8; 2]> = (0..256)
                    .map(|v| f16_bits(srgb_to_linear(v as f32 / 255.0) * k).to_le_bytes())
                    .collect();
                let one = f16_bits(1.0).to_le_bytes();
                let mut d = Vec::with_capacity(bgra.len() * 2);
                for p in bgra.as_chunks::<4>().0 {
                    for c in [
                        lut[p[2] as usize],
                        lut[p[1] as usize],
                        lut[p[0] as usize],
                        one,
                    ] {
                        d.extend_from_slice(&c);
                    }
                }
                (d, 8)
            }
            _ => (bgra, 4),
        };
        let (slot, ready) = pool.put_cpu(&data, w * bpp, w, hh);
        drop(pool);
        self.set_current(slot, ready, (0, 0, w, hh));
        Ok(())
    }

    fn set_current(&mut self, slot: usize, ready: u64, crop: (i32, i32, u32, u32)) {
        let overlay = self.current.take().map(|c| c.overlay).unwrap_or_default();
        self.current = Some(GpuFrame {
            slot,
            ready,
            geometry: FrameGeometry {
                mode: self.mode,
                white: self.white,
                crop,
                out: self.plan.out,
            },
            overlay,
        });
        self.have = true;
    }

    /// Every 500 ms: the window's display may have changed (its SDR white with it), the window
    /// may be gone.
    fn check_window(&mut self) -> bool {
        if self.last_check.elapsed() < WINDOW_CHECK {
            return true;
        }
        self.last_check = Instant::now();
        let Some(h) = self.plan.hwnd else {
            return true;
        };
        if !znimok_win::raw::is_alive(h) {
            return false;
        }
        if let Some(id) = znimok_win::raw::display_of(h)
            && let Some(m) = znimok_win::raw::monitors()
                .into_iter()
                .find(|m| m.info.id == id)
        {
            self.white = m.info.color.sdr_white_nits;
        }
        true
    }
}

impl Drop for WgcSource {
    fn drop(&mut self) {
        let _ = self.frames.RemoveFrameArrived(self.token);
        let _ = self.session.Close();
        let _ = self.frames.Close();
        let _ = &self.item;
    }
}

impl FrameSource for WgcSource {
    type Frame = GpuFrame;

    fn pull(&mut self, wait: Duration) -> Result<Pulled> {
        let deadline = Instant::now() + wait;
        loop {
            if let Some(f) = self.drain() {
                let r = self.take(&f);
                let _ = f.Close();
                r?;
                return Ok(Pulled::Frame);
            }
            if self.closed.load(Ordering::SeqCst) || !self.check_window() {
                return Ok(Pulled::Closed);
            }
            // A window that has not presented since the start: its picture once, by other means.
            if !self.have
                && !self.printed
                && self.plan.hwnd.is_some()
                && self.opened.elapsed() >= FIRST_FRAME_WAIT
            {
                self.printed = true;
                if self.print().is_ok() {
                    return Ok(Pulled::Frame);
                }
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(Pulled::Unchanged);
            }
            let (m, cv) = &*self.arrived;
            let mut flag = m.lock().unwrap_or_else(|e| e.into_inner());
            if !*flag {
                let (g, _) = cv
                    .wait_timeout(flag, deadline - now)
                    .unwrap_or_else(|e| e.into_inner());
                flag = g;
            }
            *flag = false;
        }
    }

    fn has_frame(&self) -> bool {
        self.have
    }

    fn frame_for_slot(&mut self, slot: i64) -> Result<&GpuFrame> {
        let c = self
            .current
            .as_mut()
            .ok_or_else(|| VideoError::Screen("ще немає кадру".into()))?;
        c.geometry.white = self.white;
        if let Some(f) = self.overlay.as_mut() {
            // A window's texture starts at its DWM bounds (it moves); a display's at its corner.
            let origin = match self.plan.hwnd {
                Some(h) => znimok_win::raw::dwm_bounds(h).map_or((0, 0), |b| (b.x, b.y)),
                None => (self.plan.monitor_bounds.x, self.plan.monitor_bounds.y),
            };
            let map = crate::shader::DesktopMap {
                origin,
                geometry: c.geometry,
            };
            c.overlay = f(slot, &map);
        }
        Ok(c)
    }
}

// ---------------------------------------------------------------------------------------------
// Desktop Duplication

/// How long a lost duplication (UAC, mode change, lock screen) waits before it is reopened.
const DDA_RETRY: Duration = Duration::from_millis(250);

pub struct DdaSource {
    pool: SharedPool,
    bridge: Rc<Bridge>,
    output: IDXGIOutput6,
    dup: Option<IDXGIOutputDuplication>,
    retry_at: Instant,
    plan: Plan,
    mode: u32,
    have: bool,
    current: Option<GpuFrame>,
    overlay: Option<OverlayFn>,
}

impl DdaSource {
    /// The pool format the display's duplication will deliver (opens and closes one to ask).
    pub fn probe_format(bridge: &Bridge, monitor: HMONITOR) -> Result<PoolFormat> {
        let out = Self::find_output(monitor)?;
        let dup = Self::duplicate(bridge, &out)?;
        // SAFETY: a description read on a live duplication.
        let f = unsafe { dup.GetDesc() }.ModeDesc.Format;
        Ok(PoolFormat::from_dxgi(f))
    }

    fn find_output(monitor: HMONITOR) -> Result<IDXGIOutput6> {
        // SAFETY: plain DXGI enumeration.
        unsafe {
            let f = CreateDXGIFactory1::<IDXGIFactory1>()
                .map_err(err("CreateDXGIFactory1"))
                .map_err(VideoError::Screen)?;
            let mut a = 0;
            while let Ok(ad) = f.EnumAdapters1(a) {
                a += 1;
                let mut o = 0;
                while let Ok(out) = ad.EnumOutputs(o) {
                    o += 1;
                    if out.GetDesc().is_ok_and(|d| d.Monitor == monitor) {
                        return out
                            .cast::<IDXGIOutput6>()
                            .map_err(err("IDXGIOutput6"))
                            .map_err(VideoError::Screen);
                    }
                }
            }
            Err(VideoError::Screen(
                "DXGI: немає виходу для цього монітора".into(),
            ))
        }
    }

    fn duplicate(bridge: &Bridge, out: &IDXGIOutput6) -> Result<IDXGIOutputDuplication> {
        let formats = [
            DXGI_FORMAT_R16G16B16A16_FLOAT,
            DXGI_FORMAT_R10G10B10A2_UNORM,
            DXGI_FORMAT_B8G8R8A8_UNORM,
        ];
        // SAFETY: the device belongs to the output's adapter (checked by the caller's plan).
        unsafe { out.DuplicateOutput1(&bridge.device, 0, &formats) }
            .map_err(err("DuplicateOutput1"))
            .map_err(VideoError::Screen)
    }

    pub fn open(bridge: Rc<Bridge>, pool: SharedPool, plan: Plan) -> Result<Self> {
        znimok_win::raw::com_thread();
        let output = Self::find_output(plan.monitor)?;
        let dup = Self::duplicate(&bridge, &output)?;
        // SAFETY: description reads on live objects.
        let (fmt, cs) = unsafe {
            (
                dup.GetDesc().ModeDesc.Format,
                output.GetDesc1().map(|d| d.ColorSpace).ok(),
            )
        };
        let pf = PoolFormat::from_dxgi(fmt);
        let mode = match pf {
            PoolFormat::Rgba16F => 1,
            PoolFormat::Rgb10A2 if cs == Some(DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020) => 2,
            _ => 0,
        };
        {
            let mut p = pool.borrow_mut();
            if p.format != pf {
                return Err(VideoError::Invalid(
                    "формат пулу не збігається з дублюванням".into(),
                ));
            }
            p.ensure(plan.monitor_bounds.width, plan.monitor_bounds.height)?;
        }
        Ok(Self {
            pool,
            bridge,
            output,
            dup: Some(dup),
            retry_at: Instant::now(),
            plan,
            mode,
            have: false,
            current: None,
            overlay: None,
        })
    }

    pub fn with_overlay(mut self, f: OverlayFn) -> Self {
        self.overlay = Some(f);
        self
    }

    pub fn api(&self) -> &'static str {
        "Desktop Duplication"
    }

    fn lost(&mut self) {
        self.dup = None;
        self.retry_at = Instant::now() + DDA_RETRY;
    }
}

impl FrameSource for DdaSource {
    type Frame = GpuFrame;

    fn pull(&mut self, wait: Duration) -> Result<Pulled> {
        if self.dup.is_none() {
            if Instant::now() < self.retry_at {
                std::thread::sleep(wait.min(DDA_RETRY));
                return Ok(Pulled::Unchanged);
            }
            match Self::duplicate(&self.bridge, &self.output) {
                Ok(d) => self.dup = Some(d),
                Err(_) => {
                    self.retry_at = Instant::now() + DDA_RETRY;
                    return Ok(Pulled::Unchanged);
                }
            }
        }
        let dup = self.dup.clone().expect("opened above");
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut res: Option<IDXGIResource> = None;
        // SAFETY: out-pointers to locals; the frame is released after the copy.
        unsafe {
            match dup.AcquireNextFrame(wait.as_millis().min(1000) as u32, &mut info, &mut res) {
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(Pulled::Unchanged),
                Err(e) => {
                    // ACCESS_LOST and everything else: reopen later, repeat the last frame now.
                    let _ = e.code() == DXGI_ERROR_ACCESS_LOST;
                    self.lost();
                    return Ok(Pulled::Unchanged);
                }
                Ok(()) => {}
            }
            // The first frame after DuplicateOutput is empty (LH §7 item 27).
            if info.AccumulatedFrames == 0 && info.LastPresentTime == 0 {
                let _ = dup.ReleaseFrame();
                return Ok(Pulled::Unchanged);
            }
            let tex = match res.and_then(|r| r.cast::<ID3D11Texture2D>().ok()) {
                Some(t) => t,
                None => {
                    let _ = dup.ReleaseFrame();
                    return Ok(Pulled::Unchanged);
                }
            };
            let (slot, ready) = {
                let mut p = self.pool.borrow_mut();
                let (w, h) = (p.width, p.height);
                p.put(&tex, 0, w, h)
            };
            let _ = dup.ReleaseFrame();
            let overlay = self.current.take().map(|c| c.overlay).unwrap_or_default();
            self.current = Some(GpuFrame {
                slot,
                ready,
                geometry: FrameGeometry {
                    mode: self.mode,
                    white: self.plan.white,
                    crop: self.plan.crop,
                    out: self.plan.out,
                },
                overlay,
            });
            self.have = true;
        }
        Ok(Pulled::Frame)
    }

    fn has_frame(&self) -> bool {
        self.have
    }

    fn frame_for_slot(&mut self, slot: i64) -> Result<&GpuFrame> {
        let c = self
            .current
            .as_mut()
            .ok_or_else(|| VideoError::Screen("ще немає кадру".into()))?;
        if let Some(f) = self.overlay.as_mut() {
            let map = crate::shader::DesktopMap {
                origin: (self.plan.monitor_bounds.x, self.plan.monitor_bounds.y),
                geometry: c.geometry,
            };
            c.overlay = f(slot, &map);
        }
        Ok(c)
    }
}

// ---------------------------------------------------------------------------------------------

/// The source of a recording, whichever API it is.
pub enum Source {
    Wgc(WgcSource),
    Dda(DdaSource),
    Synthetic(crate::synthetic::SyntheticSource),
}

impl Source {
    pub fn api(&self) -> &'static str {
        match self {
            Self::Wgc(s) => s.api(),
            Self::Dda(s) => s.api(),
            Self::Synthetic(_) => "synthetic",
        }
    }
}

impl FrameSource for Source {
    type Frame = GpuFrame;

    fn pull(&mut self, wait: Duration) -> Result<Pulled> {
        match self {
            Self::Wgc(s) => s.pull(wait),
            Self::Dda(s) => s.pull(wait),
            Self::Synthetic(s) => s.pull(wait),
        }
    }

    fn has_frame(&self) -> bool {
        match self {
            Self::Wgc(s) => s.has_frame(),
            Self::Dda(s) => s.has_frame(),
            Self::Synthetic(s) => s.has_frame(),
        }
    }

    fn frame_for_slot(&mut self, slot: i64) -> Result<&GpuFrame> {
        match self {
            Self::Wgc(s) => s.frame_for_slot(slot),
            Self::Dda(s) => s.frame_for_slot(slot),
            Self::Synthetic(s) => s.frame_for_slot(slot),
        }
    }
}

/// Which capture API to try first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Api {
    /// WGC, then Desktop Duplication for a display.
    #[default]
    Wgc,
    /// Desktop Duplication for a display, WGC when it is refused; windows are always WGC.
    Duplication,
}

/// Open the source for `plan` and the pool for it. Windows come only from WGC; a display from
/// WGC or Desktop Duplication in the order `api` says, the other one when the first fails.
pub fn open_source(
    gpu: &Rc<Gpu>,
    bridge: &Rc<Bridge>,
    plan: &Plan,
    api: Api,
    overlay: Option<OverlayFn>,
) -> Result<(Source, SharedPool)> {
    let mut overlay = overlay;
    let wgc = |overlay: &mut Option<OverlayFn>| -> Result<(Source, SharedPool)> {
        let fp16 = plan.hwnd.is_some() || plan.hdr;
        let pool = FramePool::new(
            gpu.clone(),
            bridge.clone(),
            plan.crop.2.max(plan.out.0),
            plan.crop.3.max(plan.out.1),
            if fp16 {
                PoolFormat::Rgba16F
            } else {
                PoolFormat::Bgra8
            },
        )?;
        let mut s = WgcSource::open(gpu, bridge, pool.clone(), plan.clone())?;
        if let Some(f) = overlay.take() {
            s = s.with_overlay(f);
        }
        Ok((Source::Wgc(s), pool))
    };
    let dda = |overlay: &mut Option<OverlayFn>| -> Result<(Source, SharedPool)> {
        let fmt = DdaSource::probe_format(bridge, plan.monitor)?;
        let pool = FramePool::new(
            gpu.clone(),
            bridge.clone(),
            plan.monitor_bounds.width,
            plan.monitor_bounds.height,
            fmt,
        )?;
        let mut s = DdaSource::open(bridge.clone(), pool.clone(), plan.clone())?;
        if let Some(f) = overlay.take() {
            s = s.with_overlay(f);
        }
        Ok((Source::Dda(s), pool))
    };
    if plan.hwnd.is_some() {
        return wgc(&mut overlay);
    }
    match api {
        Api::Wgc => wgc(&mut overlay).or_else(|e| dda(&mut overlay).map_err(|_| e)),
        Api::Duplication => dda(&mut overlay).or_else(|e| wgc(&mut overlay).map_err(|_| e)),
    }
}

/// The sRGB curve undone (for the `PrintWindow` frame in FP16 slots, ZK-193).
pub(crate) fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// An `f32` as IEEE half bits (truncated; values here are 0 … a few, tiny ones become 0).
pub(crate) fn f16_bits(v: f32) -> u16 {
    let b = v.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xff) as i32 - 127 + 15;
    if exp <= 0 {
        return sign;
    }
    if exp >= 31 {
        return sign | 0x7c00;
    }
    sign | ((exp as u16) << 10) | (((b & 0x7f_ffff) >> 13) as u16)
}

#[cfg(test)]
mod print_tests {
    use super::*;

    #[test]
    fn half_floats_and_the_srgb_curve() {
        assert_eq!(f16_bits(0.0), 0);
        assert_eq!(f16_bits(1.0), 0x3c00);
        assert_eq!(f16_bits(0.5), 0x3800);
        assert_eq!(f16_bits(2.0), 0x4000);
        assert!((srgb_to_linear(1.0) - 1.0).abs() < 1e-6);
        assert!((srgb_to_linear(0.5) - 0.214).abs() < 1e-3);
    }
}
