//! Windows: Media Foundation's Source Reader on a D3D11 device of the same adapter as the UI's
//! wgpu device (matched by LUID — on a hybrid laptop decoder and renderer must share one GPU),
//! hardware decoding into D3D11 NV12 textures, one GPU copy of each frame into a small pool of
//! shareable NV12 textures that the wgpu device opens (the decoder's own surfaces are not
//! shareable), then `nv12.wgsl` reads the pool texture's two planes. The MP4 is read in place
//! from the `.znimok` through an `IStream` over its byte ranges.
//!
//! The two APIs are ordered on the CPU, on this thread: after the D3D11 copy the player waits for
//! its fence (a copy of one frame, well under a millisecond), and before a pool texture is written
//! again it waits for the wgpu submission that read it. GPU-side shared fences (P4) would attach
//! to whatever the UI thread submits next on the same queue — not safe on a shared queue.
//!
//! Fallback, when the device has no NV12 textures or the decoder hands out CPU memory: the
//! planes are locked and uploaded (one copy), the same shader converts.
//!
//! Traps (see memory `znimok-video-win-mf-traps`): never `MF_LOW_LATENCY` on the reader (the first
//! frame twice at 0, the rest a frame early); the decoder may allocate more rows than it shows —
//! the minimum display aperture says what to show.

use std::ffi::c_void;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::sync::Mutex;
use std::sync::OnceLock;

use wgpu::hal::api::Dx12;
use windows::Win32::Foundation::{
    CloseHandle, E_NOTIMPL, HANDLE, HMODULE, S_FALSE, S_OK, STG_E_ACCESSDENIED, WAIT_OBJECT_0,
};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Direct3D12::{ID3D12Device, ID3D12Resource};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_SHARED_RESOURCE_READ,
    DXGI_SHARED_RESOURCE_WRITE, IDXGIAdapter, IDXGIAdapter1, IDXGIFactory4, IDXGIResource1,
};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0};
use windows::Win32::System::Com::{
    COINIT_MULTITHREADED, CoInitializeEx, ISequentialStream_Impl, IStream, IStream_Impl, LOCKTYPE,
    STATFLAG, STATSTG, STGC, STGTY_STREAM, STREAM_SEEK, STREAM_SEEK_CUR, STREAM_SEEK_END,
    STREAM_SEEK_SET,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::System::Variant::VT_I8;
use windows::core::{GUID, HRESULT, HSTRING, Interface, PCWSTR, Ref, implement};

use crate::convert::{Converter, Gpu, Planes};
use crate::{Decoder, Source};

const FIRST_VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

fn e(what: &str) -> impl Fn(windows::core::Error) -> String + '_ {
    move |err| format!("{what}: {} (0x{:08X})", err.message(), err.code().0)
}

fn startup() -> Result<(), String> {
    static DONE: OnceLock<Result<(), String>> = OnceLock::new();
    // SAFETY: COM on this thread (MTA; S_FALSE/RPC_E_CHANGED_MODE are fine) and MF once per process.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    DONE.get_or_init(|| unsafe {
        MFStartup(MF_VERSION, MFSTARTUP_FULL).map_err(|err| format!("MFStartup: {err}"))
    })
    .clone()
}

// ------------------------------------------------------------------ the MP4 inside the document

/// The MP4's byte ranges in the `.znimok`, as a read-only COM stream (Media Foundation reads it
/// through `MFCreateMFByteStreamOnStream`).
#[implement(IStream)]
struct PayloadStream {
    r: Mutex<znimok_format::video::PayloadReader<BufReader<std::fs::File>>>,
    len: u64,
}

impl ISequentialStream_Impl for PayloadStream_Impl {
    fn Read(&self, pv: *mut c_void, cb: u32, pcbread: *mut u32) -> HRESULT {
        let Ok(mut r) = self.r.lock() else {
            return STG_E_ACCESSDENIED;
        };
        // SAFETY: the caller gives a buffer of `cb` bytes.
        let buf = unsafe { std::slice::from_raw_parts_mut(pv.cast::<u8>(), cb as usize) };
        let mut total = 0usize;
        while total < buf.len() {
            match r.read(&mut buf[total..]) {
                Ok(0) => break,
                Ok(n) => total += n,
                Err(_) => return STG_E_ACCESSDENIED,
            }
        }
        if !pcbread.is_null() {
            // SAFETY: an out-pointer from the caller.
            unsafe { *pcbread = total as u32 };
        }
        if total < buf.len() { S_FALSE } else { S_OK }
    }

