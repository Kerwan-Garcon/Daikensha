// Every Windows call of the Shell panels (docs/WINDOWS_SHELL.md), all of them
// documented Win32 or WinRT APIs. Nothing here runs unless the panel asks.

use windows::core::{w, BOOL, HSTRING, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HWND, LPARAM, RECT, TRUE};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, MonitorFromWindow, HDC, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindow, GetWindowLongPtrW, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId,
    IsIconic, IsWindow, IsWindowVisible, IsZoomed, SetForegroundWindow, SetWindowPos, ShowWindow, GWL_EXSTYLE, GW_OWNER,
    SWP_NOACTIVATE, SWP_NOZORDER, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, SW_SHOWNORMAL, WS_EX_TOOLWINDOW,
};

use super::apps::App;
use super::controls::{Controls, Display, Radio};
use super::stats::{display_name, Raw};
use super::windows_mgr::{move_to_screen, placed, placement, snap_rect, Action, Placement, Rect, Snap, WindowInfo};

fn ft(f: FILETIME) -> u64 {
    ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
}

fn err(e: windows::core::Error) -> String {
    e.message()
}

/// Runs `f` on a thread of its own with COM initialised (single-threaded
/// apartment, as the Shell objects want), so no Tauri thread's apartment is
/// touched.
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

// ── Stats ─────────────────────────────────────────────────────────────────────

/// The GPU's 3D engines, through PDH; opened the first time Stats asks.
pub struct GpuQuery {
    query: windows::Win32::System::Performance::PDH_HQUERY,
    counter: windows::Win32::System::Performance::PDH_HCOUNTER,
}

// Only ever used under the Sampler's mutex.
unsafe impl Send for GpuQuery {}

impl Drop for GpuQuery {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::System::Performance::PdhCloseQuery(self.query);
        }
    }
}

fn gpu_open() -> Option<GpuQuery> {
    use windows::Win32::System::Performance::{PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhOpenQueryW};
    unsafe {
        let mut query = Default::default();
        if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
            return None;
        }
        let mut counter = Default::default();
        if PdhAddEnglishCounterW(query, w!("\\GPU Engine(*engtype_3D)\\Utilization Percentage"), 0, &mut counter) != 0 {
            let _ = PdhCloseQuery(query);
            return None;
        }
        PdhCollectQueryData(query);
        Some(GpuQuery { query, counter })
    }
}

fn gpu_read(g: &GpuQuery) -> Option<f32> {
    use windows::Win32::System::Performance::{PdhCollectQueryData, PdhGetFormattedCounterArrayW, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE};
    unsafe {
        if PdhCollectQueryData(g.query) != 0 {
            return None;
        }
        let (mut size, mut count) = (0u32, 0u32);
        let _ = PdhGetFormattedCounterArrayW(g.counter, PDH_FMT_DOUBLE, &mut size, &mut count, None);
        if size == 0 {
            return Some(0.0);
        }
        // u64 words keep the items aligned.
        let mut buf = vec![0u64; (size as usize).div_ceil(8)];
        let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
        if PdhGetFormattedCounterArrayW(g.counter, PDH_FMT_DOUBLE, &mut size, &mut count, Some(items)) != 0 {
            return None;
        }
        let items = std::slice::from_raw_parts(items, count as usize);
        Some(items.iter().map(|i| i.FmtValue.Anonymous.doubleValue).sum::<f64>().clamp(0.0, 100.0) as f32)
    }
}

fn net_bytes() -> (u64, u64) {
    use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
    unsafe {
        let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        if GetIfTable2(&mut table).is_err() || table.is_null() {
            return (0, 0);
        }
        let t = &*table;
        let rows = std::slice::from_raw_parts(t.Table.as_ptr(), t.NumEntries as usize);
        // Physical adapters only: virtual ones (VPN, Hyper-V) repeat the same traffic.
        let (rx, tx) = rows
            .iter()
            .filter(|r| r.InterfaceAndOperStatusFlags._bitfield & 1 != 0)
            .fold((0u64, 0u64), |(a, b), r| (a + r.InOctets, b + r.OutOctets));
        FreeMibTable(table as *const _);
        (rx, tx)
    }
}

