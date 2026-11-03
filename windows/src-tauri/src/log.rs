// Small append-only log at %LOCALAPPDATA%\Coucou\coucou.log (Windows) or
// ~/.local/share/coucou/coucou.log (Linux) — the equivalent of nbLog() in
// HookServer.swift. Nothing leaves the machine.

use std::io::Write;

use crate::{platform, settings};

pub fn line(message: impl AsRef<str>) {
    let t = platform::local_time();
    let stamp = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        t.year, t.month, t.day, t.hour, t.minute, t.second
    );
    let dir = settings::local_dir();
    if platform::ensure_private_dir(&dir).is_err() {
        return;
    }
    let path = dir.join("coucou.log");
    // Keep it from growing forever: start fresh past ~1 MB.
    if std::fs::metadata(&path).map(|m| m.len() > 1_000_000).unwrap_or(false) {
        let _ = std::fs::remove_file(&path);
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    // Readable by us only, like the macOS log.
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    if let Ok(mut file) = options.open(path) {
        let _ = writeln!(file, "{stamp} {}", message.as_ref());
    }
}

/// "own 212 MB, system 91% of 15.9 GB" — what the crash and heartbeat lines carry, to tell a leak from a crash.
fn memory_summary() -> String {
    #[cfg(windows)]
    {
        use windows::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
        use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        use windows::Win32::System::Threading::GetCurrentProcess;
        let mut own = PROCESS_MEMORY_COUNTERS { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
        let mut sys = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
        unsafe {
            let _ = K32GetProcessMemoryInfo(GetCurrentProcess(), &raw mut own, own.cb);
            let _ = GlobalMemoryStatusEx(&raw mut sys);
        }
        format!(
            "own {} MB, system {}% of {:.1} GB",
            own.WorkingSetSize / 1_048_576,
            sys.dwMemoryLoad,
            sys.ullTotalPhys as f64 / 1_073_741_824.0
        )
    }
    #[cfg(not(windows))]
    {
        let kb = |path: &str, key: &str| -> u64 {
            std::fs::read_to_string(path)
                .ok()
                .and_then(|t| t.lines().find(|l| l.starts_with(key)).map(str::to_string))
                .and_then(|l| l.split_whitespace().nth(1).and_then(|n| n.parse().ok()))
                .unwrap_or(0)
        };
        let own = kb("/proc/self/status", "VmRSS:");
        let (total, free) = (kb("/proc/meminfo", "MemTotal:"), kb("/proc/meminfo", "MemAvailable:"));
        let used = if total > 0 { 100 * total.saturating_sub(free) / total } else { 0 };
        format!("own {} MB, system {used}% of {:.1} GB", own / 1024, total as f64 / 1_048_576.0)
    }
}

/// A panic used to end Coucou without a word, because the windowed build has no console. Now it leaves its
/// message, where it happened and the memory in the log, and the heartbeat shows how memory moved before it.
pub fn install_crash_log() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let place = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        line(format!("PANIC in thread '{}' at {place}: {message} ({})", thread.name().unwrap_or("unnamed"), memory_summary()));
        previous(info);
    }));
    let _ = std::thread::Builder::new().name("heartbeat".into()).spawn(|| loop {
        std::thread::sleep(std::time::Duration::from_secs(600));
        line(format!("heartbeat: {}", memory_summary()));
    });
}
