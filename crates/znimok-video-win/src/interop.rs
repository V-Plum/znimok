//! The two devices of the pipeline and the bridge between them (from prototype P4, ZK-17):
//!
//! - **wgpu on DX12** ([`Gpu`]) runs the recording shader. The adapter is the one that drives the
//!   recorded display when known (Desktop Duplication only works there; on a hybrid laptop the
//!   encoder must sit on the same GPU as the frames), else the fastest one; WARP as the last resort.
//! - **D3D11 on the same adapter** ([`Bridge`], matched by LUID) is what WGC, Desktop Duplication
//!   and Media Foundation speak. Its `IMFDXGIDeviceManager` goes to the Sink Writer.
//!
//! Textures cross between them as NT shared handles ([`SharedTexture`]) and never through the
//! CPU. Two shared fences, each a monotonic counter, order the work: `f11` is signalled by the
//! D3D11 context (a frame copied into a slot, an output taken by the encoder) and waited on by
//! the wgpu queue; `f12` is signalled by the wgpu queue (the shader finished) and waited on by
//! the D3D11 context. Nothing on the CPU ever waits for the GPU on the hardware path.

use std::cell::Cell;

use wgpu::hal::api::Dx12;
use windows::Win32::Foundation::{CloseHandle, GENERIC_ALL, HANDLE, HMODULE, LUID};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_UNKNOWN, D3D_DRIVER_TYPE_WARP,
};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_FENCE_FLAG_SHARED, ID3D12Device, ID3D12Fence, ID3D12Resource,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_SHARED_RESOURCE_READ,
    DXGI_SHARED_RESOURCE_WRITE, IDXGIAdapter, IDXGIAdapter1, IDXGIFactory4, IDXGIResource1,
};
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::Media::MediaFoundation::{IMFDXGIDeviceManager, MFCreateDXGIDeviceManager};
use windows::core::{Interface, PCWSTR};

use crate::mf::err;

/// The wgpu side.
pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub name: String,
    pub luid: LUID,
    /// The adapter writes BGRA8 storage textures (what the encoder takes without a swizzle).
    pub bgra_storage: bool,
    pub max_texture: u32,
    _instance: wgpu::Instance,
}

fn luid_of(a: &wgpu::Adapter) -> Option<LUID> {
    // SAFETY: the hal adapter is only read (its DXGI description) while `a` is alive.
    unsafe {
        let hal = a.as_hal::<Dx12>()?;
        hal.raw_adapter().GetDesc1().ok().map(|d| d.AdapterLuid)
    }
}

pub fn same_luid(a: LUID, b: LUID) -> bool {
    a.LowPart == b.LowPart && a.HighPart == b.HighPart
}

pub fn luid_text(l: LUID) -> String {
    format!("{:08x}:{:08x}", l.HighPart as u32, l.LowPart)
}

impl Gpu {
    /// A DX12 device: on the adapter with `prefer`'s LUID when given and present, else the
    /// fastest adapter, else the software one.
    pub fn new(prefer: Option<LUID>) -> Result<Self, String> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::DX12;
        let instance = wgpu::Instance::new(desc);
        // ZNIMOK_GPU_SOFTWARE=1: the software adapter, as on a CI runner without a GPU.
        let software = std::env::var_os("ZNIMOK_GPU_SOFTWARE").is_some_and(|v| v == "1");
        let mut adapter = None;
        if software {
            adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::None,
                force_fallback_adapter: true,
                ..Default::default()
            }))
            .ok();
        } else if let Some(want) = prefer {
            adapter = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::DX12))
                .into_iter()
                .find(|a| luid_of(a).is_some_and(|l| same_luid(l, want)));
        }
        let adapter = match adapter {
            Some(a) => a,
            None => pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                ..Default::default()
            }))
            .or_else(|_| {
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::None,
                    force_fallback_adapter: true,
                    ..Default::default()
                }))
            })
            .map_err(|e| format!("немає адаптера wgpu (DX12): {e}"))?,
        };
        let info = adapter.get_info();
        let luid = luid_of(&adapter).ok_or("адаптер wgpu без LUID (не DX12?)")?;
        let bgra_storage = adapter
            .features()
            .contains(wgpu::Features::BGRA8UNORM_STORAGE);
        let max_texture = adapter.limits().max_texture_dimension_2d;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("znimok-record"),
            required_features: if bgra_storage {
                wgpu::Features::BGRA8UNORM_STORAGE
            } else {
                wgpu::Features::empty()
            },
            required_limits: wgpu::Limits {
                max_texture_dimension_2d: max_texture,
                ..wgpu::Limits::downlevel_defaults()
            },
            ..Default::default()
        }))
        .map_err(|e| format!("request_device: {e}"))?;
        Ok(Self {
            device,
            queue,
            name: format!("{} ({:?})", info.name, info.device_type),
            luid,
            bgra_storage,
            max_texture,
            _instance: instance,
        })
    }

    /// Wait for everything submitted so far (tests, the software path's readback).
    pub fn wait(&self) {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }
}

