// The Stats tab (docs/WINDOWS_SHELL.md): CPU, memory, network, GPU where the
// system says, and the processes that use the most. Nothing runs in the
// background: the panel asks for a sample once a second while it is open, and
// each sample is the difference with the one before. Closed, it costs nothing.

use std::sync::Mutex;
use std::time::Instant;

use serde::Serialize;

/// What the system reports at one moment, before any difference is taken.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Raw {
    /// CPU time of the whole machine, all cores, in the platform's unit.
    pub cpu_busy: u64,
    pub cpu_total: u64,
    pub mem_used: u64,
    pub mem_total: u64,
    /// Bytes received and sent since boot, every real interface together.
    pub net_rx: u64,
    pub net_tx: u64,
    /// 3D engine use, 0–100, where the system has a counter for it.
    pub gpu: Option<f32>,
    /// (pid, name, CPU time in the same unit as `cpu_total`, resident memory).
    /// Empty when this sample did not list them.
    pub procs: Vec<(u32, String, u64, u64)>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Proc {
    pub name: String,
    /// Share of the whole machine, 0–100.
    pub cpu: f32,
    pub mem: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub cpu: f32,
    pub mem_used: u64,
    pub mem_total: u64,
    /// Bytes per second.
    pub net_rx: u64,
    pub net_tx: u64,
    pub gpu: Option<f32>,
    /// The five busiest processes; empty when not listed this time.
    pub top: Vec<Proc>,
    /// Coucou's own share and memory, so the panel can show what it costs.
    pub own_cpu: Option<f32>,
    pub own_mem: Option<u64>,
}

/// How many processes the tab shows.
pub const TOP: usize = 5;

fn pct(part: u64, whole: u64) -> f32 {
    if whole == 0 {
        return 0.0;
    }
    (100.0 * part as f64 / whole as f64).clamp(0.0, 100.0) as f32
}

/// The last process listing and the machine's CPU time when it was taken: the
/// base the next listing is compared with (processes are listed every other
/// sample, so this is not always the previous reading).
pub type ProcBase = (Vec<(u32, String, u64, u64)>, u64);

/// The sample between two readings `secs` apart. A first reading (no `prev`)
/// gives memory and GPU only; the top processes need a `base` listing too.
pub fn compute(prev: Option<&Raw>, cur: &Raw, secs: f64, own_pid: u32, base: Option<&ProcBase>) -> Sample {
    let mut s = Sample { mem_used: cur.mem_used, mem_total: cur.mem_total, gpu: cur.gpu, ..Default::default() };
    let Some(prev) = prev else { return s };
    let total = cur.cpu_total.saturating_sub(prev.cpu_total);
    s.cpu = pct(cur.cpu_busy.saturating_sub(prev.cpu_busy), total);
    let per_sec = |a: u64, b: u64| if secs > 0.0 { (a.saturating_sub(b) as f64 / secs) as u64 } else { 0 };
    s.net_rx = per_sec(cur.net_rx, prev.net_rx);
    s.net_tx = per_sec(cur.net_tx, prev.net_tx);
    if let (false, Some((before, base_total))) = (cur.procs.is_empty(), base) {
        let total = cur.cpu_total.saturating_sub(*base_total);
        let mut used: Vec<(u32, &str, u64, u64)> = cur
            .procs
            .iter()
            .filter_map(|(pid, name, t, mem)| {
                let then = before.iter().find(|p| p.0 == *pid)?;
                Some((*pid, name.as_str(), t.saturating_sub(then.2), *mem))
            })
            .collect();
        if let Some(own) = used.iter().find(|p| p.0 == own_pid) {
            s.own_cpu = Some(pct(own.2, total));
            s.own_mem = Some(own.3);
        }
        // The idle process and Coucou itself are not "apps using the machine".
        used.retain(|p| p.0 != 0 && p.0 != own_pid);
        used.sort_by_key(|p| std::cmp::Reverse((p.2, p.3)));
        s.top = used.iter().take(TOP).map(|p| Proc { name: display_name(p.1), cpu: pct(p.2, total), mem: p.3 }).collect();
    }
    s
}

/// "chrome.exe" → "chrome"; names are shown, never used for anything else.
pub fn display_name(exe: &str) -> String {
    let name = exe.rsplit(['\\', '/']).next().unwrap_or(exe);
    name.strip_suffix(".exe").or_else(|| name.strip_suffix(".EXE")).unwrap_or(name).to_string()
}

/// The last reading, kept between two calls of the panel.
#[derive(Default)]
pub struct Sampler {
    last: Mutex<Option<(Raw, Instant)>>,
    procs: Mutex<Option<ProcBase>>,
    #[cfg(windows)]
    gpu: Mutex<Option<super::win::GpuQuery>>,
}

