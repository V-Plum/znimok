//! Media Foundation: the test clip generator (Sink Writer, H.264 High, GOP = fps and no B-frames,
//! as Little Helpers records) and the Source Reader that playback reads from.

use crate::pattern::Nv12;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::Win32::System::Variant::VT_I8;
use windows::core::{GUID, HSTRING, Interface};

/// CODECAPI_AVEncMPVGOPSize and CODECAPI_AVEncMPVDefaultBPictureCount (not every header set has codecapi.h).
const GOP_SIZE: GUID = GUID::from_u128(0x95f31b26_95a4_41aa_9303_246a7fc6eef1);
const B_COUNT: GUID = GUID::from_u128(0x8d390aac_dc5c_4200_b57f_814d04bab2b2);
pub const FIRST_VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

pub fn startup() -> Result<(), String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        MFStartup(MF_VERSION, MFSTARTUP_FULL).map_err(|e| format!("MFStartup: {e}"))
    }
}

fn e(what: &str) -> impl Fn(windows::core::Error) -> String + '_ {
    move |err| format!("{what}: {err}")
}

fn size(t: &IMFMediaType, key: &GUID) -> Option<(u32, u32)> {
    unsafe { t.GetUINT64(key).ok().map(|v| ((v >> 32) as u32, v as u32)) }
}

fn set_size(t: &IMFMediaType, key: &GUID, a: u32, b: u32) -> windows::core::Result<()> {
    unsafe { t.SetUINT64(key, (u64::from(a) << 32) | u64::from(b)) }
}

/// Friendly name and hardware flag of the transform at `index` of a stream (decoder or encoder).
fn transform_name(t: &IMFTransform) -> (String, bool) {
    unsafe {
        let Ok(a) = t.GetAttributes() else {
            return ("?".into(), false);
        };
        let hw = a.GetStringLength(&MFT_ENUM_HARDWARE_URL_Attribute).is_ok();
        let mut name = String::from("?");
        if let Ok(n) = a.GetStringLength(&MFT_FRIENDLY_NAME_Attribute) {
            let mut buf = vec![0u16; n as usize + 1];
            if a.GetString(&MFT_FRIENDLY_NAME_Attribute, &mut buf, None)
                .is_ok()
            {
                name = String::from_utf16_lossy(&buf[..n as usize]);
            }
        }
        (name, hw)
    }
}

pub struct GenParams {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub frames: u32,
    pub gop: u32,
    pub bits_per_pixel: f64,
}

