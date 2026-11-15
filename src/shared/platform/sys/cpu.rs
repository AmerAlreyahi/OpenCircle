//! How busy the PC is with OTHER work (every core, every process but ours):
//! live translate backs off its slow work while a game needs the CPU. Our own
//! work is left out: it runs below normal priority and yields anyway, and
//! counting it would pause us for our own load (on the Windows VM a detect
//! burst alone took the PC over 85 %, so discovery stopped for seconds).

/// Busy and total CPU time of the whole system so far, in the OS's units
/// (Windows 100 ns; Linux clock ticks), summed over the cores.
fn system() -> Option<(u64, u64)> {
    #[cfg(windows)]
    {
        let (mut idle, mut kernel, mut user) = Default::default();
        // SAFETY: writes the three FILETIMEs we own.
        unsafe {
            windows::Win32::System::Threading::GetSystemTimes(
                Some(&mut idle),
                Some(&mut kernel),
                Some(&mut user),
            )
        }
        .ok()?;
        // (kernel time includes the idle time)
        let total = ft(kernel) + ft(user);
        Some((total.saturating_sub(ft(idle)), total))
    }
    #[cfg(not(windows))]
    {
        let stat = std::fs::read_to_string("/proc/stat").ok()?;
        parse_stat(stat.lines().next()?)
    }
}

#[cfg(windows)]
fn ft(f: windows::Win32::Foundation::FILETIME) -> u64 {
    (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime)
}

/// A process's CPU time so far (`None`: this one), in the same units.
fn process(pid: Option<u32>) -> Option<u64> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::{CloseHandle, FILETIME};
        use windows::Win32::System::Threading::{
            GetCurrentProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        let (mut a, mut b, mut kernel, mut user): (FILETIME, FILETIME, FILETIME, FILETIME) =
            Default::default();
        // SAFETY: a handle we open (closed below) or the pseudo handle; the call
        // writes the four FILETIMEs we own.
        unsafe {
            let (h, opened) = match pid {
                None => (GetCurrentProcess(), false),
                Some(p) => (
                    OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, p).ok()?,
                    true,
                ),
            };
            let ok = GetProcessTimes(h, &mut a, &mut b, &mut kernel, &mut user).is_ok();
            if opened {
                let _ = CloseHandle(h);
            }
            ok.then(|| ft(kernel) + ft(user))
        }
    }
    #[cfg(not(windows))]
    {
        let who = pid.map_or("self".to_string(), |p| p.to_string());
        let stat = std::fs::read_to_string(format!("/proc/{who}/stat")).ok()?;
        parse_pid_stat(&stat)
    }
}

/// /proc/stat's "cpu" line -> (busy, total) ticks (idle + iowait = not busy).
#[cfg(any(not(windows), test))]
fn parse_stat(line: &str) -> Option<(u64, u64)> {
    let mut f = line.split_whitespace();
    (f.next()? == "cpu").then_some(())?;
    let n: Vec<u64> = f.map_while(|v| v.parse().ok()).take(8).collect();
    let total: u64 = n.iter().sum();
    let idle = n.get(3)? + n.get(4).copied().unwrap_or(0);
    Some((total - idle, total))
}

/// /proc/<pid>/stat -> utime + stime ticks (fields 14, 15; the name in
/// parentheses may hold spaces).
#[cfg(any(not(windows), test))]
fn parse_pid_stat(stat: &str) -> Option<u64> {
    let rest = &stat[stat.rfind(')')? + 1..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    Some(f.get(11)?.parse::<u64>().ok()? + f.get(12)?.parse::<u64>().ok()?)
}

/// Samples the others' load since the last sample.
#[derive(Default)]
pub struct Load {
    /// System busy, system total, ours (this process + `helpers`), and
    /// which helpers those were.
    last: Option<(u64, u64, u64, Vec<u32>)>,
}

impl Load {
    /// The share of all CPU time that OTHER processes used since the last
    /// call (0..1), this process and `helpers` (our OCR / translation
    /// processes) left out; none the first time or where the OS can't say.
    pub fn sample(&mut self, helpers: &[u32]) -> Option<f32> {
        let (busy, total) = system()?;
        let theirs: u64 = (helpers.iter()).filter_map(|&p| process(Some(p))).sum();
        let ours = process(None)? + theirs;
        let last = self.last.replace((busy, total, ours, helpers.to_vec()))?;
        // (another helper since: its time so far isn't in the last sample)
        let ours_then = if last.3 == helpers { last.2 } else { ours };
        let dt = total.saturating_sub(last.1);
        let others = busy
            .saturating_sub(last.0)
            .saturating_sub(ours.saturating_sub(ours_then));
        (dt > 0).then(|| (others as f32 / dt as f32).min(1.0))
    }
}

#[test]
fn reads_the_cpu_lines_of_proc() {
    // user nice system idle iowait irq softirq steal guest guest_nice
    let line = "cpu  100 5 50 800 40 3 2 0 0 0";
    assert_eq!(parse_stat(line), Some((160, 1000)));
    assert_eq!(parse_stat("cpu0 1 2 3 4"), None, "one core's line");
    let pid = "4242 (open circle) S 1 2 3 4 5 6 7 8 9 10 30 12 0 0 20 0 9";
    assert_eq!(parse_pid_stat(pid), Some(42));
    let mut load = Load::default();
    assert_eq!(load.sample(&[]), None, "nothing to compare with yet");
    #[cfg(any(windows, target_os = "linux"))]
    {
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert!(load.sample(&[]).is_some_and(|l| (0.0..=1.0).contains(&l)));
    }
}

/// Every helper of ours is left out (live translate: the OCR helper AND the
/// graphics card's translator): a busy one never reads as a busy PC.
#[cfg(target_os = "linux")]
#[test]
fn every_helper_of_ours_is_left_out() {
    let mut busy = std::process::Command::new("sh")
        .args(["-c", "while :; do :; done"])
        .spawn()
        .unwrap();
    let helpers = [busy.id()];
    let (mut theirs, mut ours) = (Load::default(), Load::default());
    let _ = (theirs.sample(&[]), ours.sample(&helpers));
    std::thread::sleep(std::time::Duration::from_millis(400));
    let (with, without) = (theirs.sample(&[]).unwrap(), ours.sample(&helpers).unwrap());
    let _ = busy.kill();
    let _ = busy.wait();
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get()) as f32;
    assert!(with - without > 0.5 / cores, "{with} vs {without}");
}
