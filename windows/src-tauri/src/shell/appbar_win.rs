// The Windows calls of the « top bar » mode (appbar.rs): the taskbar's
// auto-hide switch and the reserved band, through SHAppBarMessage only.

use parking_lot::Mutex;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, ABE_TOP, ABM_GETSTATE, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, ABM_SETSTATE, ABN_POSCHANGED,
    ABS_AUTOHIDE, APPBARDATA,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowW, GetMessageW, PostMessageW, PostQuitMessage,
    RegisterClassW, MSG, WINDOW_STYLE, WM_APP, WM_CLOSE, WM_DESTROY, WM_DISPLAYCHANGE, WM_DPICHANGED, WNDCLASSW, WS_EX_TOOLWINDOW,
    WS_POPUP,
};

fn data(hwnd: HWND) -> APPBARDATA {
    APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, hWnd: hwnd, ..Default::default() }
}

pub fn auto_hide() -> bool {
    let mut d = data(HWND::default());
    let state = unsafe { SHAppBarMessage(ABM_GETSTATE, &raw mut d) } as u32;
    state & ABS_AUTOHIDE != 0
}

pub fn set_auto_hide(on: bool) -> Result<(), String> {
    let mut d = data(HWND::default());
    let state = unsafe { SHAppBarMessage(ABM_GETSTATE, &raw mut d) } as u32;
    let next = if on { state | ABS_AUTOHIDE } else { state & !ABS_AUTOHIDE };
    d.lParam = LPARAM(next as isize);
    unsafe { SHAppBarMessage(ABM_SETSTATE, &raw mut d) };
    if auto_hide() == on {
        return Ok(());
    }
    // Some builds want the taskbar's handle in the message: it is only
    // looked up and handed to the same documented call, never touched.
    let tray = unsafe { FindWindowW(w!("Shell_TrayWnd"), None) }.map_err(|e| e.message())?;
    let mut d = data(tray);
    d.lParam = LPARAM(next as isize);
    unsafe { SHAppBarMessage(ABM_SETSTATE, &raw mut d) };
    if auto_hide() == on {
        Ok(())
    } else {
        Err(crate::i18n::t("Windows did not allow it."))
    }
}

// ── The reserved band ─────────────────────────────────────────────────────

const CALLBACK: u32 = WM_APP + 0x42;
static BAND: Mutex<Option<isize>> = Mutex::new(None);

/// Registers (or removes) the top band as an AppBar, from a hidden tool
/// window of its own that sleeps in GetMessage.
pub fn reserve(on: bool) {
    let mut band = BAND.lock();
    match (on, *band) {
        (true, None) => {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || run(tx));
            if let Ok(Some(h)) = rx.recv() {
                *band = Some(h);
            }
        }
        (false, Some(h)) => {
            *band = None;
            unsafe {
                let _ = PostMessageW(Some(HWND(h as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        _ => {}
    }
}

/// The band's height in physical pixels on the primary screen: the compact island's.
fn band_rect() -> RECT {
    unsafe {
        let monitor = MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(monitor, &raw mut info);
        let dpi = windows::Win32::UI::HiDpi::GetDpiForSystem().max(96);
        let height = (32 * dpi / 96) as i32;
        let m = info.rcMonitor;
        RECT { left: m.left, top: m.top, right: m.right, bottom: m.top + height }
    }
}

fn place(hwnd: HWND) {
    let mut d = data(hwnd);
    d.uEdge = ABE_TOP;
    d.rc = band_rect();
    let height = d.rc.bottom - d.rc.top;
    unsafe {
        SHAppBarMessage(ABM_QUERYPOS, &raw mut d);
        d.rc.bottom = d.rc.top + height;
        SHAppBarMessage(ABM_SETPOS, &raw mut d);
    }
}

fn run(ready: std::sync::mpsc::Sender<Option<isize>>) {
    unsafe {
        let class = w!("CoucouTopBand");
        let wc = WNDCLASSW { lpfnWndProc: Some(proc), lpszClassName: class, ..Default::default() };
        RegisterClassW(&raw const wc);
        let Ok(hwnd) = CreateWindowExW(WS_EX_TOOLWINDOW, class, w!(""), WINDOW_STYLE(WS_POPUP.0), 0, 0, 0, 0, None, None, None, None)
        else {
            let _ = ready.send(None);
            return;
        };
        let mut d = data(hwnd);
        d.uCallbackMessage = CALLBACK;
        if SHAppBarMessage(ABM_NEW, &raw mut d) == 0 {
            let _ = DestroyWindow(hwnd);
            let _ = ready.send(None);
            return;
        }
        place(hwnd);
        let _ = ready.send(Some(hwnd.0 as isize));
        let mut msg = MSG::default();
        while GetMessageW(&raw mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&raw const msg);
        }
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT { unsafe {
    match msg {
        // Another AppBar, the taskbar, the resolution or the DPI changed: place the band again.
        CALLBACK if wparam.0 as u32 == ABN_POSCHANGED => {
            place(hwnd);
            LRESULT(0)
        }
        WM_DISPLAYCHANGE | WM_DPICHANGED => {
            place(hwnd);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_CLOSE => {
            let mut d = data(hwnd);
            SHAppBarMessage(ABM_REMOVE, &raw mut d);
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}}
