//! The zero-copy bridge: a D3D11 device on exactly the adapter wgpu picked (matched by LUID — on
//! a hybrid laptop the decoder and the renderer must share one GPU), the MF DXGI device manager
//! for the decoder, and a small pool of NV12 textures shared through NT handles into wgpu's D3D12
//! device. Per frame: one GPU copy decoder surface → pool slot (decoder surfaces are not shareable),
//! then wgpu reads the slot directly. Two shared fences order the two APIs:
//!   copy fence — D3D11 signals after the copy, the wgpu queue waits before the shader;
//!   read fence — the wgpu queue signals after the shader, D3D11 waits before reusing the slot.

use crate::gpu::Gpu;
use wgpu::hal::api::Dx12;
use windows::Win32::Foundation::{CloseHandle, GENERIC_ALL, HANDLE, HMODULE, LUID};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_FENCE_FLAG_SHARED, ID3D12Device, ID3D12Fence, ID3D12Resource,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_SHARED_RESOURCE_READ,
    DXGI_SHARED_RESOURCE_WRITE, IDXGIAdapter, IDXGIAdapter1, IDXGIFactory4, IDXGIResource1,
};
use windows::Win32::Media::MediaFoundation::{
    IMFDXGIBuffer, IMFDXGIDeviceManager, IMFSample, MFCreateDXGIDeviceManager,
};
use windows::core::{Interface, PCWSTR};

fn e(what: &str) -> impl Fn(windows::core::Error) -> String + '_ {
    move |err| format!("{what}: {err}")
}

struct Slot {
    tex11: ID3D11Texture2D,
    bind: wgpu::BindGroup,
    // Keeps the imported texture alive for the bind group.
    _wgpu: wgpu::Texture,
    last_read: u64,
}

pub struct Bridge {
    pub device: ID3D11Device,
    d12: ID3D12Device,
    ctx: ID3D11DeviceContext4,
    pub manager: IMFDXGIDeviceManager,
    copy11: ID3D11Fence,
    copy12: ID3D12Fence,
    copy_value: u64,
    read11: ID3D11Fence,
    read12: ID3D12Fence,
    read_value: u64,
    slots: Vec<Slot>,
    next: usize,
    /// Fallback path: CPU-readable copies, mapped `lag` frames after the copy so the map never
    /// waits for the GPU.
    staging: Vec<ID3D11Texture2D>,
    staged: std::collections::VecDeque<usize>,
    stage_next: usize,
    width: u32,
    height: u32,
    pub adapter: String,
    pub luid: String,
}

/// The decoder's texture (often one slice of a texture array) behind a sample.
unsafe fn decoder_texture(sample: &IMFSample) -> Result<(ID3D11Texture2D, u32), String> {
    unsafe {
        let buf = sample.GetBufferByIndex(0).map_err(e("GetBufferByIndex"))?;
        let dx: IMFDXGIBuffer = buf
            .cast()
            .map_err(e("не DXGI-буфер (декодер не на GPU?)"))?;
        let mut src: Option<ID3D11Texture2D> = None;
        dx.GetResource(&ID3D11Texture2D::IID, (&raw mut src).cast())
            .map_err(e("GetResource"))?;
        let sub = dx.GetSubresourceIndex().map_err(e("GetSubresourceIndex"))?;
        Ok((src.ok_or("немає текстури декодера")?, sub))
    }
}

/// Whether a decoded sample lives in GPU memory (DXVA) or in CPU memory.
pub fn on_gpu(sample: &IMFSample) -> bool {
    unsafe {
        sample
            .GetBufferByIndex(0)
            .is_ok_and(|b| b.cast::<IMFDXGIBuffer>().is_ok())
    }
}

fn luid_str(l: LUID) -> String {
    format!("{:08x}:{:08x}", l.HighPart as u32, l.LowPart)
}

impl Bridge {
    /// The D3D11 device and the decoder's device manager; `alloc` adds the texture pool once the
    /// frame size is known.
    pub fn new(gpu: &Gpu) -> Result<Self, String> {
        if !gpu.nv12 {
            return Err("адаптер wgpu не підтримує NV12-текстури (потрібен DX12)".into());
        }
        unsafe {
            let d12: ID3D12Device = gpu
                .device
                .as_hal::<Dx12>()
                .ok_or("wgpu не на DX12 (WGPU_BACKEND?)")?
                .raw_device()
                .clone();
            let luid = d12.GetAdapterLuid();
            let factory: IDXGIFactory4 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))
                .map_err(e("CreateDXGIFactory2"))?;
            let adapter: IDXGIAdapter1 = factory
                .EnumAdapterByLuid(luid)
                .map_err(e("EnumAdapterByLuid"))?;
            let desc = adapter.GetDesc1().map_err(e("GetDesc1"))?;
            let name = String::from_utf16_lossy(&desc.Description)
                .trim_end_matches('\0')
                .to_string();
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
            let device: ID3D11Device = device.ok_or("немає D3D11-пристрою")?;
            let ctx: ID3D11DeviceContext4 = ctx
                .ok_or("немає контексту")?
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
            let manager = manager.ok_or("немає менеджера")?;
            manager
                .ResetDevice(&device, token)
                .map_err(e("ResetDevice"))?;