/// LUID of the adapter whose output drives `monitor` (DXGI walks every adapter: a GPU may be
/// listed twice and only one copy has outputs).
pub fn adapter_of_monitor(monitor: HMONITOR) -> Option<LUID> {
    // SAFETY: plain DXGI enumeration; every interface is owned by windows-rs.
    unsafe {
        let f: IDXGIFactory4 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)).ok()?;
        let mut a = 0;
        while let Ok(ad) = f.EnumAdapters1(a) {
            a += 1;
            let mut o = 0;
            while let Ok(out) = ad.EnumOutputs(o) {
                o += 1;
                if out.GetDesc().is_ok_and(|d| d.Monitor == monitor) {
                    return ad.GetDesc1().ok().map(|d| d.AdapterLuid);
                }
            }
        }
        None
    }
}

/// The D3D11 side and the fences.
pub struct Bridge {
    pub device: ID3D11Device,
    pub ctx: ID3D11DeviceContext4,
    pub manager: IMFDXGIDeviceManager,
    d12: ID3D12Device,
    /// Signalled by D3D11, waited on by wgpu.
    f11: ID3D11Fence,
    f11_in_12: ID3D12Fence,
    v11: Cell<u64>,
    /// Signalled by wgpu, waited on by D3D11.
    f12: ID3D12Fence,
    f12_in_11: ID3D11Fence,
    v12: Cell<u64>,
    pub adapter: String,
}

