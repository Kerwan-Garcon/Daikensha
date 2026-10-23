// The Windows calls of the Clipboard, Wallpapers and Power tabs
// (docs/WINDOWS_SHELL.md), all documented Win32 APIs. The clipboard listener
// is a message-only window that sleeps in GetMessage until Windows posts
// WM_CLIPBOARDUPDATE; the rest runs only when the panel asks.

use std::path::Path;
use std::sync::OnceLock;

use parking_lot::Mutex;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, GetClipboardData, GetClipboardOwner, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, RemoveClipboardFormatListener,
};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, GetWindowThreadProcessId, PostMessageW,
    PostQuitMessage, RegisterClassW, HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLIPBOARDUPDATE, WM_CLOSE, WM_DESTROY,
    WNDCLASSW,
};

use super::clipboard;
use super::power::{Battery, PowerAction};

fn err(e: windows::core::Error) -> String {
    e.message()
}

// ── Clipboard ─────────────────────────────────────────────────────────────────

/// The listener's window, while it runs (as an integer: HWND is not Send).
static LISTENER: Mutex<Option<isize>> = Mutex::new(None);
/// Told after each kept copy (the island refreshes the tab if it shows).
static NOTIFY: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Starts or stops the listener. `notify` runs after each kept copy.
pub fn clipboard_listen(on: bool, notify: impl Fn() + Send + Sync + 'static) {
    let _ = NOTIFY.set(Box::new(notify));
    let mut current = LISTENER.lock();
    match (on, *current) {
        (true, None) => {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || run_listener(tx));
            if let Ok(Some(hwnd)) = rx.recv() {
                *current = Some(hwnd);
            }
        }
        (false, Some(hwnd)) => {
            *current = None;
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        _ => {}
    }
}

fn run_listener(ready: std::sync::mpsc::Sender<Option<isize>>) {
    unsafe {
        let class = w!("CoucouClipboardListener");
        let wc = WNDCLASSW { lpfnWndProc: Some(listener_proc), lpszClassName: class, ..Default::default() };
        // A second start registers the class again and fails harmlessly.
        RegisterClassW(&wc);
        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!(""),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            None,
            None,
        ) {
            Ok(h) => h,
            Err(_) => {
                let _ = ready.send(None);
                return;
            }
        };
        if AddClipboardFormatListener(hwnd).is_err() {
            let _ = DestroyWindow(hwnd);
            let _ = ready.send(None);
            return;
        }
        let _ = ready.send(Some(hwnd.0 as isize));
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn listener_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CLIPBOARDUPDATE => {
            if let Some((text, app)) = read_copy(hwnd) {
                clipboard::history().push(&text, &app, clipboard::now_ms());
                if let Some(notify) = NOTIFY.get() {
                    notify();
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = RemoveClipboardFormatListener(hwnd);
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// The executable's file name, lower case ("keepassxc.exe"), and its display name.
fn owner_app(pid: u32) -> (String, String) {
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return (String::new(), String::new()) };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if !ok {
            return (String::new(), String::new());
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        let file = full.rsplit(['\\', '/']).next().unwrap_or("").to_lowercase();
        (file, super::stats::display_name(&full))
    }
}

/// Runs `f` on a global memory block (its address and size), under its lock.
unsafe fn with_global<R>(handle: HANDLE, f: impl FnOnce(*const u8, usize) -> R) -> Option<R> {
    let global = HGLOBAL(handle.0);
    let ptr = GlobalLock(global) as *const u8;
    if ptr.is_null() {
        return None;
    }
    let out = f(ptr, GlobalSize(global));
    let _ = GlobalUnlock(global);
    Some(out)
}

/// The text just copied and the app it came from, unless it must not be kept.
fn read_copy(hwnd: HWND) -> Option<(String, String)> {
    unsafe {
        let owner = GetClipboardOwner().ok().filter(|h| !h.is_invalid());
        let (file, app) = match owner {
            Some(o) => {
                let mut pid = 0u32;
                GetWindowThreadProcessId(o, Some(&mut pid));
                if pid == std::process::id() {
                    return None;
                }
                owner_app(pid)
            }
            None => (String::new(), String::new()),
        };
        if clipboard::excluded(&file) {
            return None;
        }
        let exclude = RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
        let history = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
        let viewer_ignore = RegisterClipboardFormatW(w!("Clipboard Viewer Ignore"));
        // Another listener may hold the clipboard for a moment.
        let mut opened = false;
        for _ in 0..5 {
            if OpenClipboard(Some(hwnd)).is_ok() {
                opened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if !opened {
            return None;
        }
        let text = (|| {
            if IsClipboardFormatAvailable(exclude).is_ok() || IsClipboardFormatAvailable(viewer_ignore).is_ok() {
                return None;
            }
            if IsClipboardFormatAvailable(history).is_ok() {
                let h = GetClipboardData(history).ok()?;
                let allowed = with_global(h, |p, size| size >= 4 && std::ptr::read_unaligned(p as *const u32) != 0)?;
                if !allowed {
                    return None;
                }
            }
            let h = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
            with_global(h, |p, size| {
                let units = (size / 2).min(clipboard::MAX_CHARS * 2);
                let slice = std::slice::from_raw_parts(p as *const u16, units);
                let end = slice.iter().position(|&c| c == 0).unwrap_or(units);
                String::from_utf16_lossy(&slice[..end])
            })
        })();
        let _ = CloseClipboard();
        text.map(|t| (t, app))
    }
}

// ── Wallpapers ────────────────────────────────────────────────────────────────

/// Runs `f` on a thread of its own with COM initialised (single-threaded apartment).
fn with_com<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE};
    std::thread::spawn(move || unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let out = f();
        if init.is_ok() {
            CoUninitialize();
        }
        out
    })
    .join()
    .map_err(|_| "COM thread failed".to_string())
}

/// The Shell's thumbnail of `path`, fitted in `w`×`h`, as BGRA rows top-down.
pub fn thumbnail(path: &Path, w: u32, h: u32) -> Option<(u32, u32, Vec<u8>)> {
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
        HGDIOBJ,
    };
    use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_RESIZETOFIT};
    let path = HSTRING::from(path.as_os_str());
    with_com(move || unsafe {
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None).ok()?;
        let bitmap = factory.GetImage(SIZE { cx: w as i32, cy: h as i32 }, SIIGBF_RESIZETOFIT).ok()?;
        let mut info = BITMAP::default();
        let got = GetObjectW(HGDIOBJ(bitmap.0), std::mem::size_of::<BITMAP>() as i32, Some(&mut info as *mut _ as *mut _));
        let (bw, bh) = (info.bmWidth.max(0) as u32, info.bmHeight.unsigned_abs());
        let mut pixels = vec![0u8; (bw * bh * 4) as usize];
        let mut header = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: bw as i32,
                biHeight: -(bh as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let dc = CreateCompatibleDC(None);
        let lines = if got > 0 && bw > 0 && bh > 0 && bw <= 1024 && bh <= 1024 {
            GetDIBits(dc, bitmap, 0, bh, Some(pixels.as_mut_ptr() as *mut _), &mut header, DIB_RGB_COLORS)
        } else {
            0
        };
        let _ = DeleteDC(dc);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        (lines > 0).then_some((bw, bh, pixels))
    })
    .ok()
    .flatten()
}