    fn Write(&self, _pv: *const c_void, _cb: u32, _pcbwritten: *mut u32) -> HRESULT {
        STG_E_ACCESSDENIED
    }
}

impl IStream_Impl for PayloadStream_Impl {
    fn Seek(
        &self,
        dlibmove: i64,
        dworigin: STREAM_SEEK,
        plibnewposition: *mut u64,
    ) -> windows::core::Result<()> {
        let mut r = self
            .r
            .lock()
            .map_err(|_| windows::core::Error::from(STG_E_ACCESSDENIED))?;
        let to = match dworigin {
            STREAM_SEEK_SET => SeekFrom::Start(dlibmove.max(0) as u64),
            STREAM_SEEK_CUR => SeekFrom::Current(dlibmove),
            STREAM_SEEK_END => SeekFrom::End(dlibmove),
            _ => return Err(STG_E_ACCESSDENIED.into()),
        };
        let p = r
            .seek(to)
            .map_err(|_| windows::core::Error::from(STG_E_ACCESSDENIED))?;
        if !plibnewposition.is_null() {
            // SAFETY: an out-pointer from the caller.
            unsafe { *plibnewposition = p };
        }
        Ok(())
    }

    fn SetSize(&self, _libnewsize: u64) -> windows::core::Result<()> {
        Err(STG_E_ACCESSDENIED.into())
    }

    fn CopyTo(
        &self,
        _pstm: Ref<IStream>,
        _cb: u64,
        _pcbread: *mut u64,
        _pcbwritten: *mut u64,
    ) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn Commit(&self, _grfcommitflags: &STGC) -> windows::core::Result<()> {
        Ok(())
    }