fn processes() -> Vec<(u32, String, u64, u64)> {
    use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    use windows::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    let mut out = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut more = Process32FirstW(snap, &mut e).is_ok();
        while more {
            if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, e.th32ProcessID) {
                let (mut c, mut x, mut k, mut u) = Default::default();
                let mut mem = PROCESS_MEMORY_COUNTERS { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
                if GetProcessTimes(h, &mut c, &mut x, &mut k, &mut u).is_ok() {
                    let _ = K32GetProcessMemoryInfo(h, &mut mem, mem.cb);
                    let len = e.szExeFile.iter().position(|&ch| ch == 0).unwrap_or(e.szExeFile.len());
                    out.push((e.th32ProcessID, String::from_utf16_lossy(&e.szExeFile[..len]), ft(k) + ft(u), mem.WorkingSetSize as u64));
                }
                let _ = CloseHandle(h);
            }
            more = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

pub fn read_stats(with_top: bool, gpu: &mut Option<GpuQuery>) -> Raw {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows::Win32::System::Threading::GetSystemTimes;
    let mut r = Raw::default();
    let (mut idle, mut kern, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
    if unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kern), Some(&mut user)) }.is_ok() {
        // Kernel time includes idle time.
        r.cpu_total = ft(kern) + ft(user);
        r.cpu_busy = r.cpu_total.saturating_sub(ft(idle));
    }
    let mut mem = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    if unsafe { GlobalMemoryStatusEx(&mut mem) }.is_ok() {
        r.mem_total = mem.ullTotalPhys;
        r.mem_used = mem.ullTotalPhys.saturating_sub(mem.ullAvailPhys);
    }
    (r.net_rx, r.net_tx) = net_bytes();
    if gpu.is_none() {
        *gpu = gpu_open();
    }
    r.gpu = gpu.as_ref().and_then(gpu_read);
    if with_top {
        r.procs = processes();
    }
    r
}

// ── Windows ───────────────────────────────────────────────────────────────────

fn rect(r: RECT) -> Rect {
    Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top }
}

/// Every screen: (handle, work area, whole area), in the order Windows lists them.
fn screens() -> Vec<(HMONITOR, Rect)> {
    unsafe extern "system" fn each(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let list = unsafe { &mut *(data.0 as *mut Vec<HMONITOR>) };
        list.push(m);
        TRUE
    }
    let mut monitors: Vec<HMONITOR> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut monitors as *mut _ as isize));
    }
    monitors
        .into_iter()
        .map(|m| {
            let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            unsafe {
                let _ = GetMonitorInfoW(m, &mut info);
            }
            (m, rect(info.rcWork))
        })
        .collect()
}

fn screen_of(hwnd: HWND, list: &[(HMONITOR, Rect)]) -> usize {
    let m = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    list.iter().position(|(h, _)| *h == m).unwrap_or(0)
}

fn pid_of(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    pid
}

fn exe_of(pid: u32) -> String {
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if ok {
            display_name(&String::from_utf16_lossy(&buf[..len as usize]))
        } else {
            String::new()
        }
    }
}

fn title_of(hwnd: HWND) -> String {
    let mut text = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, &mut text) };
    String::from_utf16_lossy(&text[..len.max(0) as usize])
}

/// A window the Windows tab may show and act on: visible, top level, titled,
/// not a tool window, not cloaked (another desktop, a suspended Store app),
/// and not Coucou's own.
fn eligible(hwnd: HWND) -> bool {
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() || !IsWindowVisible(hwnd).as_bool() || GetWindow(hwnd, GW_OWNER).is_ok() {
            return false;
        }
        if GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
            return false;
        }
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut _ as _, 4);
        if cloaked != 0 || title_of(hwnd).is_empty() {
            return false;
        }
    }
    pid_of(hwnd) != std::process::id()
}

fn all_windows() -> Vec<HWND> {
    unsafe extern "system" fn each(h: HWND, data: LPARAM) -> BOOL {
        let list = unsafe { &mut *(data.0 as *mut Vec<HWND>) };
        list.push(h);
        TRUE
    }
    let mut list: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut list as *mut _ as isize));
    }
    list.into_iter().filter(|h| eligible(*h)).collect()
}

pub fn list_windows() -> Vec<WindowInfo> {
    let list = screens();
    let fg = unsafe { GetForegroundWindow() };
    all_windows()
        .into_iter()
        .map(|h| WindowInfo {
            id: (h.0 as isize).to_string(),
            title: title_of(h),
            app: exe_of(pid_of(h)),
            screen: screen_of(h, &list),
            minimized: unsafe { IsIconic(h) }.as_bool(),
            focused: h == fg,
        })
        .collect()
}

