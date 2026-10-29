//! Console handling. On Windows Phosphor is built as a GUI program, so
//! starting it from Explorer opens no console window; when it is started
//! from a command prompt it attaches to that console so `--help`,
//! `--import` and error messages still appear there.

/// Attach to the console of the parent process, if there is one. Returns
/// whether output will be visible on a console.
#[cfg(windows)]
pub fn attach() -> bool {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: AttachConsole has no preconditions; without a parent console
    // it simply fails.
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) != 0 }
}

#[cfg(not(windows))]
pub fn attach() -> bool {
    true
}

/// Report a fatal error on stderr, and in a message box when there is no
/// console to show it (Windows, started from Explorer).
pub fn report_error(msg: &str, has_console: bool) {
    eprintln!("phosphor: {msg}");
    #[cfg(windows)]
    if !has_console {
        message_box(msg);
    }
    #[cfg(not(windows))]
    let _ = has_console;
}

#[cfg(windows)]
fn message_box(msg: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (text, caption) = (wide(msg), wide("Phosphor"));
    // SAFETY: both strings are NUL-terminated UTF-16 that outlive the call;
    // a null owner window is allowed.
    unsafe {
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), MB_OK | MB_ICONERROR);
    }
}