    fn Revert(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn LockRegion(&self, _o: u64, _cb: u64, _t: &LOCKTYPE) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn UnlockRegion(&self, _o: u64, _cb: u64, _t: u32) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn Stat(&self, pstatstg: *mut STATSTG, _grfstatflag: &STATFLAG) -> windows::core::Result<()> {
        if pstatstg.is_null() {
            return Err(STG_E_ACCESSDENIED.into());
        }
        // SAFETY: an out-pointer from the caller; the name stays null (STATFLAG_NONAME or not).
        unsafe {
            *pstatstg = STATSTG {
                r#type: STGTY_STREAM.0 as u32,
                cbSize: self.len,
                ..Default::default()
            };
        }
        Ok(())
    }

    fn Clone(&self) -> windows::core::Result<IStream> {
        Err(E_NOTIMPL.into())
    }
}

fn byte_stream(
    path: &std::path::Path,
    ranges: &[std::ops::Range<u64>],
) -> Result<IMFByteStream, String> {
    let payload = znimok_format::video::Payload {
        ranges: ranges.to_vec(),
    };
    let f = std::fs::File::open(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let reader = znimok_format::video::PayloadReader::new(BufReader::new(f), &payload);
    let stream: IStream = PayloadStream {
        len: payload.len(),
        r: Mutex::new(reader),
    }
    .into();
    // SAFETY: MF wraps our stream; the attribute tells its resolver what is inside.
    unsafe {
        let bs =
            MFCreateMFByteStreamOnStream(&stream).map_err(e("MFCreateMFByteStreamOnStream"))?;
        if let Ok(a) = bs.cast::<IMFAttributes>() {
            let _ = a.SetString(&MF_BYTESTREAM_CONTENT_TYPE, &HSTRING::from("video/mp4"));
        }
        Ok(bs)
    }
}

// ------------------------------------------------------------------ the D3D11 side

struct Bridge {
    device: ID3D11Device,
    ctx: ID3D11DeviceContext4,
    manager: IMFDXGIDeviceManager,
    d12: ID3D12Device,
    fence: ID3D11Fence,
    fence_value: u64,
    event: HANDLE,
}

impl Drop for Bridge {
    fn drop(&mut self) {
        // SAFETY: our own event handle.
        unsafe {
            let _ = CloseHandle(self.event);
        }
    }
}

impl Bridge {
    /// A D3D11 device (with video support) on the adapter of the wgpu device.
    fn new(gpu: &Gpu) -> Result<Self, String> {
        // SAFETY: the hal device is only read (its D3D12 device) while `gpu` is alive; the rest
        // are plain D3D11 / DXGI / MF calls on objects we own.
        unsafe {
            let d12: ID3D12Device = gpu
                .device
                .as_hal::<Dx12>()
                .ok_or("the UI's wgpu device is not DX12")?
                .raw_device()
                .clone();
            let luid = d12.GetAdapterLuid();
            let factory: IDXGIFactory4 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))
                .map_err(e("CreateDXGIFactory2"))?;
            let adapter: IDXGIAdapter1 = factory
                .EnumAdapterByLuid(luid)
                .map_err(e("EnumAdapterByLuid"))?;
            let (mut device, mut ctx) = (None, None);
            D3D11CreateDevice(
                &adapter.cast::<IDXGIAdapter>().map_err(e("IDXGIAdapter"))?,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_VIDEO_SUPPORT | D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut ctx),
            )
            .map_err(e("D3D11CreateDevice"))?;
            let device: ID3D11Device = device.ok_or("no D3D11 device")?;
            let ctx: ID3D11DeviceContext4 = ctx
                .ok_or("no D3D11 context")?
                .cast()
                .map_err(e("ID3D11DeviceContext4"))?;
            // The decoder works on this device from its own threads.
            let _ = device
                .cast::<ID3D11Multithread>()
                .map_err(e("ID3D11Multithread"))?
                .SetMultithreadProtected(true);
            let (mut token, mut manager) = (0u32, None);
            MFCreateDXGIDeviceManager(&mut token, &mut manager)
                .map_err(e("MFCreateDXGIDeviceManager"))?;
            let manager = manager.ok_or("no DXGI device manager")?;
            manager
                .ResetDevice(&device, token)
                .map_err(e("ResetDevice"))?;
            let dev5: ID3D11Device5 = device.cast().map_err(e("ID3D11Device5"))?;
            let mut fence: Option<ID3D11Fence> = None;
            dev5.CreateFence(0, D3D11_FENCE_FLAG_NONE, &mut fence)
                .map_err(e("CreateFence"))?;
            let event =
                CreateEventW(None, false, false, PCWSTR::null()).map_err(e("CreateEvent"))?;
            Ok(Self {
                device,
                ctx,
                manager,
                d12,
                fence: fence.ok_or("no fence")?,
                fence_value: 0,
                event,
            })
        }
    }

    /// Waits on this thread until the D3D11 work queued so far is done.
    fn finish(&mut self) -> Result<(), String> {
        self.fence_value += 1;
        // SAFETY: our own context, fence and event.
        unsafe {
            self.ctx
                .Signal(&self.fence, self.fence_value)
                .map_err(e("Signal"))?;
            self.ctx.Flush();
            if self.fence.GetCompletedValue() < self.fence_value {
                self.fence
                    .SetEventOnCompletion(self.fence_value, self.event)
                    .map_err(e("SetEventOnCompletion"))?;
                if WaitForSingleObject(self.event, 2000) != WAIT_OBJECT_0 {
                    return Err("the D3D11 copy did not finish".into());
                }
            }
        }
        Ok(())
    }
}

/// A shareable NV12 texture of the pool, open on both devices.
struct Slot {
    tex11: ID3D11Texture2D,
    wgpu: wgpu::Texture,
    y: wgpu::TextureView,
    uv: wgpu::TextureView,
    read: Option<wgpu::SubmissionIndex>,
}