fn desktop_wallpaper() -> windows::core::Result<windows::Win32::UI::Shell::IDesktopWallpaper> {
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
    use windows::Win32::UI::Shell::DesktopWallpaper;
    unsafe { CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL) }
}

pub fn wallpaper_screens() -> usize {
    with_com(|| unsafe { desktop_wallpaper().and_then(|d| d.GetMonitorDevicePathCount()).unwrap_or(0) as usize }).unwrap_or(0)
}

pub fn set_wallpaper(path: &Path, screen: Option<usize>) -> Result<(), String> {
    let path = HSTRING::from(path.as_os_str());
    with_com(move || -> Result<(), String> {
        unsafe {
            let d = desktop_wallpaper().map_err(err)?;
            match screen {
                None => d.SetWallpaper(PCWSTR::null(), PCWSTR(path.as_ptr())).map_err(err),
                Some(i) => {
                    let count = d.GetMonitorDevicePathCount().map_err(err)? as usize;
                    if i >= count {
                        return Err(crate::i18n::t("That screen is gone."));
                    }
                    let id = d.GetMonitorDevicePathAt(i as u32).map_err(err)?;
                    let out = d.SetWallpaper(PCWSTR(id.0), PCWSTR(path.as_ptr())).map_err(err);
                    windows::Win32::System::Com::CoTaskMemFree(Some(id.0 as *const _));
                    out
                }
            }
        }
    })?
}

// ── Power ─────────────────────────────────────────────────────────────────────

/// Lets this process shut the computer down (ExitWindowsEx needs the privilege held).
fn shutdown_privilege() -> Result<(), String> {
    use windows::Win32::Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED, SE_SHUTDOWN_NAME, TOKEN_ADJUST_PRIVILEGES,
        TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token).map_err(err)?;
        let mut privileges = TOKEN_PRIVILEGES { PrivilegeCount: 1, Privileges: [LUID_AND_ATTRIBUTES { Attributes: SE_PRIVILEGE_ENABLED, ..Default::default() }] };
        let out = LookupPrivilegeValueW(None, SE_SHUTDOWN_NAME, &mut privileges.Privileges[0].Luid)
            .and_then(|_| AdjustTokenPrivileges(token, false, Some(&privileges), 0, None, None))
            .map_err(err);
        let _ = CloseHandle(token);
        out
    }
}

