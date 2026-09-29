//! A Source Reader decoder for checks, thumbnails and export (NV12 frames in CPU memory).
//! Playback without copying is ZK-92 (prototype P4 has the GPU path).
//!
//! The row pitch traps of §7 items 5–6: the decoder aligns width AND height (a 1918×1050
//! recording arrives in a 1920×1056 buffer); the display aperture says what to show; the planes
//! are read through `IMF2DBuffer` with its own pitch, and the chroma plane starts after the
//! ALLOCATED rows, not the displayed ones.

use std::path::Path;

use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0};
use windows::Win32::System::Variant::VT_I8;
use windows::core::{GUID, HSTRING, Interface};
use znimok_video::traits::{Decoded, StreamInfo, VideoDecoder};
use znimok_video::{Result, VideoError};

use crate::mf::{err, get_size};

const FIRST_VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
const FIRST_AUDIO: u32 = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;

/// One decoded frame, planes tightly packed (`y`: w × h, `uv`: w × h/2 interleaved).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Nv12Frame {
    pub width: u32,
    pub height: u32,
    pub y: Vec<u8>,
    pub uv: Vec<u8>,
}

impl Nv12Frame {
    /// Luma at a pixel.
    pub fn luma(&self, x: u32, y: u32) -> u8 {
        self.y[(y * self.width + x) as usize]
    }

    /// Y, Cb, Cr of a pixel (chroma of its 2×2 block).
    pub fn ycc(&self, x: u32, y: u32) -> [u8; 3] {
        let c = ((y / 2) * self.width + (x / 2) * 2) as usize;
        [self.luma(x, y), self.uv[c], self.uv[c + 1]]
    }
}

pub struct MfDecoder {
    reader: IMFSourceReader,
    info: StreamInfo,
    alloc_height: u32,
}

fn d(e: String) -> VideoError {
    VideoError::Decode(e)
}

