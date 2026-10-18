//! « Coucou Shell », phase 0: what this Windows machine offers to the panels
//! and voice tools of docs/WINDOWS_SHELL.md, read only, and what the Stats
//! panel's sampling costs. Nothing is changed: no volume, radio, wallpaper,
//! taskbar or window is touched, and the clipboard's content is never read.
//!
//!   cargo run --release            (from windows/spikes/shell-spike)
//!
//! Prints a Markdown report to paste into docs/WINDOWS_SHELL.md, "Mesures".

#[cfg(not(windows))]
fn main() {
    eprintln!("This probe only runs on Windows.");
}

#[cfg(windows)]
fn main() {
    win::run();
}

#[cfg(windows)]
mod win {
    use std::time::{Duration, Instant};
    use windows::core::{w, BOOL, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, FILETIME, HWND, LPARAM, RECT, TRUE};

    fn section<F: FnOnce() -> Result<Vec<String>, String>>(title: &str, f: F) {
        println!("\n## {title}\n");
        let t = Instant::now();
        match f() {
            Ok(lines) => {
                for l in lines {
                    println!("- {l}");
                }
            }
            Err(e) => println!("- **error**: {e}"),
        }
        println!("- _({} ms)_", t.elapsed().as_millis());
    }

    fn err<E: std::fmt::Display>(e: E) -> String {
        e.to_string()
    }

    fn ft(f: FILETIME) -> u64 {
        ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
    }

    pub fn run() {
        println!("# Coucou Shell — phase 0 probe\n");
        let v = windows_version();
        println!("Windows {v}");
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        section("Controls: audio (read only)", audio);
        section("Controls: radios — Wi-Fi, Bluetooth (Windows.Devices.Radios, read only)", radios);
        section("Controls: displays, DPI, brightness (DDC/CI, read only)", displays);
        section("Controls: theme and night light", theme);
        section("Windows: top-level windows", windows_list);
        section("Apps: shell:AppsFolder", apps_folder);
        section("Wallpaper: IDesktopWallpaper (read only)", wallpaper);
        section("Taskbar: SHAppBarMessage(ABM_GETSTATE)", taskbar);
        section("Clipboard: exclusion formats (content never read)", clipboard);
        section("Stats: sampling cost, 1 Hz for 20 s (CPU, RAM, network, GPU, top 5)", stats_cost);
    }

