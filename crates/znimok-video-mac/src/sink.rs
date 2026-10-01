//! The recording's sink: frames from ScreenCaptureKit and the mixed-down sound into the
//! [`AvWriter`]. AVAssetWriter places samples by time and ignores durations, so the recorder
//! repeats a stretched frame (`carries_duration = false`); a busy real-time input is the loop's
//! back-pressure ([`SinkError::Busy`]).

use std::path::Path;

use znimok_video::audio::AudioSampleTime;
use znimok_video::cfr::VideoSampleTime;
use znimok_video::traits::{EncoderConfig, SinkCaps, SinkError, VideoSink};

use crate::writer::{AvWriter, PixelBuf, WriterConfig};

pub struct AvSink {
    w: AvWriter,
}

impl AvSink {
    pub fn open(part: &Path, cfg: &EncoderConfig) -> Result<Self, String> {
        let w = AvWriter::create(
            part,
            &WriterConfig {
                width: cfg.width,
                height: cfg.height,
                fps: cfg.fps,
                bitrate: cfg.bitrate,
                keyframe_interval: cfg.keyframe_interval,
                audio_tracks: cfg.audio_tracks,
                audio_bitrate: cfg.audio_bitrate,
                real_time: true,
            },
        )?;
        Ok(Self { w })
    }
}

impl VideoSink for AvSink {
    type Frame = PixelBuf;

    fn caps(&self) -> SinkCaps {
        SinkCaps {
            carries_duration: false,
        }
    }

    fn write_video(&mut self, frame: &PixelBuf, t: VideoSampleTime) -> Result<(), SinkError> {
        match self.w.append_frame(&frame.0, t.slot) {
            Ok(true) => Ok(()),
            Ok(false) => Err(SinkError::Busy),
            Err(e) => Err(SinkError::Failed(e)),
        }
    }

    fn write_audio(
        &mut self,
        track: usize,
        pcm: &[i16],
        t: AudioSampleTime,
    ) -> Result<(), SinkError> {
        // The loop ignores sound errors (LH: the video goes on), so a busy input is waited for
        // briefly here rather than losing the piece.
        for _ in 0..25 {
            match self.w.append_pcm(track, pcm, t.index) {
                Ok(true) => return Ok(()),
                Ok(false) => std::thread::sleep(std::time::Duration::from_millis(2)),
                Err(e) => return Err(SinkError::Failed(e)),
            }
        }
        Err(SinkError::Busy)
    }

    fn finalize(&mut self) -> Result<(), SinkError> {
        self.w.finish().map_err(SinkError::Failed)
    }
}