impl Sampler {
    /// One sample; `with_top` lists the processes too (the panel asks every
    /// other second: listing them is the costly part).
    pub fn sample(&self, with_top: bool) -> Sample {
        let now = Instant::now();
        #[cfg(windows)]
        let cur = {
            let mut gpu = self.gpu.lock().unwrap();
            super::win::read_stats(with_top, &mut gpu)
        };
        #[cfg(not(windows))]
        let cur = linux::read(with_top);
        let mut last = self.last.lock().unwrap();
        let mut base = self.procs.lock().unwrap();
        let secs = last.as_ref().map(|(_, at)| now.duration_since(*at).as_secs_f64()).unwrap_or(0.0);
        let sample = compute(last.as_ref().map(|(r, _)| r), &cur, secs, std::process::id(), base.as_ref());
        let mut cur = cur;
        if !cur.procs.is_empty() {
            *base = Some((std::mem::take(&mut cur.procs), cur.cpu_total));
        }
        *last = Some((cur, now));
        sample
    }

    /// The panel closed: forget the base, so a reopened panel starts clean.
    pub fn reset(&self) {
        *self.last.lock().unwrap() = None;
        *self.procs.lock().unwrap() = None;
    }
}

#[cfg(not(windows))]
mod linux {
    use super::Raw;

    pub fn read(with_top: bool) -> Raw {
        let mut r = Raw::default();
        if let Ok(stat) = std::fs::read_to_string("/proc/stat") {
            if let Some((busy, total)) = parse_cpu(&stat) {
                r.cpu_busy = busy;
                r.cpu_total = total;
            }
        }
        if let Ok(mem) = std::fs::read_to_string("/proc/meminfo") {
            if let Some((used, total)) = parse_meminfo(&mem) {
                r.mem_used = used;
                r.mem_total = total;
            }
        }
        if let Ok(net) = std::fs::read_to_string("/proc/net/dev") {
            (r.net_rx, r.net_tx) = parse_net(&net);
        }
        if with_top {
            r.procs = processes();
        }
        r
    }

    /// (busy, total) jiffies from the first line of /proc/stat.
    pub fn parse_cpu(stat: &str) -> Option<(u64, u64)> {
        let line = stat.lines().find(|l| l.starts_with("cpu "))?;
        let v: Vec<u64> = line.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
        if v.len() < 4 {
            return None;
        }
        // user nice system idle iowait irq softirq steal: guest is already in user.
        let total: u64 = v.iter().take(8).sum();
        let idle = v[3] + v.get(4).copied().unwrap_or(0);
        Some((total.saturating_sub(idle), total))
    }