/// Write the synthetic clip. Returns the encoder's name and whether it was hardware.
pub fn generate(path: &str, p: &GenParams) -> Result<serde_json::Value, String> {
    let t0 = std::time::Instant::now();
    unsafe {
        let mut attrs = None;
        MFCreateAttributes(&mut attrs, 3).map_err(e("MFCreateAttributes"))?;
        let attrs = attrs.unwrap();
        attrs
            .SetGUID(&MF_TRANSCODE_CONTAINERTYPE, &MFTranscodeContainerType_MPEG4)
            .map_err(e("attr"))?;
        attrs
            .SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)
            .map_err(e("attr"))?;
        attrs
            .SetUINT32(&MF_SINK_WRITER_DISABLE_THROTTLING, 1)
            .map_err(e("attr"))?;
        let writer = MFCreateSinkWriterFromURL(&HSTRING::from(path), None, &attrs)
            .map_err(e("MFCreateSinkWriterFromURL"))?;

        let out = MFCreateMediaType().map_err(e("MFCreateMediaType"))?;
        out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(e("out"))?;
        out.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)
            .map_err(e("out"))?;
        let bitrate =
            f64::from(p.width) * f64::from(p.height) * f64::from(p.fps) * p.bits_per_pixel;
        out.SetUINT32(&MF_MT_AVG_BITRATE, bitrate as u32)
            .map_err(e("out"))?;
        out.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
            .map_err(e("out"))?;
        out.SetUINT32(&MF_MT_MPEG2_PROFILE, 100).map_err(e("out"))?; // High, as in LH
        out.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32)
            .map_err(e("out"))?;
        out.SetUINT32(&MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0 as u32)
            .map_err(e("out"))?;
        out.SetUINT32(&MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0 as u32)
            .map_err(e("out"))?;
        out.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32)
            .map_err(e("out"))?;
        set_size(&out, &MF_MT_FRAME_SIZE, p.width, p.height).map_err(e("out"))?;
        set_size(&out, &MF_MT_FRAME_RATE, p.fps, 1).map_err(e("out"))?;
        set_size(&out, &MF_MT_PIXEL_ASPECT_RATIO, 1, 1).map_err(e("out"))?;
        let stream = writer.AddStream(&out).map_err(e("AddStream"))?;

        let inp = MFCreateMediaType().map_err(e("MFCreateMediaType"))?;
        inp.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(e("in"))?;
        inp.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)
            .map_err(e("in"))?;
        inp.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
            .map_err(e("in"))?;
        inp.SetUINT32(&MF_MT_DEFAULT_STRIDE, p.width)
            .map_err(e("in"))?;
        inp.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32)
            .map_err(e("in"))?;
        inp.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32)
            .map_err(e("in"))?;
        set_size(&inp, &MF_MT_FRAME_SIZE, p.width, p.height).map_err(e("in"))?;
        set_size(&inp, &MF_MT_FRAME_RATE, p.fps, 1).map_err(e("in"))?;
        set_size(&inp, &MF_MT_PIXEL_ASPECT_RATIO, 1, 1).map_err(e("in"))?;
        let mut ep = None;
        MFCreateAttributes(&mut ep, 2).map_err(e("MFCreateAttributes"))?;
        let ep = ep.unwrap();
        ep.SetUINT32(&GOP_SIZE, p.gop).map_err(e("gop"))?;
        ep.SetUINT32(&B_COUNT, 0).map_err(e("bcount"))?;
        writer
            .SetInputMediaType(stream, &inp, &ep)
            .map_err(e("SetInputMediaType"))?;
        let (encoder, hw) = writer
            .cast::<IMFSinkWriterEx>()
            .ok()
            .and_then(|w| {
                let mut cat = GUID::zeroed();
                let mut t = None;
                w.GetTransformForStream(stream, 0, Some(&mut cat), &mut t)
                    .ok()?;
                t.map(|t| transform_name(&t))
            })
            .unwrap_or(("?".into(), false));
        writer.BeginWriting().map_err(e("BeginWriting"))?;

        let mut frame = Nv12::new(p.width, p.height);
        let len = frame.data.len() as u32;
        let dur = 10_000_000i64 / i64::from(p.fps);
        for i in 0..p.frames {
            frame.draw(i);
            let buf = MFCreateMemoryBuffer(len).map_err(e("MFCreateMemoryBuffer"))?;
            let mut ptr = std::ptr::null_mut();
            buf.Lock(&mut ptr, None, None).map_err(e("Lock"))?;
            std::ptr::copy_nonoverlapping(frame.data.as_ptr(), ptr, frame.data.len());
            buf.Unlock().map_err(e("Unlock"))?;
            buf.SetCurrentLength(len).map_err(e("SetCurrentLength"))?;
            let s = MFCreateSample().map_err(e("MFCreateSample"))?;
            s.AddBuffer(&buf).map_err(e("AddBuffer"))?;
            // Exact rational time, so frame i always rounds back to i.
            s.SetSampleTime(i64::from(i) * 10_000_000 / i64::from(p.fps))
                .map_err(e("time"))?;
            s.SetSampleDuration(dur).map_err(e("dur"))?;
            writer.WriteSample(stream, &s).map_err(e("WriteSample"))?;
        }
        writer.Finalize().map_err(e("Finalize"))?;
        let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        Ok(serde_json::json!({
            "file": path, "size": [p.width, p.height], "fps": p.fps, "frames": p.frames, "gop": p.gop,
            "encoder": encoder, "encoder_hardware": hw, "megabytes": bytes / 1_000_000,
            "mbit_s": (bytes as f64 * 8.0 / (f64::from(p.frames) / f64::from(p.fps)) / 1e6).round(),
            "seconds_to_write": (t0.elapsed().as_secs_f64() * 10.0).round() / 10.0,
        }))
    }
}

