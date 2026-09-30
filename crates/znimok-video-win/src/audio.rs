//! Sound sources on Windows (ZK-89): WASAPI in shared mode — the system sound as a loopback of
//! the render device, the microphone as a capture device. The OS converts to 48 kHz stereo
//! float (`AUTOCONVERTPCM`), so there is no resampler here; packets carry the QPC stamp WASAPI
//! gives them, which is the recording clock ([`crate::clock::QpcClock`]).
//!
//! A loopback sends nothing while nothing plays — the timeline makes that silence by the
//! stamps (`znimok_video::audio`). A device that goes away (`AUDCLNT_E_DEVICE_INVALIDATED`)
//! reports [`AudioError::DeviceLost`]: the recorder writes silence and reopens it every 500 ms,
//! and for "the default device" that is whatever is the default then.

use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::E_ACCESSDENIED;
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_E_DEVICE_IN_USE, AUDCLNT_E_DEVICE_INVALIDATED,
    AUDCLNT_E_SERVICE_NOT_RUNNING, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_LOOPBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, DEVICE_STATE_ACTIVE,
    EDataFlow, IAudioCaptureClient, IAudioClient, IMMDevice, IMMDeviceEnumerator,
    MMDeviceEnumerator, WAVEFORMATEX, eCapture, eConsole, eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, STGM_READ};
use windows::core::{HSTRING, PWSTR};
use znimok_video::traits::{AudioError, AudioKind, AudioPacket, AudioSource};

/// Not found (`HRESULT_FROM_WIN32(ERROR_NOT_FOUND)`): no such device, or no default one.
const E_NOTFOUND: i32 = 0x8007_0490_u32 as i32;
/// `WAVE_FORMAT_IEEE_FLOAT`.
const FLOAT: u16 = 3;
/// The shared-mode buffer asked for, 100 ns (200 ms: the recorder polls every 10–20 ms).
const BUFFER_HNS: i64 = 2_000_000;

/// An endpoint the person can choose (Settings → Recording, ZK-189).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub default: bool,
}

fn flow(kind: AudioKind) -> EDataFlow {
    match kind {
        AudioKind::System => eRender,
        AudioKind::Microphone => eCapture,
    }
}

fn map(e: windows::core::Error) -> AudioError {
    let c = e.code();
    if c == AUDCLNT_E_DEVICE_INVALIDATED || c.0 == E_NOTFOUND {
        AudioError::DeviceLost
    } else if c == E_ACCESSDENIED {
        AudioError::PermissionDenied
    } else if c == AUDCLNT_E_DEVICE_IN_USE {
        AudioError::DeviceInUse
    } else if c == AUDCLNT_E_SERVICE_NOT_RUNNING {
        AudioError::Other("служба звуку Windows не працює".into())
    } else {
        AudioError::Other(e.message())
    }
}

fn enumerator() -> windows::core::Result<IMMDeviceEnumerator> {
    znimok_win::raw::com_thread();
    // SAFETY: a plain COM creation on an apartment thread.
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
}

fn friendly_name(d: &IMMDevice) -> String {
    // SAFETY: the property store of a live device, read only; the PROPVARIANT is dropped here.
    unsafe {
        d.OpenPropertyStore(STGM_READ)
            .and_then(|s| s.GetValue(&PKEY_Device_FriendlyName))
            .map(|v| v.to_string())
            .unwrap_or_default()
    }
}

fn device_id(d: &IMMDevice) -> String {
    // SAFETY: GetId allocates a string with CoTaskMemAlloc; `to_string` copies it and the
    // allocation is freed right after.
    unsafe {
        d.GetId()
            .map(|p: PWSTR| {
                let s = p.to_string().unwrap_or_default();
                windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
                s
            })
            .unwrap_or_default()
    }
}

/// The active endpoints of a kind, the default one first. Runs on a thread of its own: the
/// caller may be a UI thread whose apartment winit makes single-threaded for drag and drop
/// (`OleInitialize`) — joining the MTA there first makes that fail.
pub fn devices(kind: AudioKind) -> Vec<AudioDevice> {
    std::thread::spawn(move || devices_here(kind))
        .join()
        .unwrap_or_default()
}