    /// (used, total) bytes from /proc/meminfo.
    pub fn parse_meminfo(mem: &str) -> Option<(u64, u64)> {
        let field = |name: &str| {
            mem.lines()
                .find(|l| l.starts_with(name))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<u64>().ok())
                .map(|kb| kb * 1024)
        };
        let total = field("MemTotal:")?;
        let available = field("MemAvailable:")?;
        Some((total.saturating_sub(available), total))
    }

    /// Received and sent bytes, every interface but the loopback.
    pub fn parse_net(net: &str) -> (u64, u64) {
        let mut rx = 0;
        let mut tx = 0;
        for line in net.lines().skip(2) {
            let Some((name, rest)) = line.split_once(':') else { continue };
            if name.trim() == "lo" {
                continue;
            }
            let v: Vec<u64> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            if v.len() >= 9 {
                rx += v[0];
                tx += v[8];
            }
        }
        (rx, tx)
    }

    /// (pid, name, utime + stime, rss bytes) from /proc/<pid>/stat.
    pub fn parse_proc_stat(pid: u32, stat: &str, page: u64) -> Option<(u32, String, u64, u64)> {
        let open = stat.find('(')?;
        let close = stat.rfind(')')?;
        let name = stat.get(open + 1..close)?.to_string();
        let rest: Vec<&str> = stat.get(close + 2..)?.split_whitespace().collect();
        // After the name: state is field 3, utime 14, stime 15, rss 24.
        let utime: u64 = rest.get(11)?.parse().ok()?;
        let stime: u64 = rest.get(12)?.parse().ok()?;
        let rss: u64 = rest.get(21)?.parse().ok()?;
        Some((pid, name, utime + stime, rss * page))
    }

    fn processes() -> Vec<(u32, String, u64, u64)> {
        let page = 4096;
        let Ok(dir) = std::fs::read_dir("/proc") else { return Vec::new() };
        dir.flatten()
            .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
            .filter_map(|pid| parse_proc_stat(pid, &std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?, page))
            .collect()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn proc_files_are_read_as_the_kernel_writes_them() {
            let stat = "cpu  100 5 50 800 20 3 2 0 0 0\ncpu0 50 2 25 400 10 1 1 0 0 0\n";
            assert_eq!(parse_cpu(stat), Some((160, 980)));
            let mem = "MemTotal:       32000000 kB\nMemFree:  1000 kB\nMemAvailable:   12000000 kB\n";
            assert_eq!(parse_meminfo(mem), Some((20_000_000 * 1024, 32_000_000 * 1024)));
            let net = "Inter-|   Receive\n face |bytes packets\n    lo: 500 5 0 0 0 0 0 0 500 5 0 0 0 0 0 0\n  eth0: 1000 10 0 0 0 0 0 0 2000 20 0 0 0 0 0 0\n wlan0: 300 3 0 0 0 0 0 0 100 1 0 0 0 0 0 0\n";
            assert_eq!(parse_net(net), (1300, 2100));
            let p = "4242 (Web Content (1)) S 1 2 3 4 5 6 7 8 9 10 120 30 0 0 20 0 8 0 100 1000000 2500 18446744073709551615";
            assert_eq!(parse_proc_stat(4242, p, 4096), Some((4242, "Web Content (1)".into(), 150, 2500 * 4096)));
            assert_eq!(parse_proc_stat(1, "garbage", 4096), None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(busy: u64, total: u64, rx: u64, procs: Vec<(u32, &str, u64, u64)>) -> Raw {
        Raw {
            cpu_busy: busy,
            cpu_total: total,
            mem_used: 8,
            mem_total: 16,
            net_rx: rx,
            net_tx: rx / 2,
            gpu: Some(12.5),
            procs: procs.into_iter().map(|(p, n, t, m)| (p, n.to_string(), t, m)).collect(),
        }
    }

    #[test]
    fn a_first_reading_gives_memory_only() {
        let s = compute(None, &raw(10, 100, 1000, vec![]), 0.0, 1, None);
        assert_eq!((s.cpu, s.net_rx, s.mem_used, s.gpu), (0.0, 0, 8, Some(12.5)));
        assert!(s.top.is_empty());
    }

    #[test]
    fn shares_rates_and_the_top_five_come_from_the_differences() {
        let before = raw(100, 1000, 10_000, vec![(0, "Idle", 0, 0), (7, "self", 10, 50), (10, "a.exe", 0, 1), (11, "b", 0, 2), (12, "c", 0, 3), (13, "d", 0, 4), (14, "e", 0, 5), (15, "f", 0, 6)]);
        let after = raw(400, 2000, 14_000, vec![(0, "Idle", 900, 0), (7, "self", 20, 60), (10, "a.exe", 300, 1), (11, "b", 100, 2), (12, "c", 50, 3), (13, "d", 40, 4), (14, "e", 30, 5), (15, "f", 20, 6), (99, "new", 999, 7)]);
        let base: ProcBase = (before.procs.clone(), before.cpu_total);
        let s = compute(Some(&before), &after, 2.0, 7, Some(&base));
        assert_eq!(s.cpu, 30.0);
        assert_eq!((s.net_rx, s.net_tx), (2000, 1000));
        let names: Vec<&str> = s.top.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["a", "b", "c", "d", "e"], "idle, Coucou and a process born meanwhile are left out");
        assert_eq!(s.top[0].cpu, 30.0);
        assert_eq!((s.own_cpu, s.own_mem), (Some(1.0), Some(60)));
    }

    #[test]
    fn counters_that_go_back_never_underflow() {
        let s = compute(Some(&raw(500, 2000, 9000, vec![])), &raw(100, 1000, 10, vec![]), 1.0, 1, None);
        assert_eq!((s.cpu, s.net_rx), (0.0, 0));
    }

    #[test]
    fn processes_listed_every_other_second_are_measured_over_their_own_span() {
        // Listed at t0 (total 1000), not at t1 (total 2000), listed at t2 (total 3000).
        let base: ProcBase = (vec![(10, "a".into(), 0, 1)], 1000);
        let t1 = raw(0, 2000, 0, vec![]);
        let t2 = raw(0, 3000, 0, vec![(10, "a", 1000, 1)]);
        let s = compute(Some(&t1), &t2, 1.0, 1, Some(&base));
        assert_eq!(s.top[0].cpu, 50.0, "1000 over the 2000 since the listing, not the 1000 since t1");
    }

    /// Reads this machine for real (Windows on the Windows CI, /proc here):
    /// two samples, the second with processes and rates.
    #[test]
    fn a_real_sample_reads_this_machine() {
        let sampler = Sampler::default();
        let first = sampler.sample(true);
        assert!(first.mem_total > 0 && first.mem_used <= first.mem_total, "{first:?}");
        std::thread::sleep(std::time::Duration::from_millis(300));
        let second = sampler.sample(true);
        assert!(second.cpu >= 0.0 && second.cpu <= 100.0);
        assert!(!second.top.is_empty(), "no process listed");
        assert!(second.top.iter().all(|p| p.cpu >= 0.0 && p.cpu <= 100.0));
        sampler.reset();
        assert!(sampler.sample(false).top.is_empty());
    }

    #[test]
    fn names_lose_their_path_and_exe() {
        assert_eq!(display_name("C:\\Program Files\\App\\chrome.exe"), "chrome");
        assert_eq!(display_name("Code.EXE"), "Code");
        assert_eq!(display_name("firefox"), "firefox");
    }
}
