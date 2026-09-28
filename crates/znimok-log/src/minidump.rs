//! Windows: a native crash (access violation, stack overflow…) never reaches the panic hook.
//! An unhandled-exception filter writes a small minidump (threads, stacks, module list — no heap,
//! so no screen pixels) and a text report, then lets Windows continue its normal crash handling.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use windows::Win32::Foundation::{CloseHandle, GENERIC_WRITE, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CREATE_ALWAYS, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_NONE,
};
use windows::Win32::System::Diagnostics::Debug::{
    EXCEPTION_CONTINUE_SEARCH, EXCEPTION_POINTERS, MINIDUMP_EXCEPTION_INFORMATION, MiniDumpNormal,
    MiniDumpWithThreadInfo, MiniDumpWriteDump, SetUnhandledExceptionFilter,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId,
};
use windows::core::HSTRING;

static DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn install(dir: &Path) {
    if DIR.set(dir.to_path_buf()).is_ok() {
        // SAFETY: registering a process-wide callback with the documented signature.
        unsafe {
            SetUnhandledExceptionFilter(Some(filter));
        }
    }
}

unsafe extern "system" fn filter(info: *const EXCEPTION_POINTERS) -> i32 {
    let Some(dir) = DIR.get() else {
        return EXCEPTION_CONTINUE_SEARCH;
    };
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S-%3f").to_string();
    let dmp = dir.join(format!("crash-{stamp}.dmp"));
    // SAFETY: the pointers come from the OS for the duration of this call.
    let code = unsafe {
        info.as_ref()
            .and_then(|i| i.ExceptionRecord.as_ref())
            .map(|r| r.ExceptionCode.0 as u32)
    };
    let written = unsafe { write_dump(&dmp, info) };
    let body = format!(
        "exception: 0x{:08X}{}\nminidump: {}",
        code.unwrap_or(0),
        match code {
            Some(0xC000_0005) => " (ACCESS_VIOLATION)",
            Some(0xC000_00FD) => " (STACK_OVERFLOW)",
            Some(0xC000_0409) => " (STACK_BUFFER_OVERRUN)",
            _ => "",
        },
        if written {
            dmp.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        } else {
            "не записано".into()
        }
    );
    // The .txt must share the .dmp's stem so they are pruned and offered together.
    if let Ok(p) = crate::write_report("native", &body) {
        let _ = std::fs::rename(&p, dmp.with_extension("txt"));
    }
    EXCEPTION_CONTINUE_SEARCH
}

unsafe fn write_dump(path: &Path, info: *const EXCEPTION_POINTERS) -> bool {
    unsafe {
        let Ok(file) = CreateFileW(
            &HSTRING::from(path.as_os_str()),
            GENERIC_WRITE.0,
            FILE_SHARE_NONE,
            None,
            CREATE_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            None,
        ) else {
            return false;
        };
        let exc = MINIDUMP_EXCEPTION_INFORMATION {
            ThreadId: GetCurrentThreadId(),
            ExceptionPointers: info as *mut _,
            ClientPointers: false.into(),
        };
        let ok = MiniDumpWriteDump(
            GetCurrentProcess(),
            GetCurrentProcessId(),
            file,
            MiniDumpNormal | MiniDumpWithThreadInfo,
            Some(&exc),
            None,
            None,
        )
        .is_ok();
        let _ = CloseHandle(HANDLE(file.0));
        ok
    }
}