impl Bridge {
    pub fn new(gpu: &Gpu) -> Result<Self, String> {
        // SAFETY: D3D11/D3D12/DXGI calls with owned interfaces; the wgpu hal device is only read.
        unsafe {
            let d12: ID3D12Device = gpu
                .device
                .as_hal::<Dx12>()
                .ok_or("wgpu не на DX12")?
                .raw_device()
                .clone();
            let luid = d12.GetAdapterLuid();
            let factory: IDXGIFactory4 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))
                .map_err(err("CreateDXGIFactory2"))?;
            let adapter: IDXGIAdapter1 = factory
                .EnumAdapterByLuid(luid)
                .map_err(err("EnumAdapterByLuid"))?;
            let desc = adapter.GetDesc1().map_err(err("GetDesc1"))?;
            let name = String::from_utf16_lossy(&desc.Description)
                .trim_end_matches('\0')
                .to_string();
            // DXGI_ADAPTER_FLAG_SOFTWARE: the Basic Render Driver (WARP).
            let software = desc.Flags & 2 != 0;
            let (mut device, mut ctx) = (None, None);
            let dxgi_adapter: IDXGIAdapter = adapter.cast().map_err(err("IDXGIAdapter"))?;
            // The video-support flag is what Media Foundation's hardware paths want; an adapter
            // that refuses it (WARP on a CI runner: DXGI_ERROR_UNSUPPORTED) still gives a plain
            // device, and the software adapter is asked for by its own driver type.
            let attempts: [(
                Option<&IDXGIAdapter>,
                D3D_DRIVER_TYPE,
                D3D11_CREATE_DEVICE_FLAG,
            ); 3] = [
                (
                    Some(&dxgi_adapter),
                    D3D_DRIVER_TYPE_UNKNOWN,
                    D3D11_CREATE_DEVICE_VIDEO_SUPPORT | D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                ),
                (
                    Some(&dxgi_adapter),
                    D3D_DRIVER_TYPE_UNKNOWN,
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                ),
                (None, D3D_DRIVER_TYPE_WARP, D3D11_CREATE_DEVICE_BGRA_SUPPORT),
            ];
            let mut r = Ok(());
            for (ad, driver, flags) in attempts {
                if driver == D3D_DRIVER_TYPE_WARP && !software {
                    break;
                }
                r = D3D11CreateDevice(
                    ad,
                    driver,
                    HMODULE::default(),
                    flags,
                    None,
                    D3D11_SDK_VERSION,
                    Some(&mut device),
                    None,
                    Some(&mut ctx),
                );
                if r.is_ok() {
                    break;
                }
            }
            r.map_err(err("D3D11CreateDevice"))?;
            let device: ID3D11Device = device.ok_or("немає D3D11-пристрою")?;
            let ctx: ID3D11DeviceContext4 = ctx
                .ok_or("немає контексту D3D11")?
                .cast()
                .map_err(err("ID3D11DeviceContext4"))?;
            // Media Foundation works on this device from its own threads (LH §7 item 10).
            let _ = device
                .cast::<ID3D11Multithread>()
                .map_err(err("ID3D11Multithread"))?
                .SetMultithreadProtected(true);
            let (mut token, mut manager) = (0u32, None);
            MFCreateDXGIDeviceManager(&mut token, &mut manager)
                .map_err(err("MFCreateDXGIDeviceManager"))?;
            let manager = manager.ok_or("немає менеджера DXGI")?;
            manager
                .ResetDevice(&device, token)
                .map_err(err("ResetDevice"))?;

