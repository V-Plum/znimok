//! Media Foundation odds and ends shared by the sink and the decoder: start-up on the calling
//! thread, media-type helpers, the encoder parameters LH set, error text.

use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::core::GUID;

/// `CODECAPI_AVEncMPVGOPSize` / `CODECAPI_AVEncMPVDefaultBPictureCount` — codecapi.h is not in
/// every header set, so the GUIDs are spelled out (as LH and P4 do).
pub const GOP_SIZE: GUID = GUID::from_u128(0x95f31b26_95a4_41aa_9303_246a7fc6eef1);
pub const B_COUNT: GUID = GUID::from_u128(0x8d390aac_dc5c_4200_b57f_814d04bab2b2);

/// COM (MTA) and Media Foundation on this thread. Safe to call more than once; the recording
/// thread and the tests call it first thing.
pub fn startup() -> Result<(), String> {
    // SAFETY: plain initialisation calls; S_FALSE / RPC_E_CHANGED_MODE only mean the thread was
    // already initialised (possibly as STA), which MF accepts.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        MFStartup(MF_VERSION, MFSTARTUP_FULL).map_err(|e| format!("MFStartup: {e}"))
    }
}

pub fn err(what: &str) -> impl Fn(windows_core::Error) -> String + '_ {
    move |e| format!("{what}: {} (0x{:08X})", e.message(), e.code().0)
}

pub fn get_size(t: &IMFMediaType, key: &GUID) -> Option<(u32, u32)> {
    // SAFETY: attribute read on a live media type.
    unsafe { t.GetUINT64(key).ok().map(|v| ((v >> 32) as u32, v as u32)) }
}

pub fn set_size(t: &IMFMediaType, key: &GUID, a: u32, b: u32) -> windows_core::Result<()> {
    // SAFETY: attribute write on a live media type.
    unsafe { t.SetUINT64(key, (u64::from(a) << 32) | u64::from(b)) }
}

/// Friendly name of a transform and whether it is a hardware one.
pub fn transform_name(t: &IMFTransform) -> (String, bool) {
    // SAFETY: attribute reads on a live transform; the string buffer is sized from GetStringLength.
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
