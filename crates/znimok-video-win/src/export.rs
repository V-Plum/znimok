//! Export on Windows (ZK-95): an MP4 writer fed from memory (NV12 frames the exporter made on the
//! CPU — the frame, the marks, the tone and the size are already in them) with an AAC track of the
//! mixed sound, and a reader of a recording's audio tracks as PCM.
//!
//! The encoder is the one recording uses (H.264 High, BT.709, studio range, GOP = fps, no
//! B-frames — the software encoder puts B-frames in and the frames come out one late without an
//! edit list, LH), hardware first, the software encoder when the hardware one does not take memory
//! input. `MF_LOW_LATENCY` on the writer keeps the first frame at 0 (never on a reader — the
//! traps in the memory note `znimok-video-win-mf-traps`).

use std::path::Path;

use windows::Win32::Media::MediaFoundation::*;
use windows::core::{GUID, HSTRING};

use crate::mf::{B_COUNT, GOP_SIZE, err, set_size};

/// Sound of the export: 48 kHz stereo, as the recorder writes it.
pub const RATE: u32 = 48_000;
pub const CHANNELS: u32 = 2;

pub struct Mp4Writer {
    writer: IMFSinkWriter,
    video: u32,
    audio: Option<u32>,
    width: u32,
    height: u32,
    /// Whether a hardware encoder took the frames.
    pub hardware: bool,
}

/// What the writer makes.
#[derive(Clone, Copy, Debug)]
pub struct Mp4Config {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Bits per second.
    pub bitrate: u32,
    /// An AAC track for the mixed sound.
    pub audio: bool,
}

fn media_types(cfg: &Mp4Config) -> Result<(IMFMediaType, IMFMediaType), String> {
    // SAFETY: attribute writes on freshly created media types.
    unsafe {
        let out = MFCreateMediaType().map_err(err("MFCreateMediaType"))?;
        out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(err("out"))?;
        out.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)
            .map_err(err("out"))?;
        out.SetUINT32(&MF_MT_AVG_BITRATE, cfg.bitrate)
            .map_err(err("out"))?;
        out.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
            .map_err(err("out"))?;
        out.SetUINT32(&MF_MT_MPEG2_PROFILE, 100)
            .map_err(err("out"))?;
        out.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32)
            .map_err(err("out"))?;
        out.SetUINT32(&MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0 as u32)
            .map_err(err("out"))?;
        out.SetUINT32(&MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0 as u32)
            .map_err(err("out"))?;
        out.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32)
            .map_err(err("out"))?;
        set_size(&out, &MF_MT_FRAME_SIZE, cfg.width, cfg.height).map_err(err("out"))?;
        set_size(&out, &MF_MT_FRAME_RATE, cfg.fps, 1).map_err(err("out"))?;
        set_size(&out, &MF_MT_PIXEL_ASPECT_RATIO, 1, 1).map_err(err("out"))?;
        let inp = MFCreateMediaType().map_err(err("MFCreateMediaType"))?;
        inp.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(err("in"))?;
        inp.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)
            .map_err(err("in"))?;
        inp.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
            .map_err(err("in"))?;
        inp.SetUINT32(&MF_MT_DEFAULT_STRIDE, cfg.width)
            .map_err(err("in"))?;
        inp.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32)
            .map_err(err("in"))?;
        inp.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32)
            .map_err(err("in"))?;
        set_size(&inp, &MF_MT_FRAME_SIZE, cfg.width, cfg.height).map_err(err("in"))?;
        set_size(&inp, &MF_MT_FRAME_RATE, cfg.fps, 1).map_err(err("in"))?;
        set_size(&inp, &MF_MT_PIXEL_ASPECT_RATIO, 1, 1).map_err(err("in"))?;
        Ok((out, inp))
    }
}

fn audio_types() -> Result<(IMFMediaType, IMFMediaType), String> {
    // SAFETY: attribute writes on freshly created media types.
    unsafe {
        let out = MFCreateMediaType().map_err(err("MFCreateMediaType"))?;
        out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)
            .map_err(err("aac"))?;
        out.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC)
            .map_err(err("aac"))?;
        out.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)
            .map_err(err("aac"))?;
        out.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, RATE)
            .map_err(err("aac"))?;
        out.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, CHANNELS)
            .map_err(err("aac"))?;
        out.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 160_000 / 8)
            .map_err(err("aac"))?;
        let inp = MFCreateMediaType().map_err(err("MFCreateMediaType"))?;
        inp.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)
            .map_err(err("pcm"))?;
        inp.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)
            .map_err(err("pcm"))?;
        inp.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)
            .map_err(err("pcm"))?;
        inp.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, RATE)
            .map_err(err("pcm"))?;
        inp.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, CHANNELS)
            .map_err(err("pcm"))?;
        inp.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, CHANNELS * 2)
            .map_err(err("pcm"))?;
        inp.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, RATE * CHANNELS * 2)
            .map_err(err("pcm"))?;
        Ok((out, inp))
    }
}