            let dev5: ID3D11Device5 = device.cast().map_err(err("ID3D11Device5"))?;
            let mut f11: Option<ID3D11Fence> = None;
            dev5.CreateFence(0, D3D11_FENCE_FLAG_SHARED, &mut f11)
                .map_err(err("CreateFence (D3D11)"))?;
            let f11 = f11.ok_or("fence")?;
            let h = f11
                .CreateSharedHandle(None, GENERIC_ALL.0, PCWSTR::null())
                .map_err(err("CreateSharedHandle (fence)"))?;
            let mut f11_in_12: Option<ID3D12Fence> = None;
            let r = d12.OpenSharedHandle(h, &mut f11_in_12);
            let _ = CloseHandle(h);
            r.map_err(err("OpenSharedHandle (fence)"))?;
            let f12: ID3D12Fence = d12
                .CreateFence(0, D3D12_FENCE_FLAG_SHARED)
                .map_err(err("CreateFence (D3D12)"))?;
            let h: HANDLE = d12
                .CreateSharedHandle(&f12, None, GENERIC_ALL.0, PCWSTR::null())
                .map_err(err("CreateSharedHandle (fence 12)"))?;
            let mut f12_in_11: Option<ID3D11Fence> = None;
            let r = dev5.OpenSharedFence(h, &mut f12_in_11);
            let _ = CloseHandle(h);
            r.map_err(err("OpenSharedFence"))?;
            Ok(Self {
                device,
                ctx,
                manager,
                d12,
                f11,
                f11_in_12: f11_in_12.ok_or("fence 12")?,
                v11: Cell::new(0),
                f12,
                f12_in_11: f12_in_11.ok_or("fence 11")?,
                v12: Cell::new(0),
                adapter: name,
            })
        }
    }

    /// D3D11 has queued work others depend on (a copy into a slot, an output consumed): signal
    /// and flush, so the wait on the other side is not stuck behind D3D11 batching.
    pub fn signal_d3d11(&self) -> u64 {
        let v = self.v11.get() + 1;
        self.v11.set(v);
        // SAFETY: the fence belongs to this context's device.
        unsafe {
            let _ = self.ctx.Signal(&self.f11, v);
            self.ctx.Flush();
        }
        v
    }

    /// D3D11 must not touch a texture before the wgpu work up to `v` is done (GPU-side wait).
    pub fn wait_d3d11(&self, v: u64) {
        if v == 0 {
            return;
        }
        // SAFETY: as above.
        unsafe {
            let _ = self.ctx.Wait(&self.f12_in_11, v);
        }
    }

    /// The next wgpu submit must not start before D3D11's work up to `v`.
    pub fn wait_in_wgpu(&self, gpu: &Gpu, v: u64) -> Result<(), String> {
        if v == 0 {
            return Ok(());
        }
        // SAFETY: the hal queue is only asked to stage a fence wait.
        unsafe {
            let q = gpu.queue.as_hal::<Dx12>().ok_or("черга wgpu не DX12")?;
            q.add_wait_fence(self.f11_in_12.clone(), v);
        }
        Ok(())
    }

    /// The next wgpu submit signals its completion; returns the value D3D11 waits for.
    pub fn signal_in_wgpu(&self, gpu: &Gpu) -> Result<u64, String> {
        let v = self.v12.get() + 1;
        self.v12.set(v);
        // SAFETY: as above.
        unsafe {
            let q = gpu.queue.as_hal::<Dx12>().ok_or("черга wgpu не DX12")?;
            q.add_signal_fence(self.f12.clone(), v);
        }
        Ok(v)
    }

    /// A texture both sides see: created in D3D11 with an NT shared handle, opened in wgpu.
    #[allow(clippy::too_many_arguments)]
    pub fn shared_texture(
        &self,
        gpu: &Gpu,
        width: u32,
        height: u32,
        dxgi: DXGI_FORMAT,
        bind: u32,
        format: wgpu::TextureFormat,
        usage: wgpu::TextureUsages,
        label: &'static str,
    ) -> Result<SharedTexture, String> {
        if width == 0 || height == 0 || width > gpu.max_texture || height > gpu.max_texture {
            return Err(format!(
                "текстура {width}×{height} поза межами адаптера ({})",
                gpu.max_texture
            ));
        }
        // SAFETY: D3D11/D3D12 resource creation and the hal import of a resource this device
        // owns; the handle is closed after the open.
        unsafe {
            let td = D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height,
                MipLevels: 1,
                ArraySize: 1,
                Format: dxgi,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: bind,
                CPUAccessFlags: 0,
                MiscFlags: (D3D11_RESOURCE_MISC_SHARED.0 | D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0)
                    as u32,
            };
            let mut tex11 = None;
            self.device
                .CreateTexture2D(&td, None, Some(&mut tex11))
                .map_err(err("CreateTexture2D (shared)"))?;
            let tex11: ID3D11Texture2D = tex11.ok_or("texture")?;
            let h = tex11
                .cast::<IDXGIResource1>()
                .map_err(err("IDXGIResource1"))?
                .CreateSharedHandle(
                    None,
                    (DXGI_SHARED_RESOURCE_READ | DXGI_SHARED_RESOURCE_WRITE).0,
                    PCWSTR::null(),
                )
                .map_err(err("CreateSharedHandle (texture)"))?;
            let mut res: Option<ID3D12Resource> = None;
            let r = self.d12.OpenSharedHandle(h, &mut res);
            let _ = CloseHandle(h);
            r.map_err(err("OpenSharedHandle (texture)"))?;
            let size = wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            };
            let hal = wgpu::hal::dx12::Device::texture_from_raw(
                res.ok_or("resource")?,
                format,
                wgpu::TextureDimension::D2,
                size,
                1,
                1,
            );
            let texture = gpu.device.create_texture_from_hal::<Dx12>(
                hal,
                &wgpu::TextureDescriptor {
                    label: Some(label),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                },
                wgpu::TextureUses::empty(),
            );
            let view = texture.create_view(&Default::default());
            Ok(SharedTexture {
                tex11,
                texture,
                view,
                width,
                height,
                format,
            })
        }
    }
}

pub struct SharedTexture {
    pub tex11: ID3D11Texture2D,
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub width: u32,
    pub height: u32,
    pub format: wgpu::TextureFormat,
}