            let dev5: ID3D11Device5 = device.cast().map_err(e("ID3D11Device5"))?;
            // Copy fence: created on D3D11, opened in D3D12.
            let mut copy11: Option<ID3D11Fence> = None;
            dev5.CreateFence(0, D3D11_FENCE_FLAG_SHARED, &mut copy11)
                .map_err(e("CreateFence11"))?;
            let copy11 = copy11.ok_or("fence")?;
            let h = copy11
                .CreateSharedHandle(None, GENERIC_ALL.0, PCWSTR::null())
                .map_err(e("fence handle"))?;
            let mut copy12: Option<ID3D12Fence> = None;
            let r = d12.OpenSharedHandle(h, &mut copy12);
            let _ = CloseHandle(h);
            r.map_err(e("OpenSharedHandle fence"))?;
            // Read fence: created on D3D12, opened in D3D11.
            let read12: ID3D12Fence = d12
                .CreateFence(0, D3D12_FENCE_FLAG_SHARED)
                .map_err(e("CreateFence12"))?;
            let h: HANDLE = d12
                .CreateSharedHandle(&read12, None, GENERIC_ALL.0, PCWSTR::null())
                .map_err(e("fence12 handle"))?;
            let mut read11: Option<ID3D11Fence> = None;
            let r = dev5.OpenSharedFence(h, &mut read11);
            let _ = CloseHandle(h);
            r.map_err(e("OpenSharedFence"))?;

