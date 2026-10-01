//! Frames and sound from one `SCStream`: ScreenCaptureKit calls back on its own queue; the latest
//! frame waits here for the recording loop, sound packets wait for [`crate::audio::ScAudio`].

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use screencapturekit::cm::CMSampleBuffer;
use screencapturekit::stream::output_type::SCStreamOutputType;
use znimok_video::traits::{AudioPacket, FrameSource, Pulled};
use znimok_video::{Result, VideoError};

use crate::clock::host_to_hns;
use crate::writer::PixelBuf;

/// What the stream's callbacks hand to the recording.
#[derive(Default)]
pub struct Shared {
    latest: Mutex<Option<Arc<PixelBuf>>>,
    arrived: Condvar,
    frames: AtomicU64,
    /// The stream stopped by itself (the window closed, the display went).
    pub stopped: AtomicBool,
    /// Packets of the system sound and of the microphone, until their source reads them.
    pub system: Mutex<Vec<AudioPacket>>,
    pub microphone: Mutex<Vec<AudioPacket>>,
}

impl Shared {
    /// A sample from ScreenCaptureKit (any output type), on its queue.
    pub fn on_sample(&self, sample: &CMSampleBuffer, kind: SCStreamOutputType) {
        match kind {
            SCStreamOutputType::Screen => {
                // Idle and blank frames carry no picture: the latest one stays.
                let ptr = sample.image_buffer_ptr_borrowed();
                // SAFETY: a CVPixelBufferRef borrowed from the live sample; retained here.
                if let Some(pb) = unsafe { PixelBuf::retain_raw(ptr) } {
                    if let Ok(mut l) = self.latest.lock() {
                        *l = Some(Arc::new(pb));
                    }
                    self.frames.fetch_add(1, Ordering::Release);
                    self.arrived.notify_all();
                }
            }
            SCStreamOutputType::Audio => {
                if let Some(p) = packet(sample)
                    && let Ok(mut q) = self.system.lock()
                {
                    q.push(p);
                }
            }
            SCStreamOutputType::Microphone => {
                if let Some(p) = packet(sample)
                    && let Ok(mut q) = self.microphone.lock()
                {
                    q.push(p);
                }
            }
        }
    }
}

/// A sound sample as the recorder's packet: interleaved stereo float at 48 kHz, stamped with the
/// host clock (the recording clock, [`crate::MachClock`]).
fn packet(sample: &CMSampleBuffer) -> Option<AudioPacket> {
    let frames = usize::try_from(sample.num_samples())
        .ok()
        .filter(|n| *n > 0)?;
    let t = sample.presentation_timestamp();
    let rate = sample
        .format_description()
        .and_then(|f| f.audio_sample_rate())
        .unwrap_or(48_000.0);
    let list = sample.audio_buffer_list().ok()?;
    let as_f32 = |b: &[u8]| -> Vec<f32> {
        b.as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_ne_bytes(*c))
            .collect()
    };
    let mut data = Vec::with_capacity(frames * 2);
    match list.num_buffers() {
        0 => return None,
        // One buffer: interleaved, one or two channels.
        1 => {
            let b = list.get(0)?;
            let s = as_f32(b.data());
            if b.number_channels >= 2 {
                let ch = b.number_channels as usize;
                for f in s.chunks_exact(ch) {
                    data.extend_from_slice(&[f[0], f[1]]);
                }
            } else {
                for v in s {
                    data.extend_from_slice(&[v, v]);
                }
            }
        }
        // Planar: the first two channels.
        _ => {
            let l = as_f32(list.get(0)?.data());
            let r = as_f32(list.get(1)?.data());
            for (a, b) in l.iter().zip(r.iter()) {
                data.extend_from_slice(&[*a, *b]);
            }
        }
    }
    // The recorder takes 48 kHz only (there is no resampler in the core): another rate — the
    // microphone's own, when the system does not convert — is resampled linearly here.
    if (rate - 48_000.0).abs() > 0.5 && rate > 0.0 {
        data = resample(&data, rate, 48_000.0);
    }
    Some(AudioPacket {
        time_hns: host_to_hns(t.value, t.timescale),
        silent: data.iter().all(|v| *v == 0.0),
        data,
    })
}

fn resample(stereo: &[f32], from: f64, to: f64) -> Vec<f32> {
    let n = stereo.len() / 2;
    if n == 0 {
        return Vec::new();
    }
    let out_n = ((n as f64) * to / from).round().max(1.0) as usize;
    let mut out = Vec::with_capacity(out_n * 2);
    for i in 0..out_n {
        let x = i as f64 * from / to;
        let a = (x.floor() as usize).min(n - 1);
        let b = (a + 1).min(n - 1);
        let k = (x - a as f64) as f32;
        for c in 0..2 {
            out.push(stereo[a * 2 + c] * (1.0 - k) + stereo[b * 2 + c] * k);
        }
    }
    out
}

/// The frame source of the recording loop.
pub struct ScSource {
    shared: Arc<Shared>,
    current: Option<Arc<PixelBuf>>,
    seen: u64,
}

impl ScSource {
    pub fn new(shared: Arc<Shared>) -> Self {
        Self {
            shared,
            current: None,
            seen: 0,
        }
    }
}

impl FrameSource for ScSource {
    type Frame = PixelBuf;

    fn pull(&mut self, wait: Duration) -> Result<Pulled> {
        if self.shared.stopped.load(Ordering::Acquire) {
            return Ok(Pulled::Closed);
        }
        let mut latest = self
            .shared
            .latest
            .lock()
            .map_err(|_| VideoError::Screen("кадр недоступний".into()))?;
        if self.shared.frames.load(Ordering::Acquire) == self.seen {
            latest = self
                .shared
                .arrived
                .wait_timeout(latest, wait)
                .map_err(|_| VideoError::Screen("кадр недоступний".into()))?
                .0;
        }
        let n = self.shared.frames.load(Ordering::Acquire);
        if n == self.seen {
            return Ok(Pulled::Unchanged);
        }
        self.seen = n;
        self.current = latest.clone();
        Ok(Pulled::Frame)
    }

    fn has_frame(&self) -> bool {
        self.current.is_some()
    }

    fn frame_for_slot(&mut self, _slot: i64) -> Result<&PixelBuf> {
        self.current
            .as_deref()
            .ok_or_else(|| VideoError::Screen("ще немає кадру".into()))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn resampling_keeps_the_length_and_the_level() {
        let one_khz: Vec<f32> = (0..441).flat_map(|_| [0.5f32, -0.5]).collect();
        let out = super::resample(&one_khz, 44_100.0, 48_000.0);
        assert_eq!(out.len(), 480 * 2);
        assert!(out.iter().step_by(2).all(|v| (*v - 0.5).abs() < 1e-6));
    }
}
