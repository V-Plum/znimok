//! BGRA8 → NV12 on the GPU with the matrix we mean: the D3D11 Video Processor, told that the
//! input is full-range sRGB and the output is BT.709 studio range. (Media Foundation's own
//! converter, which the Sink Writer inserts for RGB input, picks BT.601 for small pictures
//! whatever the media types say — a 640×360 region came back with its colours 20 off.) The
//! encoder then takes NV12 straight from the allocator's pool: no converter in between.

use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709, DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
    DXGI_RATIONAL,
};
use windows::core::Interface;

use crate::mf::err;

/// Output views kept for the encoder's recycled textures (its pool has at most 16).
const VIEW_CACHE: usize = 32;

pub struct Nv12Converter {
    vdev: ID3D11VideoDevice,
    vctx: ID3D11VideoContext,
    enumerator: ID3D11VideoProcessorEnumerator,
    vp: ID3D11VideoProcessor,
    in_views: Vec<(usize, ID3D11VideoProcessorInputView)>,
    out_views: Vec<((usize, u32), ID3D11VideoProcessorOutputView)>,
}

impl Nv12Converter {
    pub fn new(
        device: &ID3D11Device,
        ctx: &ID3D11DeviceContext4,
        width: u32,
        height: u32,
        fps: u32,
    ) -> Result<Self, String> {
        // SAFETY: D3D11 video interfaces of the same device; the descriptors are plain values.
        unsafe {
            let vdev: ID3D11VideoDevice = device.cast().map_err(err("ID3D11VideoDevice"))?;
            let vctx: ID3D11VideoContext = ctx.cast().map_err(err("ID3D11VideoContext"))?;
            let rate = DXGI_RATIONAL {
                Numerator: fps.max(1),
                Denominator: 1,
            };
            let desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
                InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                InputFrameRate: rate,
                InputWidth: width,
                InputHeight: height,
                OutputFrameRate: rate,
                OutputWidth: width,
                OutputHeight: height,
                Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
            };
            let enumerator = vdev
                .CreateVideoProcessorEnumerator(&desc)
                .map_err(err("CreateVideoProcessorEnumerator"))?;
            let vp = vdev
                .CreateVideoProcessor(&enumerator, 0)
                .map_err(err("CreateVideoProcessor"))?;
            vctx.VideoProcessorSetStreamFrameFormat(&vp, 0, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE);
            // Colour spaces, the old way (bit fields: Usage 0, RGB_Range full, YCbCr_Matrix 709,
            // Nominal_Range 0–255 in / 16–235 out) and the Windows 10 way where it exists.
            let input = D3D11_VIDEO_PROCESSOR_COLOR_SPACE {
                _bitfield: (1 << 2) | (2 << 4),
            };
            let output = D3D11_VIDEO_PROCESSOR_COLOR_SPACE {
                _bitfield: (1 << 2) | (1 << 4),
            };
            vctx.VideoProcessorSetStreamColorSpace(&vp, 0, &input);
            vctx.VideoProcessorSetOutputColorSpace(&vp, &output);
            if let Ok(v1) = vctx.cast::<ID3D11VideoContext1>() {
                v1.VideoProcessorSetStreamColorSpace1(
                    &vp,
                    0,
                    DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709,
                );
                v1.VideoProcessorSetOutputColorSpace1(
                    &vp,
                    DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
                );
            }
            // No scaling, no letterboxing: the whole input onto the whole output.
            vctx.VideoProcessorSetStreamSourceRect(&vp, 0, false, None);
            vctx.VideoProcessorSetOutputTargetRect(&vp, false, None);
            Ok(Self {
                vdev,
                vctx,
                enumerator,
                vp,
                in_views: Vec::new(),
                out_views: Vec::new(),
            })
        }
    }

    fn in_view(&mut self, src: &ID3D11Texture2D) -> Result<ID3D11VideoProcessorInputView, String> {
        let key = src.as_raw() as usize;
        if let Some((_, v)) = self.in_views.iter().find(|(k, _)| *k == key) {
            return Ok(v.clone());
        }
        let desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            FourCC: 0,
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPIV {
                    MipSlice: 0,
                    ArraySlice: 0,
                },
            },
        };
        let mut view = None;
        // SAFETY: a view of a texture of this device with the enumerator it was made for.
        unsafe {
            self.vdev
                .CreateVideoProcessorInputView(src, &self.enumerator, &desc, Some(&mut view))
                .map_err(err("CreateVideoProcessorInputView"))?;
        }
        let view = view.ok_or("input view")?;
        if self.in_views.len() >= VIEW_CACHE {
            self.in_views.remove(0);
        }
        self.in_views.push((key, view.clone()));
        Ok(view)
    }

    fn out_view(
        &mut self,
        dst: &ID3D11Texture2D,
        sub: u32,
    ) -> Result<ID3D11VideoProcessorOutputView, String> {
        let key = (dst.as_raw() as usize, sub);
        if let Some((_, v)) = self.out_views.iter().find(|(k, _)| *k == key) {
            return Ok(v.clone());
        }
        let mut td = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: description read; the view of an array slice or of the plain texture.
        unsafe {
            dst.GetDesc(&mut td);
        }
        let desc = if td.ArraySize > 1 || sub > 0 {
            D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2DARRAY,
                Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                    Texture2DArray: D3D11_TEX2D_ARRAY_VPOV {
                        MipSlice: 0,
                        FirstArraySlice: sub,
                        ArraySize: 1,
                    },
                },
            }
        } else {
            D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                    Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
                },
            }
        };
        let mut view = None;
        // SAFETY: as above.
        unsafe {
            self.vdev
                .CreateVideoProcessorOutputView(dst, &self.enumerator, &desc, Some(&mut view))
                .map_err(err("CreateVideoProcessorOutputView"))?;
        }
        let view = view.ok_or("output view")?;
        if self.out_views.len() >= VIEW_CACHE {
            self.out_views.remove(0);
        }
        self.out_views.push((key, view.clone()));
        Ok(view)
    }

    /// Convert `src` (BGRA8) into slice `dst_sub` of `dst` (NV12), on the immediate context.
    pub fn blt(
        &mut self,
        src: &ID3D11Texture2D,
        dst: &ID3D11Texture2D,
        dst_sub: u32,
    ) -> Result<(), String> {
        let input = self.in_view(src)?;
        let output = self.out_view(dst, dst_sub)?;
        let stream = D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: true.into(),
            OutputIndex: 0,
            InputFrameOrField: 0,
            PastFrames: 0,
            FutureFrames: 0,
            ppPastSurfaces: std::ptr::null_mut(),
            pInputSurface: std::mem::ManuallyDrop::new(Some(input)),
            ppFutureSurfaces: std::ptr::null_mut(),
            ppPastSurfacesRight: std::ptr::null_mut(),
            pInputSurfaceRight: std::mem::ManuallyDrop::new(None),
            ppFutureSurfacesRight: std::ptr::null_mut(),
        };
        // SAFETY: views of live textures; the stream holds a reference the struct hands over
        // (ManuallyDrop: the call does not consume it, the cache keeps the view alive).
        let r = unsafe { self.vctx.VideoProcessorBlt(&self.vp, &output, 0, &[stream]) };
        r.map_err(err("VideoProcessorBlt"))
    }
}