            Ok(Self {
                device,
                d12,
                ctx,
                manager,
                copy11,
                copy12: copy12.ok_or("fence12")?,
                copy_value: 0,
                read11: read11.ok_or("fence11")?,
                read12,
                read_value: 0,
                slots: Vec::new(),
                next: 0,
                staging: Vec::new(),
                staged: Default::default(),
                stage_next: 0,
                width: 0,
                height: 0,
                adapter: name,
                luid: luid_str(luid),
            })
        }
    }

    /// Create `pool` shared NV12 textures of the frame size, each with its bind group into `out`.
    pub fn alloc(
        &mut self,
        gpu: &Gpu,
        out: &wgpu::Texture,
        width: u32,
        height: u32,
        pool: usize,
    ) -> Result<(), String> {
        self.width = width;
        self.height = height;
        self.slots.clear();
        unsafe {
            for _ in 0..pool {
                let td = D3D11_TEXTURE2D_DESC {
                    Width: width,
                    Height: height,
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
                    MiscFlags: (D3D11_RESOURCE_MISC_SHARED.0
                        | D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0)
                        as u32,
                };
                let mut tex11 = None;
                self.device
                    .CreateTexture2D(&td, None, Some(&mut tex11))
                    .map_err(e("CreateTexture2D NV12"))?;
                let tex11: ID3D11Texture2D = tex11.ok_or("texture")?;
                let h = tex11
                    .cast::<IDXGIResource1>()
                    .map_err(e("IDXGIResource1"))?
                    .CreateSharedHandle(
                        None,
                        (DXGI_SHARED_RESOURCE_READ | DXGI_SHARED_RESOURCE_WRITE).0,
                        PCWSTR::null(),
                    )
                    .map_err(e("CreateSharedHandle"))?;
                let mut res: Option<ID3D12Resource> = None;
                let r = self.d12.OpenSharedHandle(h, &mut res);
                let _ = CloseHandle(h);
                r.map_err(e("OpenSharedHandle texture"))?;
                let size = wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                };
                let hal = wgpu::hal::dx12::Device::texture_from_raw(
                    res.ok_or("resource")?,
                    wgpu::TextureFormat::NV12,
                    wgpu::TextureDimension::D2,
                    size,
                    1,
                    1,
                );
                let tex = gpu.device.create_texture_from_hal::<Dx12>(
                    hal,
                    &wgpu::TextureDescriptor {
                        label: Some("nv12 shared"),
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
                let bind = gpu.bind(
                    &view(wgpu::TextureFormat::R8Unorm, wgpu::TextureAspect::Plane0),
                    &view(wgpu::TextureFormat::Rg8Unorm, wgpu::TextureAspect::Plane1),
                    out,
                );
                self.slots.push(Slot {
                    tex11,
                    bind,
                    _wgpu: tex,
                    last_read: 0,
                });
            }
        }
        Ok(())
    }

    /// Fallback path: `count` staging textures of the frame size.
    pub fn alloc_staging(&mut self, width: u32, height: u32, count: usize) -> Result<(), String> {
        self.width = width;
        self.height = height;
        self.staging.clear();
        self.staged.clear();
        for _ in 0..count {
            let td = D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_NV12,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
            };
            let mut t = None;
            unsafe { self.device.CreateTexture2D(&td, None, Some(&mut t)) }
                .map_err(e("CreateTexture2D staging"))?;
            self.staging.push(t.ok_or("staging")?);
        }
        Ok(())
    }

    /// Fallback path, step 1: GPU copy of a decoded sample into the next staging texture.
    pub fn stage(&mut self, sample: &IMFSample) -> Result<(), String> {
        if self.staged.len() == self.staging.len() {
            return Err("усі staging-текстури зайняті — спершу unstage".into());
        }
        unsafe {
            let (src, sub) = decoder_texture(sample)?;
            let i = self.stage_next;
            self.stage_next = (i + 1) % self.staging.len();
            let bx = D3D11_BOX {
                left: 0,
                top: 0,
                front: 0,
                right: self.width,
                bottom: self.height,
                back: 1,
            };
            self.ctx
                .CopySubresourceRegion(&self.staging[i], 0, 0, 0, 0, &src, sub, Some(&bx));
            self.ctx.Flush();
            self.staged.push_back(i);
        }
        Ok(())
    }

    /// Fallback path, step 2: map the oldest staged copy and hand its planes straight to `put`
    /// (luma, chroma, row pitch) — no intermediate copy: reading the mapped memory is the cost.
    pub fn unstage(&mut self, put: impl FnOnce(&[u8], &[u8], u32)) -> Result<bool, String> {
        let Some(i) = self.staged.pop_front() else {
            return Ok(false);
        };
        unsafe {
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            self.ctx
                .Map(&self.staging[i], 0, D3D11_MAP_READ, 0, Some(&mut m))
                .map_err(e("Map staging"))?;
            let (h, pitch) = (self.height as usize, m.RowPitch as usize);
            let p = m.pData.cast::<u8>();
            let y = std::slice::from_raw_parts(p, pitch * h);
            let uv = std::slice::from_raw_parts(p.add(pitch * h), pitch * h / 2);
            put(y, uv, m.RowPitch);
            self.ctx.Unmap(&self.staging[i], 0);
        }
        Ok(true)
    }

    /// Copy a decoded sample into the next pool slot and queue the wgpu conversion of it.
    /// Nothing waits on the CPU: both directions are GPU-side fence waits.
    pub fn present(&mut self, gpu: &Gpu, sample: &IMFSample) -> Result<(), String> {
        unsafe {
            let (src, sub) = decoder_texture(sample)?;
            let n = self.slots.len();
            let slot = &mut self.slots[self.next];
            self.next = (self.next + 1) % n;
            if slot.last_read > 0 {
                self.ctx
                    .Wait(&self.read11, slot.last_read)
                    .map_err(e("Wait read"))?;
            }
            let bx = D3D11_BOX {
                left: 0,
                top: 0,
                front: 0,
                right: self.width,
                bottom: self.height,
                back: 1,
            };
            self.ctx
                .CopySubresourceRegion(&slot.tex11, 0, 0, 0, 0, &src, sub, Some(&bx));
            self.copy_value += 1;
            self.ctx
                .Signal(&self.copy11, self.copy_value)
                .map_err(e("Signal copy"))?;
            // Hand the copy to the GPU now, or the D3D12 wait could stall behind D3D11 batching.
            self.ctx.Flush();
            self.read_value += 1;
            slot.last_read = self.read_value;
            {
                let q = gpu.queue.as_hal::<Dx12>().ok_or("черга не DX12")?;
                q.add_wait_fence(self.copy12.clone(), self.copy_value);
                q.add_signal_fence(self.read12.clone(), self.read_value);
            }
            gpu.queue
                .submit([gpu.convert(&slot.bind, self.width, self.height)]);
            Ok(())
        }
    }
}