    fn windows_version() -> String {
        use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
        let mut buf = [0u16; 64];
        let mut len = (buf.len() * 2) as u32;
        let key = w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
        let build = unsafe {
            RegGetValueW(HKEY_LOCAL_MACHINE, key, w!("CurrentBuild"), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as _), Some(&mut len))
        };
        if build.is_err() {
            return "(unknown build)".into();
        }
        let b = String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]);
        let n: u32 = b.parse().unwrap_or(0);
        format!("build {b} ({})", if n >= 22000 { "Windows 11" } else { "Windows 10" })
    }

    fn audio() -> Result<Vec<String>, String> {
        use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
        use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE};
        use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
        unsafe {
            let e: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(err)?;
            let dev = e.GetDefaultAudioEndpoint(eRender, eConsole).map_err(err)?;
            let vol: IAudioEndpointVolume = dev.Activate(CLSCTX_ALL, None).map_err(err)?;
            let level = vol.GetMasterVolumeLevelScalar().map_err(err)?;
            let muted = vol.GetMute().map_err(err)?.as_bool();
            let outputs = e.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE).map_err(err)?.GetCount().map_err(err)?;
            Ok(vec![
                format!("default output volume: {:.0} %, muted: {muted}", level * 100.0),
                format!("active outputs: {outputs}"),
                "set volume / mute: IAudioEndpointVolume (documented) ✓".into(),
                "switch the default output: **no documented API** (only the undocumented IPolicyConfig) — see the doc".into(),
            ])
        }
    }

    fn radios() -> Result<Vec<String>, String> {
        use windows::Devices::Radios::Radio;
        let list = Radio::GetRadiosAsync().map_err(err)?.get().map_err(err)?;
        let mut out = Vec::new();
        for r in list {
            let kind = r.Kind().map_err(err)?;
            let state = r.State().map_err(err)?;
            out.push(format!("{} — kind {:?}, state {:?}", r.Name().map(|n| n.to_string()).unwrap_or_default(), kind, state));
        }
        if out.is_empty() {
            out.push("no radio reported".into());
        }
        out.push("toggling needs Radio::RequestAccessAsync then SetStateAsync (not done here)".into());
        Ok(out)
    }

    fn displays() -> Result<Vec<String>, String> {
        use windows::Win32::Devices::Display::{
            DestroyPhysicalMonitors, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR, GetPhysicalMonitorsFromHMONITOR,
            PHYSICAL_MONITOR,
        };
        use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW};
        use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
        unsafe extern "system" fn each(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
            let list = &mut *(data.0 as *mut Vec<HMONITOR>);
            list.push(m);
            TRUE
        }
        let mut monitors: Vec<windows::Win32::Graphics::Gdi::HMONITOR> = Vec::new();
        let mut out = Vec::new();
        unsafe {
            let _ = EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut monitors as *mut _ as isize));
            for m in monitors {
                let mut info = MONITORINFOEXW::default();
                info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
                let _ = GetMonitorInfoW(m, &mut info as *mut _ as *mut _);
                let r = info.monitorInfo.rcMonitor;
                let (mut dx, mut dy) = (0u32, 0u32);
                let _ = GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
                let mut n = 0u32;
                let mut bright = "DDC/CI brightness: not available (an internal laptop panel needs WMI instead)".to_string();
                if GetNumberOfPhysicalMonitorsFromHMONITOR(m, &mut n).is_ok() && n > 0 {
                    let mut phys = vec![PHYSICAL_MONITOR::default(); n as usize];
                    if GetPhysicalMonitorsFromHMONITOR(m, &mut phys).is_ok() {
                        let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
                        if GetMonitorBrightness(phys[0].hPhysicalMonitor, &mut min, &mut cur, &mut max) != 0 {
                            bright = format!("DDC/CI brightness {cur} (range {min}–{max}) ✓");
                        }
                        let _ = DestroyPhysicalMonitors(&phys);
                    }
                }
                out.push(format!(
                    "{}×{} at ({}, {}), {} dpi{} — {bright}",
                    r.right - r.left,
                    r.bottom - r.top,
                    r.left,
                    r.top,
                    dx,
                    if info.monitorInfo.dwFlags & 1 != 0 { ", primary" } else { "" }
                ));
            }
        }
        Ok(out)
    }

    fn reg_dword(key: PCWSTR, value: PCWSTR) -> Option<u32> {
        use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
        let mut data = 0u32;
        let mut len = 4u32;
        unsafe { RegGetValueW(HKEY_CURRENT_USER, key, value, RRF_RT_REG_DWORD, None, Some(&mut data as *mut _ as _), Some(&mut len)) }
            .ok()
            .ok()
            .map(|_| data)
    }

    fn theme() -> Result<Vec<String>, String> {
        let key = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
        let apps = reg_dword(key, w!("AppsUseLightTheme"));
        let system = reg_dword(key, w!("SystemUsesLightTheme"));
        Ok(vec![
            format!("AppsUseLightTheme = {apps:?}, SystemUsesLightTheme = {system:?}"),
            "light/dark: only these per-user values + a WM_SETTINGCHANGE \"ImmersiveColorSet\" broadcast — widely used, not a documented API".into(),
            "night light: **no documented API** (its state is an undocumented CloudStore blob) — see the doc".into(),
        ])
    }

    fn windows_list() -> Result<Vec<String>, String> {
        use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
        use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindow, GetWindowTextLengthW, IsWindowVisible, GW_OWNER};
        unsafe extern "system" fn each(h: HWND, data: LPARAM) -> BOOL {
            let list = &mut *(data.0 as *mut Vec<HWND>);
            list.push(h);
            TRUE
        }
        let mut all: Vec<HWND> = Vec::new();
        let t = Instant::now();
        let mut shown = 0;
        unsafe {
            let _ = EnumWindows(Some(each), LPARAM(&mut all as *mut _ as isize));
            for &h in &all {
                if !IsWindowVisible(h).as_bool() || GetWindowTextLengthW(h) == 0 || GetWindow(h, GW_OWNER).is_ok_and(|o| !o.is_invalid()) {
                    continue;
                }
                let mut cloaked = 0u32;
                let _ = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut _ as _, 4);
                if cloaked == 0 {
                    shown += 1;
                }
            }
        }
        Ok(vec![
            format!("{} top-level windows, {shown} that a window list would show, in {} µs", all.len(), t.elapsed().as_micros()),
            "focus / minimise / maximise / move: SetForegroundWindow, ShowWindow, SetWindowPos ✓ (documented)".into(),
            "Snap Layouts themselves: no API; halves and quarters are computed from the work area and set with SetWindowPos".into(),
        ])
    }

    fn apps_folder() -> Result<Vec<String>, String> {
        use windows::Win32::UI::Shell::{BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, SHGetKnownFolderItem, KF_FLAG_DEFAULT};
        let t = Instant::now();
        unsafe {
            let folder: IShellItem = SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None).map_err(err)?;
            let items: IEnumShellItems = folder.BindToHandler(None, &BHID_EnumItems).map_err(err)?;
            let mut count = 0;
            loop {
                let mut one = [None];
                let mut fetched = 0u32;
                if items.Next(&mut one, Some(&mut fetched)).is_err() || fetched == 0 {
                    break;
                }
                count += 1;
            }
            Ok(vec![
                format!("{count} apps (desktop and Store) in {} ms", t.elapsed().as_millis()),
                "launch: open shell:AppsFolder\\<AppUserModelID> through ShellExecute — never an arbitrary path".into(),
            ])
        }
    }

    fn wallpaper() -> Result<Vec<String>, String> {
        use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
        use windows::Win32::UI::Shell::{DesktopWallpaper, IDesktopWallpaper};
        unsafe {
            let wp: IDesktopWallpaper = CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL).map_err(err)?;
            let n = wp.GetMonitorDevicePathCount().map_err(err)?;
            let mut out = vec![format!("{n} monitor(s) known to IDesktopWallpaper")];
            for i in 0..n {
                let id: PWSTR = wp.GetMonitorDevicePathAt(i).map_err(err)?;
                let path = wp.GetWallpaper(PCWSTR(id.0)).map(|p| p.to_string().unwrap_or_default()).unwrap_or_default();
                let name = std::path::Path::new(&path).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| "(none)".into());
                out.push(format!("monitor {i}: current wallpaper file {name}"));
            }
            out.push("set: IDesktopWallpaper::SetWallpaper(monitor, path) ✓ (documented)".into());
            Ok(out)
        }
    }

    fn taskbar() -> Result<Vec<String>, String> {
        use windows::Win32::UI::Shell::{SHAppBarMessage, ABM_GETSTATE, APPBARDATA};
        let mut data = APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, ..Default::default() };
        let state = unsafe { SHAppBarMessage(ABM_GETSTATE, &mut data) };
        Ok(vec![
            format!("state {state:#x} — auto-hide: {}, always on top: {}", state & 1 != 0, state & 2 != 0),
            "set: SHAppBarMessage(ABM_SETSTATE, ABS_AUTOHIDE) (documented; one setting for every monitor's taskbar) — not done here".into(),
            "reserve the top of the screen: ABM_NEW / ABM_QUERYPOS / ABM_SETPOS, ABN_POSCHANGED, ABM_REMOVE ✓".into(),
        ])
    }

    fn clipboard() -> Result<Vec<String>, String> {
        use windows::Win32::System::DataExchange::{IsClipboardFormatAvailable, RegisterClipboardFormatW};
        let mut out = Vec::new();
        for name in [w!("ExcludeClipboardContentFromMonitorProcessing"), w!("CanIncludeInClipboardHistory"), w!("CanUploadToCloudClipboard")] {
            let id = unsafe { RegisterClipboardFormatW(name) };
            let present = id != 0 && unsafe { IsClipboardFormatAvailable(id) }.is_ok();
            out.push(format!("{} → format {id}, on the clipboard now: {present}", unsafe { name.to_string() }.unwrap_or_default()));
        }
        out.push("watch: AddClipboardFormatListener + WM_CLIPBOARDUPDATE (no polling) ✓".into());
        Ok(out)
    }

    // ── Stats: what a 1 Hz sampler costs ──────────────────────────────────────

    struct Gpu {
        query: windows::Win32::System::Performance::PDH_HQUERY,
        counter: windows::Win32::System::Performance::PDH_HCOUNTER,
    }

    fn gpu_open() -> Option<Gpu> {
        use windows::Win32::System::Performance::{PdhAddEnglishCounterW, PdhCollectQueryData, PdhOpenQueryW};
        unsafe {
            let mut q = Default::default();
            if PdhOpenQueryW(PCWSTR::null(), 0, &mut q) != 0 {
                return None;
            }
            let mut c = Default::default();
            if PdhAddEnglishCounterW(q, w!("\\GPU Engine(*engtype_3D)\\Utilization Percentage"), 0, &mut c) != 0 {
                return None;
            }
            PdhCollectQueryData(q);
            Some(Gpu { query: q, counter: c })
        }
    }

    fn gpu_read(g: &Gpu) -> Option<f64> {
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
            let mut buf = vec![0u8; size as usize];
            if PdhGetFormattedCounterArrayW(g.counter, PDH_FMT_DOUBLE, &mut size, &mut count, Some(buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W)) != 0 {
                return None;
            }
            let items = std::slice::from_raw_parts(buf.as_ptr() as *const PDH_FMT_COUNTERVALUE_ITEM_W, count as usize);
            Some(items.iter().map(|i| i.FmtValue.Anonymous.doubleValue).sum::<f64>().min(100.0))
        }
    }

    fn net_bytes() -> u64 {
        use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
        unsafe {
            let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
            if GetIfTable2(&mut table).is_err() || table.is_null() {
                return 0;
            }
            let t = &*table;
            let rows = std::slice::from_raw_parts(t.Table.as_ptr(), t.NumEntries as usize);
            let sum = rows.iter().map(|r| r.InOctets + r.OutOctets).sum();
            FreeMibTable(table as *const _);
            sum
        }
    }

    /// (pid, name, cpu 100-ns ticks so far, working set bytes) of every process we may query.
    fn processes() -> Vec<(u32, String, u64, usize)> {
        use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
        use windows::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
        use windows::Win32::System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        let mut out = Vec::new();
        unsafe {
            let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
            let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            let mut ok = Process32FirstW(snap, &mut e).is_ok();
            while ok {
                if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, e.th32ProcessID) {
                    let (mut c, mut x, mut k, mut u) = Default::default();
                    let mut mem = PROCESS_MEMORY_COUNTERS { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
                    if GetProcessTimes(h, &mut c, &mut x, &mut k, &mut u).is_ok() {
                        let _ = K32GetProcessMemoryInfo(h, &mut mem, mem.cb);
                        let len = e.szExeFile.iter().position(|&ch| ch == 0).unwrap_or(e.szExeFile.len());
                        out.push((e.th32ProcessID, String::from_utf16_lossy(&e.szExeFile[..len]), ft(k) + ft(u), mem.WorkingSetSize));
                    }
                    let _ = CloseHandle(h);
                }
                ok = Process32NextW(snap, &mut e).is_ok();
            }
            let _ = CloseHandle(snap);
        }
        out
    }

    fn own_cpu() -> u64 {
        use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
        let (mut c, mut x, mut k, mut u) = Default::default();
        unsafe {
            let _ = GetProcessTimes(GetCurrentProcess(), &mut c, &mut x, &mut k, &mut u);
        }
        ft(k) + ft(u)
    }

    fn stats_cost() -> Result<Vec<String>, String> {
        use windows::Win32::System::SystemInformation::{GetSystemInfo, GlobalMemoryStatusEx, MEMORYSTATUSEX, SYSTEM_INFO};
        use windows::Win32::System::Threading::GetSystemTimes;
        let mut si = SYSTEM_INFO::default();
        unsafe { GetSystemInfo(&mut si) };
        let cores = si.dwNumberOfProcessors.max(1) as f64;
        let gpu = gpu_open();
        let (mut idle0, mut kern0, mut user0) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
        unsafe { GetSystemTimes(Some(&mut idle0), Some(&mut kern0), Some(&mut user0)).map_err(err)? };
        let mut prev_procs = processes();
        let mut prev_net = net_bytes();
        let start_cpu = own_cpu();
        let start = Instant::now();
        let mut last_line = String::new();
        let mut sample_us = Vec::new();
        for _ in 0..20 {
            std::thread::sleep(Duration::from_secs(1));
            let t = Instant::now();
            let (mut idle, mut kern, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
            unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kern), Some(&mut user)).map_err(err)? };
            let busy = (ft(kern) - ft(kern0)) + (ft(user) - ft(user0)) - (ft(idle) - ft(idle0));
            let total = (ft(kern) - ft(kern0)) + (ft(user) - ft(user0));
            let cpu = 100.0 * busy as f64 / total.max(1) as f64;
            (idle0, kern0, user0) = (idle, kern, user);
            let mut mem = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
            unsafe { GlobalMemoryStatusEx(&mut mem).map_err(err)? };
            let net = net_bytes();
            let rate = net.saturating_sub(prev_net);
            prev_net = net;
            let procs = processes();
            let mut top: Vec<(String, u64)> = procs
                .iter()
                .filter_map(|(pid, name, cpu, _)| prev_procs.iter().find(|p| p.0 == *pid).map(|p| (name.clone(), cpu.saturating_sub(p.2))))
                .collect();
            top.sort_by_key(|t| std::cmp::Reverse(t.1));
            prev_procs = procs;
            let g = gpu.as_ref().and_then(gpu_read);
            sample_us.push(t.elapsed().as_micros());
            last_line = format!(
                "CPU {cpu:.0} %, RAM {} %, net {} KB/s, GPU {}, top: {}",
                mem.dwMemoryLoad,
                rate / 1024,
                g.map(|g| format!("{g:.0} %")).unwrap_or_else(|| "n/a".into()),
                top.iter().take(5).map(|(n, t)| format!("{n} {:.1} %", *t as f64 / 1e5 / cores)).collect::<Vec<_>>().join(", ")
            );
        }
        let used = (own_cpu() - start_cpu) as f64 / 1e7;
        let wall = start.elapsed().as_secs_f64();
        sample_us.sort();
        Ok(vec![
            format!("last sample: {last_line}"),
            format!("one sample takes {} µs (median), {} µs (worst)", sample_us[sample_us.len() / 2], sample_us[sample_us.len() - 1]),
            format!(
                "the sampler used {:.3} s of CPU in {:.0} s: {:.2} % of one core, {:.3} % of this {}-core machine",
                used,
                wall,
                100.0 * used / wall,
                100.0 * used / wall / cores,
                cores
            ),
        ])
    }
}