fn devices_here(kind: AudioKind) -> Vec<AudioDevice> {
    let Ok(en) = enumerator() else {
        return Vec::new();
    };
    // SAFETY: plain enumerator calls; the collection is read while it lives.
    unsafe {
        let default = en
            .GetDefaultAudioEndpoint(flow(kind), eConsole)
            .map(|d| device_id(&d))
            .unwrap_or_default();
        let Ok(list) = en.EnumAudioEndpoints(flow(kind), DEVICE_STATE_ACTIVE) else {
            return Vec::new();
        };
        let n = list.GetCount().unwrap_or(0);
        let mut out: Vec<AudioDevice> = (0..n)
            .filter_map(|i| list.Item(i).ok())
            .map(|d| {
                let id = device_id(&d);
                AudioDevice {
                    default: id == default,
                    name: friendly_name(&d),
                    id,
                }
            })
            .collect();
        out.sort_by_key(|d| !d.default);
        out
    }
}

struct Open {
    client: IAudioClient,
    capture: IAudioCaptureClient,
}

// SAFETY: WASAPI objects are free-threaded in the multithreaded apartment, which this process
// pins (`com_thread`); the recorder opens a source on one thread and reads it on its audio
// thread, never both at once.
unsafe impl Send for Open {}

/// System sound (loopback) or a microphone; `device` = an id from [`devices`], `None` = the
/// default device at the moment of (re)opening.
pub struct WasapiSource {
    kind: AudioKind,
    device: Option<String>,
    label: String,
    open: Option<Open>,
}

impl WasapiSource {
    pub fn new(kind: AudioKind, device: Option<String>) -> Self {
        Self {
            kind,
            device,
            label: String::new(),
            open: None,
        }
    }

    pub fn system(device: Option<String>) -> Self {
        Self::new(AudioKind::System, device)
    }

    pub fn microphone(device: Option<String>) -> Self {
        Self::new(AudioKind::Microphone, device)
    }
}

impl AudioSource for WasapiSource {
    fn kind(&self) -> AudioKind {
        self.kind
    }

    fn label(&self) -> String {
        self.label.clone()
    }

    fn open(&mut self) -> Result<(), AudioError> {
        self.close();
        let en = enumerator().map_err(map)?;
        // SAFETY: COM calls on objects made here; the format lives across Initialize.
        let open = unsafe {
            let device = match &self.device {
                Some(id) => en.GetDevice(&HSTRING::from(id.as_str())),
                None => en.GetDefaultAudioEndpoint(flow(self.kind), eConsole),
            }
            .map_err(map)?;
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None).map_err(map)?;
            let format = WAVEFORMATEX {
                wFormatTag: FLOAT,
                nChannels: 2,
                nSamplesPerSec: 48_000,
                nAvgBytesPerSec: 48_000 * 8,
                nBlockAlign: 8,
                wBitsPerSample: 32,
                cbSize: 0,
            };
            let mut flags =
                AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
            if self.kind == AudioKind::System {
                flags |= AUDCLNT_STREAMFLAGS_LOOPBACK;
            }
            client
                .Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    flags,
                    BUFFER_HNS,
                    0,
                    &format,
                    None,
                )
                .map_err(map)?;
            let capture: IAudioCaptureClient = client.GetService().map_err(map)?;
            client.Start().map_err(map)?;
            self.label = friendly_name(&device);
            Open { client, capture }
        };
        self.open = Some(open);
        Ok(())
    }

    fn read(&mut self, out: &mut Vec<AudioPacket>) -> Result<(), AudioError> {
        let Some(o) = &self.open else {
            return Err(AudioError::DeviceLost);
        };
        // SAFETY: GetBuffer hands out `frames` interleaved stereo floats that stay valid until
        // ReleaseBuffer, which follows the copy.
        unsafe {
            loop {
                let next = o.capture.GetNextPacketSize().map_err(map)?;
                if next == 0 {
                    return Ok(());
                }
                let mut data = std::ptr::null_mut();
                let (mut frames, mut flags, mut qpc) = (0u32, 0u32, 0u64);
                o.capture
                    .GetBuffer(&mut data, &mut frames, &mut flags, None, Some(&mut qpc))
                    .map_err(map)?;
                let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
                let n = frames as usize * 2;
                let samples = if silent || data.is_null() {
                    vec![0.0; n]
                } else {
                    std::slice::from_raw_parts(data as *const f32, n).to_vec()
                };
                o.capture.ReleaseBuffer(frames).map_err(map)?;
                if frames > 0 {
                    out.push(AudioPacket {
                        time_hns: qpc as i64,
                        data: samples,
                        silent,
                    });
                }
            }
        }
    }

    fn close(&mut self) {
        if let Some(o) = self.open.take() {
            // SAFETY: stopping a started client of this source.
            let _ = unsafe { o.client.Stop() };
        }
    }
}

impl Drop for WasapiSource {
    fn drop(&mut self) {
        self.close();
    }
}