fn memory_sample(bytes: &[u8], time_hns: i64, dur_hns: i64) -> Result<IMFSample, String> {
    // SAFETY: a new memory buffer filled within its length, wrapped into a new sample.
    unsafe {
        let buf = MFCreateMemoryBuffer(bytes.len() as u32).map_err(err("MFCreateMemoryBuffer"))?;
        let mut p = std::ptr::null_mut();
        buf.Lock(&mut p, None, None).map_err(err("Lock"))?;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
        buf.Unlock().map_err(err("Unlock"))?;
        buf.SetCurrentLength(bytes.len() as u32)
            .map_err(err("SetCurrentLength"))?;
        let s = MFCreateSample().map_err(err("MFCreateSample"))?;
        s.AddBuffer(&buf).map_err(err("AddBuffer"))?;
        s.SetSampleTime(time_hns).map_err(err("SetSampleTime"))?;
        s.SetSampleDuration(dur_hns.max(1))
            .map_err(err("SetSampleDuration"))?;
        Ok(s)
    }
}

impl Mp4Writer {
    /// A writer of `path` (the `.part` file); the hardware encoder when it takes memory frames,
    /// else the software one.
    pub fn open(path: &Path, cfg: &Mp4Config) -> Result<Self, String> {
        if cfg.width < 2
            || cfg.height < 2
            || !cfg.width.is_multiple_of(2)
            || !cfg.height.is_multiple_of(2)
        {
            return Err(format!("{}×{} must be even", cfg.width, cfg.height));
        }
        crate::mf::startup()?;
        match Self::open_with(path, cfg, true) {
            Ok(w) => Ok(w),
            Err(e) => {
                tracing::warn!("export: hardware encoder: {e}; the software one");
                let _ = std::fs::remove_file(path);
                Self::open_with(path, cfg, false)
            }
        }
    }

    fn open_with(path: &Path, cfg: &Mp4Config, hardware: bool) -> Result<Self, String> {
        let abs = std::path::absolute(path).map_err(|e| e.to_string())?;
        // SAFETY: Media Foundation calls with owned interfaces.
        unsafe {
            let mut attrs = None;
            MFCreateAttributes(&mut attrs, 3).map_err(err("MFCreateAttributes"))?;
            let attrs = attrs.ok_or("attrs")?;
            // The extension `.part` says nothing about the container.
            attrs
                .SetGUID(&MF_TRANSCODE_CONTAINERTYPE, &MFTranscodeContainerType_MPEG4)
                .map_err(err("attr"))?;
            attrs.SetUINT32(&MF_LOW_LATENCY, 1).map_err(err("attr"))?;
            attrs
                .SetUINT32(
                    &MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS,
                    u32::from(hardware),
                )
                .map_err(err("attr"))?;
            let writer = MFCreateSinkWriterFromURL(&HSTRING::from(abs.as_os_str()), None, &attrs)
                .map_err(err("MFCreateSinkWriterFromURL"))?;
            let (out, inp) = media_types(cfg)?;
            let video = writer.AddStream(&out).map_err(err("AddStream (video)"))?;
            let mut ep = None;
            MFCreateAttributes(&mut ep, 2).map_err(err("MFCreateAttributes"))?;
            let ep = ep.ok_or("encoder params")?;
            ep.SetUINT32(&GOP_SIZE, cfg.fps.max(1))
                .map_err(err("gop"))?;
            ep.SetUINT32(&B_COUNT, 0).map_err(err("bcount"))?;
            writer
                .SetInputMediaType(video, &inp, &ep)
                .map_err(err("SetInputMediaType (video)"))?;
            let audio = if cfg.audio {
                let (ao, ai) = audio_types()?;
                let s = writer.AddStream(&ao).map_err(err("AddStream (audio)"))?;
                writer
                    .SetInputMediaType(s, &ai, None)
                    .map_err(err("SetInputMediaType (audio)"))?;
                Some(s)
            } else {
                None
            };
            writer.BeginWriting().map_err(err("BeginWriting"))?;
            Ok(Self {
                writer,
                video,
                audio,
                width: cfg.width,
                height: cfg.height,
                hardware,
            })
        }
    }

    /// One frame: NV12, luma rows then interleaved chroma rows, `width` bytes each.
    pub fn video(&mut self, nv12: &[u8], time_hns: i64, dur_hns: i64) -> Result<(), String> {
        let want = (self.width * self.height * 3 / 2) as usize;
        if nv12.len() != want {
            return Err(format!("frame of {} bytes, {want} wanted", nv12.len()));
        }
        let s = memory_sample(nv12, time_hns, dur_hns)?;
        // SAFETY: a sample written to our own stream.
        unsafe { self.writer.WriteSample(self.video, &s) }.map_err(err("WriteSample (video)"))
    }

    /// Interleaved stereo PCM at [`RATE`] starting at `time_hns`.
    pub fn audio(&mut self, pcm: &[i16], time_hns: i64) -> Result<(), String> {
        let Some(stream) = self.audio else {
            return Ok(());
        };
        if pcm.is_empty() {
            return Ok(());
        }
        let frames = (pcm.len() / CHANNELS as usize) as i64;
        let dur = frames * 10_000_000 / i64::from(RATE);
        let bytes: Vec<u8> = pcm.iter().flat_map(|v| v.to_le_bytes()).collect();
        let s = memory_sample(&bytes, time_hns, dur)?;
        // SAFETY: a sample written to our own stream.
        unsafe { self.writer.WriteSample(stream, &s) }.map_err(err("WriteSample (audio)"))
    }