/// How decoded frames leave the decoder.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// DXVA decoder on the wgpu adapter, frames stay on the GPU (the P4 path).
    Zero,
    /// Fallback: same DXVA decoder, our own GPU copy into a staging ring, mapped two frames later
    /// and uploaded to wgpu.
    Cpu,
    /// The naive fallback: MF locks every decoded texture into CPU memory itself (synchronous).
    MfLock,
    /// No D3D device at all: MF picks a software decoder, frames in CPU memory.
    Software,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "zero" => Some(Self::Zero),
            "cpu" => Some(Self::Cpu),
            "mflock" => Some(Self::MfLock),
            "sw" => Some(Self::Software),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Zero => "zero",
            Self::Cpu => "cpu",
            Self::MfLock => "mflock",
            Self::Software => "sw",
        }
    }
}

pub struct Reader {
    reader: IMFSourceReader,
    pub width: u32,
    pub height: u32,
    /// Allocated height of CPU-memory frames (the chroma plane starts at pitch × this).
    pub alloc_height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    pub duration_100ns: i64,
    pub low_latency: bool,
}

pub struct Decoded {
    pub timestamp: i64,
    pub sample: IMFSample,
}

impl Reader {
    pub fn open(
        path: &str,
        manager: Option<&IMFDXGIDeviceManager>,
        low_latency: bool,
    ) -> Result<Self, String> {
        unsafe {
            let mut attrs = None;
            MFCreateAttributes(&mut attrs, 3).map_err(e("MFCreateAttributes"))?;
            let attrs = attrs.unwrap();
            if let Some(m) = manager {
                attrs
                    .SetUnknown(&MF_SOURCE_READER_D3D_MANAGER, m)
                    .map_err(e("attr"))?;
                attrs
                    .SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)
                    .map_err(e("attr"))?;
            }
            if low_latency {
                // Asks the decoder to output each frame as soon as it is decoded (no reorder queue).
                attrs.SetUINT32(&MF_LOW_LATENCY, 1).map_err(e("attr"))?;
            }
            let reader = MFCreateSourceReaderFromURL(&HSTRING::from(path), &attrs)
                .map_err(e("MFCreateSourceReaderFromURL"))?;
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
            let duration_100ns = reader
                .GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)
                .map(|v| v.Anonymous.Anonymous.Anonymous.hVal)
                .unwrap_or(0);
            let mut me = Self {
                reader,
                width: 0,
                height: 0,
                alloc_height: 0,
                fps_num: 0,
                fps_den: 1,
                duration_100ns,
                low_latency,
            };
            me.refresh_type()?;
            Ok(me)
        }
    }

    fn refresh_type(&mut self) -> Result<(), String> {
        unsafe {
            let t = self
                .reader
                .GetCurrentMediaType(FIRST_VIDEO)
                .map_err(e("GetCurrentMediaType"))?;
            let (w, h) = size(&t, &MF_MT_FRAME_SIZE).ok_or("немає MF_MT_FRAME_SIZE")?;
            self.alloc_height = h;
            let (mut dw, mut dh) = (w, h);
            // The decoder may allocate 2176 rows for 2160 (16-row macroblocks); the aperture says what to show.
            let mut area = MFVideoArea::default();
            let blob = std::slice::from_raw_parts_mut(
                (&raw mut area).cast::<u8>(),
                std::mem::size_of::<MFVideoArea>(),
            );
            if t.GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, blob, None)
                .is_ok()
            {
                dw = area.Area.cx as u32;
                dh = area.Area.cy as u32;
            }
            self.width = dw;
            self.height = dh;
            if let Some((n, d)) = size(&t, &MF_MT_FRAME_RATE) {
                self.fps_num = n;
                self.fps_den = d.max(1);
            }
            Ok(())
        }
    }

    pub fn fps(&self) -> f64 {
        f64::from(self.fps_num) / f64::from(self.fps_den)
    }

    /// Frame index of a timestamp (100 ns units).
    pub fn index_of(&self, ts: i64) -> u32 {
        (ts as f64 * self.fps() / 1e7).round() as u32
    }

    pub fn time_of(&self, index: u32) -> i64 {
        (f64::from(index) * 1e7 / self.fps()).round() as i64
    }

    pub fn frame_count(&self) -> u32 {
        self.index_of(self.duration_100ns)
    }

    /// Next decoded frame, `None` at the end of the stream.
    pub fn next(&mut self) -> Result<Option<Decoded>, String> {
        loop {
            let (mut flags, mut ts, mut sample) = (0u32, 0i64, None);
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
            if let Some(sample) = sample {
                return Ok(Some(Decoded {
                    timestamp: ts,
                    sample,
                }));
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                return Ok(None);
            }
        }
    }

    /// Jump to `index`: the reader lands on the key frame at or before it, then frames are decoded
    /// forward until the wanted one. Returns it and how many frames were decoded on the way.
    pub fn seek(&mut self, index: u32) -> Result<(Decoded, u32), String> {
        let v = PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VT_I8,
                    Anonymous: windows::Win32::System::Com::StructuredStorage::PROPVARIANT_0_0_0 {
                        hVal: self.time_of(index),
                    },
                    ..Default::default()
                }),
            },
        };
        unsafe {
            self.reader
                .SetCurrentPosition(&GUID::zeroed(), &v)
                .map_err(e("SetCurrentPosition"))?;
        }
        let mut decoded = 0;
        loop {
            let f = self.next()?.ok_or("кінець потоку до потрібного кадру")?;
            decoded += 1;
            if self.index_of(f.timestamp) >= index {
                return Ok((f, decoded));
            }
        }
    }
}

