//! Leaving when Windows asks (ZK-170). An installer's Restart Manager — an MSI started by hand
//! while Znimok runs — or a log-off asks the process's top-level windows with
//! `WM_QUERYENDSESSION` / `WM_ENDSESSION`. Closing Znimok's own windows only hides them to the
//! tray, so without an answer the installer reported that it could not close Znimok. A hidden
//! top-level window here says yes and runs the app's own way out; the app is also registered for
//! a restart, so Windows starts it again (to the tray) once the installer is done.

use std::cell::RefCell;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Recovery::{
    RESTART_NO_CRASH, RESTART_NO_HANG, RESTART_NO_REBOOT, RegisterApplicationRestart,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, RegisterClassW, WINDOW_EX_STYLE, WM_ENDSESSION,
    WM_QUERYENDSESSION, WNDCLASSW, WS_EX_TOOLWINDOW, WS_OVERLAPPED,
};
use windows::core::{HSTRING, PCWSTR, w};

thread_local! {
    /// What to do when Windows ends the session for this process (the UI thread's).
    static ON_END: RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
}

/// Calls `leave` on this (the UI) thread when Windows asks the process to end — an installer
/// updating Znimok, a log-off — and registers the app for a restart with `restart_args` after
/// an installer is done. Returns the hidden window (for tests).
pub fn on_session_end(
    leave: impl Fn() + 'static,
    restart_args: &str,
) -> windows::core::Result<HWND> {
    ON_END.with(|f| *f.borrow_mut() = Some(Box::new(leave)));
    // SAFETY: a window class and a hidden top-level window of this process; the procedure below
    // only reads thread-local state of this thread.
    let hwnd = unsafe {
        let module = GetModuleHandleW(None)?;
        let class = w!("ZnimokSession");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc_),
            hInstance: module.into(),
            lpszClassName: class,
            ..Default::default()
        };
        // A second call finds the class registered already: that is fine.
        RegisterClassW(&wc);
        CreateWindowExW(
            WINDOW_EX_STYLE(WS_EX_TOOLWINDOW.0),
            class,
            w!("Znimok"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(module.into()),
            None,
        )?
    };
    let args = HSTRING::from(restart_args);
    // SAFETY: plain registration; a restart only after an installer or an update, never after a
    // crash, a hang or a reboot.
    let _ = unsafe {
        RegisterApplicationRestart(
            PCWSTR(args.as_ptr()),
            RESTART_NO_CRASH | RESTART_NO_HANG | RESTART_NO_REBOOT,
        )
    };
    Ok(hwnd)
}

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        // Yes, Znimok may be closed: everything is saved as it goes.
        WM_QUERYENDSESSION => LRESULT(1),
        WM_ENDSESSION => {
            if wp.0 != 0 {
                ON_END.with(|f| {
                    if let Some(f) = f.borrow().as_ref() {
                        f();
                    }
                });
            }
            LRESULT(0)
        }
        // SAFETY: the default handling of every other message.
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;
    use windows::Win32::UI::WindowsAndMessaging::{DestroyWindow, SendMessageW};

    #[test]
    fn an_installer_asking_to_end_the_session_makes_the_app_leave() {
        let left = Rc::new(Cell::new(0));
        let l = left.clone();
        let hwnd = on_session_end(move || l.set(l.get() + 1), "--background").unwrap();
        // SAFETY: messages to the window made above, on this thread.
        unsafe {
            let ok = SendMessageW(hwnd, WM_QUERYENDSESSION, Some(WPARAM(0)), Some(LPARAM(1)));
            assert_eq!(ok.0, 1, "Znimok agrees to be closed");
            // The session does not end after all: nothing happens.
            SendMessageW(hwnd, WM_ENDSESSION, Some(WPARAM(0)), Some(LPARAM(1)));
            assert_eq!(left.get(), 0);
            SendMessageW(hwnd, WM_ENDSESSION, Some(WPARAM(1)), Some(LPARAM(1)));
            assert_eq!(left.get(), 1, "the app's way out ran once");
            let _ = DestroyWindow(hwnd);
        }
    }
}
