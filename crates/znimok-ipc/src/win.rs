//! Windows transport: a per-user named pipe.
//!
//! - name `\\.\pipe\znimok-<SID>[-suffix]`, so two users on one machine never meet;
//! - DACL `D:P(A;;GA;;;<SID>)` — only the current user, no inherited entries (other users,
//!   services and Administrators get "access denied" at open);
//! - `PIPE_REJECT_REMOTE_CLIENTS`; the first instance is created with `FILE_FLAG_FIRST_PIPE_INSTANCE`
//!   so another process cannot squat on the name before us;
//! - clients open it with `SECURITY_IDENTIFICATION`: the server may identify, never impersonate them.
//!
//! Synchronous pipe reads have no timeout, so a watchdog cancels the blocked read of the connection
//! thread (`CancelSynchronousIo`) when its deadline passes (hello and idle limits). Disconnecting the
//! pipe from another thread does not work: I/O on a synchronous handle is serialised, so it would
//! wait for the very read it should break.

use std::fs::File;
use std::io::{Read, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::FromRawHandle;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    CloseHandle, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, HANDLE, HLOCAL, INVALID_HANDLE_VALUE,
    LocalFree,
};
use windows::Win32::Foundation::{DUPLICATE_SAME_ACCESS, DuplicateHandle};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
use windows::Win32::System::IO::CancelSynchronousIo;
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::Threading::{GetCurrentProcess, GetCurrentThread, OpenProcessToken};
use windows_core::{HSTRING, PWSTR};

use crate::{Config, Handler, Split, serve};

/// `SECURITY_IDENTIFICATION` for `security_qos_flags` (SecurityIdentification << 16).
const SECURITY_IDENTIFICATION: u32 = 1 << 16;

/// The current user's SID as a string (`S-1-5-21-…`).
pub(crate) fn user_sid() -> std::io::Result<String> {
    // SAFETY: token query on our own process; buffers sized by the first call; strings freed.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).map_err(io)?;
        let mut len = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
        let mut buf = vec![0u8; len as usize];
        let r = GetTokenInformation(
            token,
            TokenUser,
            Some(buf.as_mut_ptr().cast()),
            len,
            &mut len,
        );
        let _ = CloseHandle(token);
        r.map_err(io)?;
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut s = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut s).map_err(io)?;
        let out = s
            .to_string()
            .map_err(|e| std::io::Error::other(e.to_string()));
        let _ = LocalFree(Some(HLOCAL(s.0.cast())));
        out
    }
}

fn io(e: windows_core::Error) -> std::io::Error {
    std::io::Error::from_raw_os_error(e.code().0 & 0xFFFF)
}

pub(crate) fn pipe_name(cfg: &Config) -> std::io::Result<String> {
    Ok(format!(r"\\.\pipe\znimok-{}{}", user_sid()?, cfg.tag()))
}

/// Owns the security descriptor for as long as pipe instances are created with it.
struct Sd(PSECURITY_DESCRIPTOR);
// SAFETY: an immutable LocalAlloc'ed buffer, only read by CreateNamedPipeW.
unsafe impl Send for Sd {}
impl Drop for Sd {
    fn drop(&mut self) {
        // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.0.0)));
        }
    }
}

fn user_only_sd() -> std::io::Result<Sd> {
    let sddl = format!("D:P(A;;GA;;;{})", user_sid()?);
    let mut sd = PSECURITY_DESCRIPTOR::default();
    // SAFETY: valid SDDL string; out-pointer to a local.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            &HSTRING::from(sddl),
            SDDL_REVISION_1,
            &mut sd,
            None,
        )
        .map_err(io)?;
    }
    Ok(Sd(sd))
}

fn create_instance(name: &str, sd: &Sd, first: bool, max: usize) -> std::io::Result<HANDLE> {
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0.0,
        bInheritHandle: false.into(),
    };
    let mut mode = PIPE_ACCESS_DUPLEX;
    if first {
        mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
    }
    // SAFETY: valid name and attributes for the duration of the call.
    let h = unsafe {
        CreateNamedPipeW(
            &HSTRING::from(name),
            mode,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            max.clamp(1, 254) as u32,
            64 * 1024,
            64 * 1024,
            0,
            Some(&sa),
        )
    };
    if h == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    Ok(h)
}

/// Split a connected pipe into halves with a watchdog-based read deadline. Must be called on the
/// thread that will read (the watchdog cancels that thread's blocking read).
fn split(h: HANDLE) -> std::io::Result<(Split, Arc<AtomicBool>)> {
    // SAFETY: we own the handle; File closes it on drop.
    let write = unsafe { File::from_raw_handle(h.0) };
    let read = write.try_clone()?;
    // A real handle of this thread for CancelSynchronousIo (the pseudo-handle means "the caller").
    let mut me = HANDLE::default();
    // SAFETY: duplicating our own thread pseudo-handle into a real one; closed by the watchdog.
    unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            GetCurrentThread(),
            GetCurrentProcess(),
            &mut me,
            0,
            false,
            DUPLICATE_SAME_ACCESS,
        )
        .map_err(io)?;
    }
    let thread = me.0 as isize;
    let deadline: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
    let done = Arc::new(AtomicBool::new(false));
    {
        let (deadline, done) = (deadline.clone(), done.clone());
        std::thread::spawn(move || {
            let thread = HANDLE(thread as *mut _);
            while !done.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(50));
                let expired = deadline
                    .lock()
                    .map(|d| d.is_some_and(|t| Instant::now() >= t))
                    .unwrap_or(false);
                if expired && !done.load(Ordering::SeqCst) {
                    // Keep cancelling until serve() gives up (a read may start just after a cancel).
                    // SAFETY: a valid thread handle owned by this watchdog.
                    unsafe {
                        let _ = CancelSynchronousIo(thread);
                    }
                }
            }
            // SAFETY: our duplicate.
            unsafe {
                let _ = CloseHandle(thread);
            }
        });
    }
    let timeout = Box::new(move |t: Option<Duration>| {
        if let Ok(mut d) = deadline.lock() {
            *d = t.map(|t| Instant::now() + t);
        }
    });
    Ok((
        Split {
            read: Box::new(read),
            write: Box::new(write),
            timeout,
        },
        done,
    ))
}