fn shared_nv12(bridge: &Bridge, gpu: &Gpu, w: u32, h: u32) -> Result<Slot, String> {
    // SAFETY: D3D11 / D3D12 calls on objects we own; the D3D12 resource goes to wgpu, which
    // keeps it alive as long as the texture.
    unsafe {
        let td = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_NV12,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: (D3D11_RESOURCE_MISC_SHARED.0 | D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0)
                as u32,
        };
        let mut t = None;
        bridge
            .device
            .CreateTexture2D(&td, None, Some(&mut t))
            .map_err(e("CreateTexture2D NV12"))?;
        let tex11: ID3D11Texture2D = t.ok_or("no texture")?;
        let h_shared = tex11
            .cast::<IDXGIResource1>()
            .map_err(e("IDXGIResource1"))?
            .CreateSharedHandle(
                None,
                (DXGI_SHARED_RESOURCE_READ | DXGI_SHARED_RESOURCE_WRITE).0,
                PCWSTR::null(),
            )
            .map_err(e("CreateSharedHandle"))?;
        let mut res: Option<ID3D12Resource> = None;
        let r = bridge.d12.OpenSharedHandle(h_shared, &mut res);
        let _ = CloseHandle(h_shared);
        r.map_err(e("OpenSharedHandle"))?;
        let size = wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        };
        let hal = wgpu::hal::dx12::Device::texture_from_raw(
            res.ok_or("no resource")?,
            wgpu::TextureFormat::NV12,
            wgpu::TextureDimension::D2,
            size,
            1,
            1,
        );
        let tex = gpu.device.create_texture_from_hal::<Dx12>(
            hal,
            &wgpu::TextureDescriptor {
                label: Some("znimok nv12 shared"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::NV12,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::TextureUses::empty(),
        );
        let view = |format, aspect| {
            tex.create_view(&wgpu::TextureViewDescriptor {
                format: Some(format),
                aspect,
                ..Default::default()
            })
        };
        Ok(Slot {
            tex11,
            y: view(wgpu::TextureFormat::R8Unorm, wgpu::TextureAspect::Plane0),
            uv: view(wgpu::TextureFormat::Rg8Unorm, wgpu::TextureAspect::Plane1),
            wgpu: tex,
            read: None,
        })
    }
}

// ------------------------------------------------------------------ the player's decoder

pub struct MfPlayer {
    reader: IMFSourceReader,
    gpu: Gpu,
    bridge: Option<Bridge>,
    /// The zero-copy pool (empty on the fallback path).
    slots: Vec<Slot>,
    next_slot: usize,
    planes: Option<Planes>,
    zero: bool,
    width: u32,
    height: u32,
    alloc_height: u32,
    fps: f64,
    sample: Option<IMFSample>,
    /// Scratch for the fallback path.
    y: Vec<u8>,
    uv: Vec<u8>,
    path: &'static str,
}