pub fn power(action: PowerAction) -> Result<(), String> {
    use windows::Win32::System::Power::SetSuspendState;
    use windows::Win32::System::Shutdown::{
        ExitWindowsEx, LockWorkStation, EWX_LOGOFF, EWX_POWEROFF, EWX_REBOOT, EXIT_WINDOWS_FLAGS, SHTDN_REASON_FLAG_PLANNED,
        SHTDN_REASON_MAJOR_OTHER, SHTDN_REASON_MINOR_OTHER,
    };
    let reason = SHTDN_REASON_MAJOR_OTHER | SHTDN_REASON_MINOR_OTHER | SHTDN_REASON_FLAG_PLANNED;
    // Never EWX_FORCE: an app with unsaved work still gets to ask.
    let exit = |flags: EXIT_WINDOWS_FLAGS| -> Result<(), String> { unsafe { ExitWindowsEx(flags, reason).map_err(err) } };
    unsafe {
        match action {
            PowerAction::Lock => LockWorkStation().map_err(err),
            PowerAction::Sleep => {
                if SetSuspendState(false, false, false) {
                    Ok(())
                } else {
                    Err(crate::i18n::t("Windows did not allow it."))
                }
            }
            PowerAction::SignOut => exit(EWX_LOGOFF),
            PowerAction::Restart => {
                shutdown_privilege()?;
                exit(EWX_REBOOT)
            }
            PowerAction::ShutDown => {
                shutdown_privilege()?;
                exit(EWX_POWEROFF)
            }
        }
    }
}

pub fn battery() -> Battery {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut s = SYSTEM_POWER_STATUS::default();
    if unsafe { GetSystemPowerStatus(&mut s) }.is_err() {
        return Battery::default();
    }
    // 128: no system battery; 255: unknown.
    let has_battery = s.BatteryFlag != 128 && s.BatteryFlag != 255 && s.BatteryLifePercent <= 100;
    Battery { percent: has_battery.then_some(s.BatteryLifePercent), plugged_in: s.ACLineStatus == 1 }
}

// ── Typing ────────────────────────────────────────────────────────────────────

/// The window in front and its app's name, unless it is one of Coucou's own.
pub fn foreground() -> Option<(isize, String)> {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 || pid == std::process::id() {
            return None;
        }
        Some((hwnd.0 as isize, owner_app(pid).1))
    }
}

/// Types `text` into `window` if it is still the one in front. Unicode
/// keystrokes only: no virtual keys, so never Enter, Tab or a shortcut.
pub fn type_text(window: isize, text: &str) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    if unsafe { GetForegroundWindow() }.0 as isize != window {
        return Err(crate::i18n::t("The window in front changed: ask again."));
    }
    let key = |unit: u16, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: unit,
                dwFlags: if up { KEYEVENTF_UNICODE | KEYEVENTF_KEYUP } else { KEYEVENTF_UNICODE },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs: Vec<INPUT> = text.encode_utf16().flat_map(|u| [key(u, false), key(u, true)]).collect();
    for chunk in inputs.chunks(200) {
        let sent = unsafe { SendInput(chunk, std::mem::size_of::<INPUT>() as i32) };
        if sent as usize != chunk.len() {
            // UIPI: an app running as administrator refuses keystrokes from Coucou.
            return Err(crate::i18n::t("Windows refused the keystrokes (is the app running as administrator?)."));
        }
    }
    Ok(())
}

// ── Capture ───────────────────────────────────────────────────────────────────

/// That part of the screen (physical pixels, desktop coordinates) as BGRA rows, top-down.
pub fn capture_screen(x: i32, y: i32, w: i32, h: i32) -> Result<(u32, u32, Vec<u8>), String> {
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject,
        BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, SRCCOPY,
    };
    if w <= 0 || h <= 0 || w > 16_384 || h > 16_384 {
        return Err(crate::i18n::t("Circle a larger area."));
    }
    unsafe {
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(dc, bitmap.into());
        let copied = BitBlt(dc, 0, 0, w, h, Some(screen), x, y, SRCCOPY | CAPTUREBLT).is_ok();
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; (w as usize) * (h as usize) * 4];
        SelectObject(dc, old);
        let lines = if copied { GetDIBits(dc, bitmap, 0, h as u32, Some(pixels.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS) } else { 0 };
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);
        if lines == h {
            Ok((w as u32, h as u32, pixels))
        } else {
            Err(crate::i18n::t("Windows did not allow it."))
        }
    }
}