    /// Finishes the file.
    pub fn finish(self) -> Result<(), String> {
        // SAFETY: the writer is finalised once.
        unsafe { self.writer.Finalize() }.map_err(err("Finalize"))
    }
}

/// One audio track of a recording, read in order as interleaved PCM at [`RATE`] stereo.
pub struct AudioTrackReader {
    reader: IMFSourceReader,
    stream: u32,
    channels: u32,
}

impl AudioTrackReader {
    /// The audio tracks of the MP4 at `path`, in the order of the file's streams (the order of
    /// `Video::audio`).
    pub fn open_all(path: &Path) -> Result<Vec<AudioTrackReader>, String> {
        crate::mf::startup()?;
        let abs = std::path::absolute(path).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        // SAFETY: Media Foundation calls with owned interfaces.
        unsafe {
            let probe = MFCreateSourceReaderFromURL(&HSTRING::from(abs.as_os_str()), None)
                .map_err(err("MFCreateSourceReaderFromURL"))?;
            let mut streams = Vec::new();
            for i in 0..32u32 {
                let Ok(t) = probe.GetNativeMediaType(i, 0) else {
                    break;
                };
                if t.GetMajorType().is_ok_and(|m| m == MFMediaType_Audio) {
                    streams.push(i);
                }
            }
            drop(probe);
            for i in streams {
                let reader = MFCreateSourceReaderFromURL(&HSTRING::from(abs.as_os_str()), None)
                    .map_err(err("MFCreateSourceReaderFromURL"))?;
                reader
                    .SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)
                    .map_err(err("SetStreamSelection"))?;
                reader
                    .SetStreamSelection(i, true)
                    .map_err(err("SetStreamSelection"))?;
                let t = MFCreateMediaType().map_err(err("MFCreateMediaType"))?;
                t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)
                    .map_err(err("pcm"))?;
                t.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)
                    .map_err(err("pcm"))?;
                t.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)
                    .map_err(err("pcm"))?;
                reader
                    .SetCurrentMediaType(i, None, &t)
                    .map_err(err("SetCurrentMediaType (PCM)"))?;
                let cur = reader
                    .GetCurrentMediaType(i)
                    .map_err(err("GetCurrentMediaType"))?;
                let rate = cur.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND).unwrap_or(0);
                let channels = cur.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS).unwrap_or(0);
                if rate != RATE || !(1..=2).contains(&channels) {
                    tracing::warn!("export: audio stream {i} is {rate} Hz × {channels} — left out");
                    continue;
                }
                out.push(AudioTrackReader {
                    reader,
                    stream: i,
                    channels,
                });
            }
        }
        Ok(out)
    }

    /// From the start of the stream again at `time_hns` (the decoder starts a little before).
    pub fn seek(&mut self, time_hns: i64) -> Result<(), String> {
        use windows::Win32::System::Com::StructuredStorage::{
            PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0,
        };
        use windows::Win32::System::Variant::VT_I8;
        let v = PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VT_I8,
                    Anonymous: windows::Win32::System::Com::StructuredStorage::PROPVARIANT_0_0_0 {
                        hVal: time_hns.max(0),
                    },
                    ..Default::default()
                }),
            },
        };
        // SAFETY: a seek of our reader.
        unsafe { self.reader.SetCurrentPosition(&GUID::zeroed(), &v) }
            .map_err(err("SetCurrentPosition"))
    }

    /// The next block: its time and stereo PCM; None at the end.
    pub fn next_block(&mut self) -> Result<Option<(i64, Vec<i16>)>, String> {
        loop {
            let (mut flags, mut ts, mut sample) = (0u32, 0i64, None);
            // SAFETY: a synchronous read of our reader; the buffer is read within its length.
            unsafe {
                self.reader
                    .ReadSample(
                        self.stream,
                        0,
                        None,
                        Some(&mut flags),
                        Some(&mut ts),
                        Some(&mut sample),
                    )
                    .map_err(err("ReadSample (audio)"))?;
                if let Some(s) = sample {
                    let buf = s
                        .ConvertToContiguousBuffer()
                        .map_err(err("ConvertToContiguousBuffer"))?;
                    let (mut p, mut len) = (std::ptr::null_mut(), 0u32);
                    buf.Lock(&mut p, None, Some(&mut len))
                        .map_err(err("Lock"))?;
                    let raw = std::slice::from_raw_parts(p.cast::<i16>(), len as usize / 2);
                    let pcm = if self.channels == 1 {
                        raw.iter().flat_map(|v| [*v, *v]).collect()
                    } else {
                        raw.to_vec()
                    };
                    let _ = buf.Unlock();
                    return Ok(Some((ts, pcm)));
                }
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                return Ok(None);
            }
        }
    }
}