impl MfPlayer {
    pub fn open(gpu: &Gpu, source: &Source) -> Result<Self, String> {
        startup()?;
        let bridge = match Bridge::new(gpu) {
            Ok(b) => Some(b),
            Err(err) => {
                tracing::warn!("player: no D3D11 decoding ({err}); software decoder");
                None
            }
        };
        let nv12 = gpu
            .device
            .features()
            .contains(wgpu::Features::TEXTURE_FORMAT_NV12);
        // SAFETY: Media Foundation calls with owned interfaces.
        let reader = unsafe {
            let mut attrs = None;
            MFCreateAttributes(&mut attrs, 3).map_err(e("MFCreateAttributes"))?;
            let attrs = attrs.ok_or("no attributes")?;
            if let Some(b) = &bridge {
                attrs
                    .SetUnknown(&MF_SOURCE_READER_D3D_MANAGER, &b.manager)
                    .map_err(e("attr"))?;
                attrs
                    .SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)
                    .map_err(e("attr"))?;
            }
            let reader = match source {
                Source::InFile { path, ranges, .. } => {
                    MFCreateSourceReaderFromByteStream(&byte_stream(path, ranges)?, &attrs)
                        .map_err(e("MFCreateSourceReaderFromByteStream"))?
                }
                Source::File(p) => {
                    let abs = std::path::absolute(p).map_err(|err| err.to_string())?;
                    MFCreateSourceReaderFromURL(&HSTRING::from(abs.as_os_str()), &attrs)
                        .map_err(e("MFCreateSourceReaderFromURL"))?
                }
            };
            reader
                .SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)
                .map_err(e("SetStreamSelection"))?;
            reader
                .SetStreamSelection(FIRST_VIDEO, true)
                .map_err(e("SetStreamSelection"))?;
            let t = MFCreateMediaType().map_err(e("MFCreateMediaType"))?;
            t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .map_err(e("type"))?;
            t.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)
                .map_err(e("type"))?;
            reader
                .SetCurrentMediaType(FIRST_VIDEO, None, &t)
                .map_err(e("SetCurrentMediaType NV12"))?;
            reader
        };
        let mut me = Self {
            reader,
            gpu: gpu.clone(),
            zero: bridge.is_some() && nv12,
            bridge,
            slots: Vec::new(),
            next_slot: 0,
            planes: None,
            width: 0,
            height: 0,
            alloc_height: 0,
            fps: 0.0,
            sample: None,
            y: Vec::new(),
            uv: Vec::new(),
            path: "gpu",
        };
        me.refresh_type()?;
        me.path = if me.bridge.is_none() {
            "software"
        } else if me.zero {
            "gpu"
        } else {
            "upload"
        };
        Ok(me)
    }

    fn refresh_type(&mut self) -> Result<(), String> {
        // SAFETY: reads of the current media type.
        unsafe {
            let t = self
                .reader
                .GetCurrentMediaType(FIRST_VIDEO)
                .map_err(e("GetCurrentMediaType"))?;
            let size = |key: &GUID| t.GetUINT64(key).ok().map(|v| ((v >> 32) as u32, v as u32));
            let (w, h) = size(&MF_MT_FRAME_SIZE).ok_or("no MF_MT_FRAME_SIZE")?;
            self.alloc_height = h;
            let (mut dw, mut dh) = (w, h);
            let mut area = MFVideoArea::default();
            let blob = std::slice::from_raw_parts_mut(
                (&raw mut area).cast::<u8>(),
                std::mem::size_of::<MFVideoArea>(),
            );
            if t.GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, blob, None)
                .is_ok()
                && area.Area.cx > 0
                && area.Area.cy > 0
            {
                dw = area.Area.cx as u32;
                dh = area.Area.cy as u32;
            }
            if (dw, dh) != (self.width, self.height) {
                self.slots.clear();
                self.planes = None;
            }
            self.width = dw;
            self.height = dh;
            if let Some((n, d)) = size(&MF_MT_FRAME_RATE) {
                self.fps = f64::from(n) / f64::from(d.max(1));
            }
        }
        Ok(())
    }

    /// The zero-copy path: the decoded surface copied into the next pool texture on the GPU.
    fn convert_zero(
        &mut self,
        conv: &Converter,
        out: &wgpu::Texture,
        thumb: bool,
    ) -> Result<wgpu::SubmissionIndex, String> {
        let sample = self.sample.as_ref().ok_or("no frame")?;
        let (w, h) = (self.width, self.height);
        // SAFETY: MF / D3D11 calls on the decoder's sample and our own textures.
        let (src, sub) = unsafe {
            let buf = sample.GetBufferByIndex(0).map_err(e("GetBufferByIndex"))?;
            let dx: IMFDXGIBuffer = match buf.cast() {
                Ok(d) => d,
                // The decoder handed out CPU memory after all: the upload path from now on.
                Err(_) => {
                    self.zero = false;
                    self.path = "upload";
                    return self.convert_upload(conv, out, thumb);
                }
            };
            let mut src: Option<ID3D11Texture2D> = None;
            dx.GetResource(&ID3D11Texture2D::IID, (&raw mut src).cast())
                .map_err(e("GetResource"))?;
            let sub = dx.GetSubresourceIndex().map_err(e("GetSubresourceIndex"))?;
            (src.ok_or("no decoder texture")?, sub)
        };
        let bridge = self.bridge.as_mut().ok_or("no D3D11 device")?;
        if self.slots.len() < 3 {
            self.slots.push(shared_nv12(bridge, &self.gpu, w, h)?);
            self.next_slot = self.slots.len() - 1;
        }
        let i = self.next_slot;
        self.next_slot = (i + 1) % 3;
        let slot = &mut self.slots[i];
        // wgpu is done with this texture before D3D11 writes it again.
        if let Some(done) = slot.read.take() {
            conv.wait(&done);
        }
        let bx = D3D11_BOX {
            left: 0,
            top: 0,
            front: 0,
            right: w,
            bottom: h,
            back: 1,
        };
        // SAFETY: a copy between two NV12 textures of this device, inside both.
        unsafe {
            bridge
                .ctx
                .CopySubresourceRegion(&slot.tex11, 0, 0, 0, 0, &src, sub, Some(&bx));
        }
        bridge.finish()?;
        let done = conv.run(&slot.y, &slot.uv, w, h, out, thumb);
        slot.read = Some(done.clone());
        let _ = &slot.wgpu;
        Ok(done)
    }

    /// The fallback: the planes locked into CPU memory and uploaded.
    fn convert_upload(
        &mut self,
        conv: &Converter,
        out: &wgpu::Texture,
        thumb: bool,
    ) -> Result<wgpu::SubmissionIndex, String> {
        let sample = self.sample.as_ref().ok_or("no frame")?;
        let (w, h) = (self.width, self.height);
        // SAFETY: a locked 2-D buffer is read inside its pitch × rows.
        unsafe {
            let buf = sample.GetBufferByIndex(0).map_err(e("GetBufferByIndex"))?;
            // A DXGI buffer maps the whole texture: its chroma starts after the texture's rows.
            let rows = match buf.cast::<IMFDXGIBuffer>() {
                Ok(d) => {
                    let mut tex: Option<ID3D11Texture2D> = None;
                    d.GetResource(&ID3D11Texture2D::IID, (&raw mut tex).cast())
                        .map_err(e("GetResource"))?;
                    let mut desc = Default::default();
                    tex.ok_or("no texture")?.GetDesc(&mut desc);
                    desc.Height
                }
                Err(_) => self.alloc_height,
            };
            let b2 = buf.cast::<IMF2DBuffer>().map_err(e("IMF2DBuffer"))?;
            let (mut p, mut pitch) = (std::ptr::null_mut(), 0i32);
            b2.Lock2D(&mut p, &mut pitch).map_err(e("Lock2D"))?;
            let pitch = pitch.unsigned_abs() as usize;
            let wu = w as usize;
            self.y.clear();
            self.uv.clear();
            for r in 0..h as usize {
                self.y
                    .extend_from_slice(std::slice::from_raw_parts(p.add(r * pitch), wu));
            }
            let c0 = p.add(pitch * rows as usize);
            for r in 0..h.div_ceil(2) as usize {
                self.uv.extend_from_slice(std::slice::from_raw_parts(
                    c0.add(r * pitch),
                    wu.div_ceil(2) * 2,
                ));
            }
            b2.Unlock2D().map_err(e("Unlock2D"))?;
        }
        if self.planes.is_none() {
            self.planes = Some(conv.planes(w, h));
        }
        let p = self.planes.as_ref().unwrap();
        conv.upload(p, &self.y, w, &self.uv, w.div_ceil(2) * 2);
        let (yv, uvv) = (
            p.y.create_view(&Default::default()),
            p.uv.create_view(&Default::default()),
        );
        Ok(conv.run(&yv, &uvv, w, h, out, thumb))
    }
}