impl MfDecoder {
    pub fn open(path: &Path) -> Result<Self> {
        crate::mf::startup().map_err(d)?;
        let abs = std::path::absolute(path).map_err(|e| d(e.to_string()))?;
        // SAFETY: Media Foundation calls with owned interfaces.
        unsafe {
            // No MF_LOW_LATENCY here: with it the H.264 decoder handed out the first frame
            // twice at time 0 and every later one a frame early.
            let reader = MFCreateSourceReaderFromURL(&HSTRING::from(abs.as_os_str()), None)
                .map_err(err("MFCreateSourceReaderFromURL"))
                .map_err(d)?;
            let _ = reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false);
            reader
                .SetStreamSelection(FIRST_VIDEO, true)
                .map_err(err("SetStreamSelection"))
                .map_err(d)?;
            let audio = reader.GetNativeMediaType(FIRST_AUDIO, 0).is_ok();
            let t = MFCreateMediaType()
                .map_err(err("MFCreateMediaType"))
                .map_err(d)?;
            t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .map_err(err("type"))
                .map_err(d)?;
            t.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)
                .map_err(err("type"))
                .map_err(d)?;
            reader
                .SetCurrentMediaType(FIRST_VIDEO, None, &t)
                .map_err(err("SetCurrentMediaType NV12"))
                .map_err(d)?;
            let duration_hns = reader
                .GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)
                .map(|v| v.Anonymous.Anonymous.Anonymous.hVal)
                .unwrap_or(0);
            let mut me = Self {
                reader,
                info: StreamInfo {
                    width: 0,
                    height: 0,
                    fps: 0.0,
                    duration_hns,
                    audio,
                },
                alloc_height: 0,
            };
            me.refresh_type()?;
            Ok(me)
        }
    }

    fn refresh_type(&mut self) -> Result<()> {
        // SAFETY: reads of the current media type.
        unsafe {
            let t = self
                .reader
                .GetCurrentMediaType(FIRST_VIDEO)
                .map_err(err("GetCurrentMediaType"))
                .map_err(d)?;
            let (w, h) = get_size(&t, &MF_MT_FRAME_SIZE)
                .ok_or_else(|| d("немає MF_MT_FRAME_SIZE".into()))?;
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
            self.info.width = dw;
            self.info.height = dh;
            if let Some((n, den)) = get_size(&t, &MF_MT_FRAME_RATE) {
                self.info.fps = n as f64 / den.max(1) as f64;
            }
        }
        Ok(())
    }

    fn planes(&self, sample: &IMFSample) -> Result<Nv12Frame> {
        let (w, h) = (self.info.width, self.info.height);
        // SAFETY: the buffer is locked for the copy and unlocked; reads stay within the pitch ×
        // allocated rows the decoder gave.
        unsafe {
            let buf = sample
                .GetBufferByIndex(0)
                .map_err(err("GetBufferByIndex"))
                .map_err(d)?;
            let mut y = Vec::with_capacity((w * h) as usize);
            let mut uv = Vec::with_capacity((w * h / 2) as usize);
            match buf.cast::<IMF2DBuffer>() {
                Ok(b2) => {
                    let (mut p, mut pitch) = (std::ptr::null_mut(), 0i32);
                    b2.Lock2D(&mut p, &mut pitch)
                        .map_err(err("Lock2D"))
                        .map_err(d)?;
                    let pitch = pitch.unsigned_abs() as usize;
                    for r in 0..h as usize {
                        y.extend_from_slice(std::slice::from_raw_parts(
                            p.add(r * pitch),
                            w as usize,
                        ));
                    }
                    let c0 = p.add(pitch * self.alloc_height as usize);
                    for r in 0..(h / 2) as usize {
                        uv.extend_from_slice(std::slice::from_raw_parts(
                            c0.add(r * pitch),
                            w as usize,
                        ));
                    }
                    let _ = b2.Unlock2D();
                }
                Err(_) => {
                    let (mut p, mut max, mut cur) = (std::ptr::null_mut(), 0u32, 0u32);
                    buf.Lock(&mut p, Some(&mut max), Some(&mut cur))
                        .map_err(err("Lock"))
                        .map_err(d)?;
                    let len = cur as usize;
                    let pitch =
                        znimok_video::stride::derive_stride(len * 4 / 6, w, h, w as usize * 4) / 4;
                    let rows = len / pitch.max(1) * 2 / 3;
                    let data = std::slice::from_raw_parts(p, len);
                    for r in 0..h as usize {
                        y.extend_from_slice(&data[r * pitch..r * pitch + w as usize]);
                    }
                    let c0 = pitch * rows.max(h as usize);
                    for r in 0..(h / 2) as usize {
                        let o = c0 + r * pitch;
                        uv.extend_from_slice(&data[o..o + w as usize]);
                    }
                    let _ = buf.Unlock();
                }
            }
            Ok(Nv12Frame {
                width: w,
                height: h,
                y,
                uv,
            })
        }
    }
}

impl VideoDecoder for MfDecoder {
    type Frame = Nv12Frame;

    fn info(&self) -> &StreamInfo {
        &self.info
    }

    fn seek(&mut self, time_hns: i64) -> Result<()> {
        let v = PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VT_I8,
                    Anonymous: windows::Win32::System::Com::StructuredStorage::PROPVARIANT_0_0_0 {
                        hVal: time_hns,
                    },
                    ..Default::default()
                }),
            },
        };
        // SAFETY: a position set on the live reader.
        unsafe {
            self.reader
                .SetCurrentPosition(&GUID::zeroed(), &v)
                .map_err(err("SetCurrentPosition"))
                .map_err(d)
        }
    }

    fn next(&mut self) -> Result<Option<Decoded<Nv12Frame>>> {
        loop {
            let (mut flags, mut ts, mut sample) = (0u32, 0i64, None);
            // SAFETY: out-pointers to locals.
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
                    .map_err(err("ReadSample"))
                    .map_err(d)?;
            }
            if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0 {
                self.refresh_type()?;
            }
            if let Some(sample) = sample {
                // SAFETY: a read on the live sample.
                let duration_hns = unsafe { sample.GetSampleDuration() }.unwrap_or(0);
                let frame = self.planes(&sample)?;
                return Ok(Some(Decoded::Video {
                    time_hns: ts,
                    duration_hns,
                    frame,
                }));
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                return Ok(None);
            }
        }
    }
}