fn parse_id(id: &str) -> Result<HWND, String> {
    let raw: isize = id.parse().map_err(|_| "bad window id")?;
    let hwnd = HWND(raw as *mut _);
    if eligible(hwnd) {
        Ok(hwnd)
    } else {
        Err("that window is gone".into())
    }
}

/// The window's visible frame, and the invisible border Windows 10/11 adds
/// around it (so a half really meets the other half).
fn frame(hwnd: HWND) -> (Rect, (i32, i32, i32, i32)) {
    let mut outer = RECT::default();
    let mut visible = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut outer);
        if DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut visible as *mut _ as _, std::mem::size_of::<RECT>() as u32).is_err() {
            visible = outer;
        }
    }
    let border = (visible.left - outer.left, visible.top - outer.top, outer.right - visible.right, outer.bottom - visible.bottom);
    (rect(visible), border)
}

fn place(hwnd: HWND, r: Rect) {
    unsafe {
        if IsZoomed(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let (_, (l, t, rr, b)) = frame(hwnd);
        let _ = SetWindowPos(hwnd, None, r.x - l, r.y - t, r.w + l + rr, r.h + t + b, SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

pub fn window_action(id: &str, action: Action) -> Result<(), String> {
    let hwnd = parse_id(id)?;
    unsafe {
        match action {
            Action::Focus => {
                if IsIconic(hwnd).as_bool() {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                }
                // Allowed: the click on the island that asked for it was the last input.
                let _ = SetForegroundWindow(hwnd);
            }
            Action::Minimize => {
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
            }
            Action::Maximize | Action::Snap { to: Snap::Full } => {
                let _ = ShowWindow(hwnd, SW_MAXIMIZE);
            }
            Action::Restore => {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            Action::Snap { to } => {
                let list = screens();
                let work = list.get(screen_of(hwnd, &list)).map(|s| s.1).ok_or("no screen")?;
                place(hwnd, snap_rect(work, to));
            }
            Action::Screen { index } => {
                let list = screens();
                let to = list.get(index).map(|s| s.1).ok_or("no such screen")?;
                let from = list.get(screen_of(hwnd, &list)).map(|s| s.1).ok_or("no screen")?;
                let zoomed = IsZoomed(hwnd).as_bool();
                if zoomed {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                }
                let (now, _) = frame(hwnd);
                place(hwnd, move_to_screen(now, from, to));
                if zoomed {
                    let _ = ShowWindow(hwnd, SW_MAXIMIZE);
                }
            }
        }
    }
    Ok(())
}

pub fn current_placements() -> Vec<Placement> {
    let list = screens();
    all_windows()
        .into_iter()
        .filter(|h| !unsafe { IsIconic(*h) }.as_bool())
        .filter_map(|h| {
            let app = exe_of(pid_of(h));
            let screen = screen_of(h, &list);
            let work = list.get(screen)?.1;
            (!app.is_empty()).then(|| placement(&app, screen, frame(h).0, work))
        })
        .collect()
}

pub fn apply_placements(placements: &[Placement]) -> usize {
    let list = screens();
    let mut used: Vec<HWND> = Vec::new();
    let windows: Vec<(HWND, String)> = all_windows().into_iter().map(|h| (h, exe_of(pid_of(h)))).collect();
    let mut moved = 0;
    for p in placements {
        let Some((hwnd, _)) = windows.iter().find(|(h, app)| app == &p.app && !used.contains(h)) else { continue };
        let Some(work) = list.get(p.screen).or_else(|| list.first()).map(|s| s.1) else { continue };
        place(*hwnd, placed(p, work));
        used.push(*hwnd);
        moved += 1;
    }
    moved
}

// ── Apps ──────────────────────────────────────────────────────────────────────

pub fn list_apps() -> Result<Vec<App>, String> {
    with_com(|| -> Result<Vec<App>, String> {
        use windows::Win32::System::Com::CoTaskMemFree;
        use windows::Win32::UI::Shell::{
            BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, SHGetKnownFolderItem, KF_FLAG_DEFAULT, SIGDN_NORMALDISPLAY,
            SIGDN_PARENTRELATIVEPARSING,
        };
        unsafe {
            let folder: IShellItem = SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None).map_err(err)?;
            let items: IEnumShellItems = folder.BindToHandler(None, &BHID_EnumItems).map_err(err)?;
            let mut out = Vec::new();
            loop {
                let mut one = [None];
                let mut fetched = 0u32;
                if items.Next(&mut one, Some(&mut fetched)).is_err() || fetched == 0 {
                    break;
                }
                let Some(item) = one[0].take() else { break };
                let read = |sigdn| -> Option<String> {
                    let p = item.GetDisplayName(sigdn).ok()?;
                    let s = p.to_string().ok();
                    CoTaskMemFree(Some(p.0 as *const _));
                    s
                };
                if let (Some(name), Some(id)) = (read(SIGDN_NORMALDISPLAY), read(SIGDN_PARENTRELATIVEPARSING)) {
                    out.push(App { id, name });
                }
                if out.len() >= 2000 {
                    break;
                }
            }
            Ok(out)
        }
    })?
}

pub fn shell_open(target: &str) -> Result<(), String> {
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW};
    let target = HSTRING::from(target);
    with_com(move || unsafe {
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC,
            lpVerb: w!("open"),
            lpFile: PCWSTR(target.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        ShellExecuteExW(&mut info).map_err(err)
    })?
}

pub fn launch_app(id: &str) -> Result<(), String> {
    shell_open(&format!("shell:AppsFolder\\{id}"))
}

// ── Controls ──────────────────────────────────────────────────────────────────

fn endpoint() -> windows::core::Result<windows::Win32::Media::Audio::IMMDevice> {
    use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
    unsafe {
        let e: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        e.GetDefaultAudioEndpoint(eRender, eConsole)
    }
}

fn volume_control() -> windows::core::Result<windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume> {
    use windows::Win32::System::Com::CLSCTX_ALL;
    unsafe { endpoint()?.Activate(CLSCTX_ALL, None) }
}

fn output_name() -> Option<String> {
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::System::Com::STGM_READ;
    unsafe {
        let store = endpoint().ok()?.OpenPropertyStore(STGM_READ).ok()?;
        let value = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
        let name = value.to_string();
        (!name.is_empty()).then_some(name)
    }
}

pub fn read_controls() -> Controls {
    let audio = with_com(|| {
        let v = volume_control().ok();
        let level = v.as_ref().and_then(|v| unsafe { v.GetMasterVolumeLevelScalar() }.ok());
        let muted = v.as_ref().and_then(|v| unsafe { v.GetMute() }.ok()).map(|m| m.as_bool());
        (level, muted, output_name())
    })
    .unwrap_or((None, None, None));
    Controls { volume: audio.0, muted: audio.1, output: audio.2, radios: radios(), displays: displays() }
}

pub fn set_volume(level: f32) -> Result<(), String> {
    with_com(move || -> Result<(), String> { unsafe { volume_control().map_err(err)?.SetMasterVolumeLevelScalar(level, std::ptr::null()).map_err(err) } })?
}

pub fn set_mute(muted: bool) -> Result<(), String> {
    with_com(move || -> Result<(), String> { unsafe { volume_control().map_err(err)?.SetMute(muted, std::ptr::null()).map_err(err) } })?
}

/// WinRT on a thread of its own (multithreaded apartment).
fn with_winrt<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};
    std::thread::spawn(move || unsafe {
        let init = RoInitialize(RO_INIT_MULTITHREADED);
        let out = f();
        if init.is_ok() {
            RoUninitialize();
        }
        out
    })
    .join()
    .map_err(|_| "WinRT thread failed".to_string())
}

fn kind_name(kind: windows::Devices::Radios::RadioKind) -> &'static str {
    use windows::Devices::Radios::RadioKind;
    match kind {
        RadioKind::WiFi => "wifi",
        RadioKind::Bluetooth => "bluetooth",
        RadioKind::MobileBroadband => "mobile",
        _ => "other",
    }
}

fn radios() -> Vec<Radio> {
    with_winrt(|| -> windows::core::Result<Vec<Radio>> {
        use windows::Devices::Radios::{Radio as WinRadio, RadioState};
        let list = WinRadio::GetRadiosAsync()?.get()?;
        let mut out = Vec::new();
        for r in list {
            out.push(Radio { kind: kind_name(r.Kind()?).into(), name: r.Name()?.to_string(), on: r.State()? == RadioState::On });
        }
        Ok(out)
    })
    .ok()
    .and_then(Result::ok)
    .unwrap_or_default()
}

pub fn set_radio(kind: &str, on: bool) -> Result<(), String> {
    let kind = kind.to_string();
    with_winrt(move || -> windows::core::Result<bool> {
        use windows::Devices::Radios::{Radio as WinRadio, RadioAccessStatus, RadioState};
        if WinRadio::RequestAccessAsync()?.get()? != RadioAccessStatus::Allowed {
            return Ok(false);
        }
        let mut done = false;
        for r in WinRadio::GetRadiosAsync()?.get()? {
            if kind_name(r.Kind()?) == kind {
                r.SetStateAsync(if on { RadioState::On } else { RadioState::Off })?.get()?;
                done = true;
            }
        }
        Ok(done)
    })?
    .map_err(err)
    .and_then(|done| if done { Ok(()) } else { Err(crate::i18n::t("Windows did not allow it.")) })
}

/// The physical monitors behind each screen, for DDC/CI.
fn physical(index: usize) -> Option<Vec<windows::Win32::Devices::Display::PHYSICAL_MONITOR>> {
    use windows::Win32::Devices::Display::{GetNumberOfPhysicalMonitorsFromHMONITOR, GetPhysicalMonitorsFromHMONITOR, PHYSICAL_MONITOR};
    let (m, _) = *screens().get(index)?;
    unsafe {
        let mut n = 0u32;
        GetNumberOfPhysicalMonitorsFromHMONITOR(m, &mut n).ok()?;
        if n == 0 {
            return None;
        }
        let mut list = vec![PHYSICAL_MONITOR::default(); n as usize];
        GetPhysicalMonitorsFromHMONITOR(m, &mut list).ok()?;
        Some(list)
    }
}

fn displays() -> Vec<Display> {
    use windows::Win32::Devices::Display::{DestroyPhysicalMonitors, GetMonitorBrightness};
    (0..screens().len())
        .map(|index| {
            let mut name = crate::i18n::tf("Screen {n}", &[("n", &(index + 1).to_string())]);
            let mut brightness = None;
            if let Some(list) = physical(index) {
                let first = &list[0];
                // A packed struct: copy the field out before reading it.
                let raw = first.szPhysicalMonitorDescription;
                let desc = String::from_utf16_lossy(&raw);
                let desc = desc.trim_end_matches('\0').trim();
                if !desc.is_empty() && desc != "Generic PnP Monitor" {
                    name = desc.to_string();
                }
                let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
                if unsafe { GetMonitorBrightness(first.hPhysicalMonitor, &mut min, &mut cur, &mut max) } != 0 && max > min {
                    brightness = Some(((cur - min) * 100 / (max - min)).min(100));
                }
                unsafe {
                    let _ = DestroyPhysicalMonitors(&list);
                }
            }
            Display { index, name, brightness }
        })
        .collect()
}

pub fn set_brightness(index: usize, value: u32) -> Result<(), String> {
    use windows::Win32::Devices::Display::{DestroyPhysicalMonitors, GetMonitorBrightness, SetMonitorBrightness};
    let list = physical(index).ok_or("no such screen")?;
    let ok = unsafe {
        let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
        let ok = GetMonitorBrightness(list[0].hPhysicalMonitor, &mut min, &mut cur, &mut max) != 0
            && max > min
            && SetMonitorBrightness(list[0].hPhysicalMonitor, min + (max - min) * value / 100) != 0;
        let _ = DestroyPhysicalMonitors(&list);
        ok
    };
    if ok {
        Ok(())
    } else {
        Err(crate::i18n::t("This screen does not let its brightness be changed."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Read-only calls against the real system (the Windows CI runner): none
    /// may fail loudly, and what they return must be sane.
    #[test]
    fn the_read_only_calls_work_on_a_real_windows() {
        for w in list_windows() {
            assert!(!w.title.is_empty());
            assert!(w.id.parse::<isize>().is_ok());
        }
        assert!(!screens().is_empty(), "no screen");
        let apps = list_apps().expect("the Start menu's apps");
        assert!(apps.iter().all(|a| !a.id.is_empty()));
        let controls = read_controls();
        assert!(controls.volume.is_none_or(|v| (0.0..=1.0).contains(&v)));
        assert!(window_action("12345678", Action::Minimize).is_err(), "a made-up handle is refused");
        assert!(window_action("not a number", Action::Focus).is_err());
    }
}