pub(crate) struct Listener {
    name: String,
    stop: Arc<AtomicBool>,
}

impl Listener {
    pub fn start(
        cfg: Config,
        token: String,
        handler: Arc<dyn Handler>,
        stop: Arc<AtomicBool>,
        active: Arc<AtomicUsize>,
    ) -> std::io::Result<Self> {
        let name = pipe_name(&cfg)?;
        let sd = user_only_sd()?;
        // Created here so "the name is taken" reaches the caller.
        let first = create_instance(&name, &sd, true, cfg.max_clients)?;
        let first_raw = first.0 as isize;
        let (n, s) = (name.clone(), stop.clone());
        std::thread::Builder::new()
            .name("znimok-ipc".into())
            .spawn(move || {
                let token = Arc::new(token);
                let cfg = Arc::new(cfg);
                let mut next = Some(HANDLE(first_raw as *mut _));
                while !s.load(Ordering::SeqCst) {
                    let h = match next.take() {
                        Some(h) => h,
                        None => match create_instance(&n, &sd, false, cfg.max_clients) {
                            Ok(h) => h,
                            Err(_) => {
                                // All instances busy: wait for one to free up.
                                std::thread::sleep(Duration::from_millis(100));
                                continue;
                            }
                        },
                    };
                    // SAFETY: a fresh pipe instance.
                    let connected = match unsafe { ConnectNamedPipe(h, None) } {
                        Ok(()) => true,
                        Err(e) => e.code() == ERROR_PIPE_CONNECTED.to_hresult(),
                    };
                    if s.load(Ordering::SeqCst) || !connected {
                        // SAFETY: our handle.
                        unsafe {
                            let _ = CloseHandle(h);
                        }
                        continue;
                    }
                    let (token, cfg, handler, active) =
                        (token.clone(), cfg.clone(), handler.clone(), active.clone());
                    let raw = h.0 as isize;
                    std::thread::spawn(move || {
                        active.fetch_add(1, Ordering::SeqCst);
                        if let Ok((conn, done)) = split(HANDLE(raw as *mut _)) {
                            serve(conn, &token, &cfg, &*handler);
                            done.store(true, Ordering::SeqCst);
                        }
                        active.fetch_sub(1, Ordering::SeqCst);
                    });
                }
            })?;
        Ok(Self { name, stop })
    }

    pub fn endpoint(&self) -> String {
        self.name.clone()
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the accept loop blocked in ConnectNamedPipe.
        let _ = open(&self.name);
    }
}

fn open(name: &str) -> std::io::Result<File> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .security_qos_flags(SECURITY_IDENTIFICATION)
        .open(name)
}

pub(crate) fn connect(
    cfg: &Config,
) -> std::io::Result<(Box<dyn Read + Send>, Box<dyn Write + Send>)> {
    let name = pipe_name(cfg)?;
    let t0 = Instant::now();
    let file = loop {
        match open(&name) {
            Ok(f) => break f,
            Err(e)
                if e.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32)
                    && t0.elapsed() < Duration::from_secs(3) =>
            {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e),
        }
    };
    let read = file.try_clone()?;
    Ok((Box::new(read), Box::new(file)))
}

/// The DACL of the running server's pipe, as SDDL (for tests and diagnostics).
#[cfg(test)]
pub(crate) fn endpoint_dacl(cfg: &Config) -> std::io::Result<String> {
    let name = pipe_name(cfg)?;
    // SAFETY: out-pointers to locals; the descriptor and the string are freed.
    unsafe {
        let mut sd = PSECURITY_DESCRIPTOR::default();
        let rc = windows::Win32::Security::Authorization::GetNamedSecurityInfoW(
            &HSTRING::from(name),
            windows::Win32::Security::Authorization::SE_FILE_OBJECT,
            windows::Win32::Security::DACL_SECURITY_INFORMATION,
            None,
            None,
            None,
            None,
            &mut sd,
        );
        if rc.0 != 0 {
            return Err(std::io::Error::from_raw_os_error(rc.0 as i32));
        }
        let mut s = PWSTR::null();
        let r = windows::Win32::Security::Authorization::ConvertSecurityDescriptorToStringSecurityDescriptorW(
            sd,
            SDDL_REVISION_1,
            windows::Win32::Security::DACL_SECURITY_INFORMATION,
            &mut s,
            None,
        );
        let _ = LocalFree(Some(HLOCAL(sd.0)));
        r.map_err(io)?;
        let out = s
            .to_string()
            .map_err(|e| std::io::Error::other(e.to_string()));
        let _ = LocalFree(Some(HLOCAL(s.0.cast())));
        out
    }
}