/// Luma and chroma of a CPU-memory frame: copies the two planes out of a locked 2D buffer.
pub fn lock_planes(
    sample: &IMFSample,
    width: u32,
    height: u32,
    alloc_height: u32,
    y: &mut Vec<u8>,
    uv: &mut Vec<u8>,
) -> Result<(), String> {
    unsafe {
        let buf = sample.GetBufferByIndex(0).map_err(e("GetBufferByIndex"))?;
        // A DXGI buffer maps the whole texture: its chroma starts after the texture's rows.
        let rows = match buf.cast::<IMFDXGIBuffer>() {
            Ok(d) => {
                let mut tex: Option<windows::Win32::Graphics::Direct3D11::ID3D11Texture2D> = None;
                d.GetResource(
                    &windows::Win32::Graphics::Direct3D11::ID3D11Texture2D::IID,
                    (&raw mut tex).cast(),
                )
                .map_err(e("GetResource"))?;
                let mut desc = Default::default();
                tex.ok_or("немає текстури")?.GetDesc(&mut desc);
                desc.Height
            }
            Err(_) => alloc_height,
        };
        let b2 = buf.cast::<IMF2DBuffer>().map_err(e("IMF2DBuffer"))?;
        let (mut p, mut pitch) = (std::ptr::null_mut(), 0i32);
        b2.Lock2D(&mut p, &mut pitch).map_err(e("Lock2D"))?;
        let pitch = pitch as usize;
        let w = width as usize;
        y.clear();
        uv.clear();
        for r in 0..height as usize {
            y.extend_from_slice(std::slice::from_raw_parts(p.add(r * pitch), w));
        }
        let c0 = p.add(pitch * rows as usize);
        for r in 0..(height / 2) as usize {
            uv.extend_from_slice(std::slice::from_raw_parts(c0.add(r * pitch), w));
        }
        b2.Unlock2D().map_err(e("Unlock2D"))?;
        Ok(())
    }
}
