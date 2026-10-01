//! System sound and the microphone (ZK-89 on macOS): both come in the recording's own `SCStream`
//! (`capturesAudio`, `captureMicrophone` — macOS 15+), stamped with the host clock; each source
//! here only hands over what the stream left for it.

use std::sync::Arc;

use znimok_video::traits::{AudioError, AudioKind, AudioPacket, AudioSource};

use crate::source::Shared;

pub struct ScAudio {
    kind: AudioKind,
    shared: Option<Arc<Shared>>,
    label: String,
}

impl ScAudio {
    pub fn system() -> Self {
        Self {
            kind: AudioKind::System,
            shared: None,
            label: String::new(),
        }
    }

    pub fn microphone() -> Self {
        Self {
            kind: AudioKind::Microphone,
            shared: None,
            label: String::new(),
        }
    }

    /// Joined to the stream that carries its sound (the recording does it before opening).
    pub fn attach(&mut self, shared: Arc<Shared>, label: String) {
        self.shared = Some(shared);
        self.label = label;
    }
}

impl AudioSource for ScAudio {
    fn kind(&self) -> AudioKind {
        self.kind
    }

    fn label(&self) -> String {
        self.label.clone()
    }

    fn open(&mut self) -> Result<(), AudioError> {
        if self.shared.is_some() {
            Ok(())
        } else {
            Err(AudioError::Other("звук не підключено до запису".into()))
        }
    }

    fn read(&mut self, out: &mut Vec<AudioPacket>) -> Result<(), AudioError> {
        let Some(s) = self.shared.as_ref() else {
            return Err(AudioError::DeviceLost);
        };
        let q = match self.kind {
            AudioKind::System => &s.system,
            AudioKind::Microphone => &s.microphone,
        };
        if let Ok(mut q) = q.lock() {
            out.append(&mut q);
        }
        Ok(())
    }

    fn close(&mut self) {}
}