impl Decoder for MfPlayer {
    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn fps(&self) -> f64 {
        self.fps
    }

    fn path(&self) -> &'static str {
        self.path
    }

    fn seek(&mut self, frame: i64) -> Result<(), String> {
        let fps = if self.fps > 0.0 { self.fps } else { 30.0 };
        let t = znimok_video::traits::seek_time_for_frame(frame.max(0), fps);
        let v = PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VT_I8,
                    Anonymous: windows::Win32::System::Com::StructuredStorage::PROPVARIANT_0_0_0 {
                        hVal: t,
                    },
                    ..Default::default()
                }),
            },
        };
        self.sample = None;
        // SAFETY: a seek of our reader.
        unsafe {
            self.reader
                .SetCurrentPosition(&GUID::zeroed(), &v)
                .map_err(e("SetCurrentPosition"))
        }
    }

    fn next(&mut self) -> Result<Option<(i64, i64)>, String> {
        loop {
            let (mut flags, mut ts, mut sample) = (0u32, 0i64, None);
            // SAFETY: a synchronous read of our reader.
            unsafe {
                self.reader
                    .ReadSample(
                        FIRST_VIDEO,
                        0,
                        None,
                        Some(&mut flags),
                        Some(&mut ts),
                        Some(&mut sample),
                    )
                    .map_err(e("ReadSample"))?;
            }
            if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0 {
                self.refresh_type()?;
            }
            if let Some(s) = sample {
                // SAFETY: a read of the sample's duration.
                let dur = unsafe { s.GetSampleDuration() }.unwrap_or(0);
                let fps = if self.fps > 0.0 { self.fps } else { 30.0 };
                let dur = if dur > 0 {
                    dur
                } else {
                    (1e7 / fps).round() as i64
                };
                self.sample = Some(s);
                return Ok(Some(znimok_video::traits::frames_of_sample(ts, dur, fps)));
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                self.sample = None;
                return Ok(None);
            }
        }
    }

    fn convert(
        &mut self,
        conv: &Converter,
        out: &wgpu::Texture,
        thumb: bool,
    ) -> Result<wgpu::SubmissionIndex, String> {
        if self.zero {
            self.convert_zero(conv, out, thumb)
        } else {
            self.convert_upload(conv, out, thumb)
        }
    }
}
