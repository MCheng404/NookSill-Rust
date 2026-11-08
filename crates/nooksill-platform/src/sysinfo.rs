//! System resource sampling: CPU load, memory usage, disk usage.
//!
//! `GetSystemTimes` reports cumulative counters, so CPU load is the delta of
//! two samples. Every call here is read-only.

use windows::Win32::Foundation::FILETIME;
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;

/// One sample.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct SysInfo {
    /// CPU load percent, 0..=100.
    pub cpu_percent: u8,
    /// Memory usage percent, 0..=100.
    pub mem_percent: u8,
    /// Disk usage percent, 0..=100.
    pub disk_percent: u8,
    /// Used memory in MB.
    pub mem_used_mb: u64,
    /// Total memory in MB.
    pub mem_total_mb: u64,
    /// Free disk space in GB.
    pub disk_free_gb: u64,
    /// Total disk size in GB.
    pub disk_total_gb: u64,
}

/// CPU load calculator.
///
/// `GetSystemTimes` returns cumulative counters, so two samples are needed. The
/// previous one is held here as explicit state instead of three globals.
#[derive(Copy, Clone, Debug, Default)]
pub struct CpuSampler {
    /// Previous cumulative idle / kernel / user, in 100 ns units.
    prev: Option<(u64, u64, u64)>,
}

impl CpuSampler {
    #[must_use]
    pub const fn new() -> Self {
        Self { prev: None }
    }

    /// Take one sample and return the CPU load percent.
    ///
    /// The first call returns 0: without a previous baseline there is no delta.
    pub fn sample(&mut self) -> u8 {
        let Some((idle, kernel, user)) = system_times() else {
            return 0;
        };
        let Some((p_idle, p_kernel, p_user)) = self.prev else {
            self.prev = Some((idle, kernel, user));
            return 0;
        };
        self.prev = Some((idle, kernel, user));

        // Counters only grow; a clock rollback or reset makes the delta huge.
        // Reporting 0 beats emitting an absurd percentage.
        let Some(d_idle) = idle.checked_sub(p_idle) else {
            return 0;
        };
        let Some(d_kernel) = kernel.checked_sub(p_kernel) else {
            return 0;
        };
        let Some(d_user) = user.checked_sub(p_user) else {
            return 0;
        };
        // The denominator is kernel + user, and kernel already includes idle.
        // Keeping full kernel in the sum matters: dropping idle makes the load
        // read too high.
        let total = d_kernel + d_user;
        if total == 0 {
            return 0;
        }
        let busy = total.saturating_sub(d_idle);
        let pct = busy.saturating_mul(100) / total;
        u8::try_from(pct.min(100)).unwrap_or(100)
    }
}

/// Read `GetSystemTimes` as cumulative (idle, kernel, user) in 100 ns units.
fn system_times() -> Option<(u64, u64, u64)> {
    let mut idle = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.ok()?;
    Some((
        filetime_value(idle),
        filetime_value(kernel),
        filetime_value(user),
    ))
}

/// Combine a FILETIME's two 32-bit halves into one 100 ns count.
fn filetime_value(ft: FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64
}

/// Sample memory and disk.
///
/// CPU is excluded because it needs two samples; see [`CpuSampler`].
pub fn sample_memory_disk() -> SysInfo {
    let mut out = SysInfo::default();

    let mut ms = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    if unsafe { GlobalMemoryStatusEx(&mut ms) }.is_ok() {
        out.mem_percent = ms.dwMemoryLoad.min(100) as u8;
        out.mem_total_mb = ms.ullTotalPhys / (1024 * 1024);
        out.mem_used_mb = ms.ullTotalPhys.saturating_sub(ms.ullAvailPhys) / (1024 * 1024);
    }

    // Always the C: drive.
    let mut free_to_caller = 0_u64;
    let mut total = 0_u64;
    let mut total_free = 0_u64;
    let path: Vec<u16> = "C:\\".encode_utf16().chain(std::iter::once(0)).collect();
    if unsafe {
        GetDiskFreeSpaceExW(
            windows::core::PCWSTR(path.as_ptr()),
            Some(&mut free_to_caller),
            Some(&mut total),
            Some(&mut total_free),
        )
    }
    .is_ok()
    {
        const GB: u64 = 1024 * 1024 * 1024;
        out.disk_free_gb = free_to_caller / GB;
        out.disk_total_gb = total / GB;
        out.disk_percent = if total > 0 {
            let used = total.saturating_sub(free_to_caller);
            u8::try_from(used.saturating_mul(100) / total).unwrap_or(100)
        } else {
            0
        };
    }

    out
}

/// Full sample: CPU, memory and disk.
pub fn sample(cpu: &mut CpuSampler) -> SysInfo {
    let mut out = sample_memory_disk();
    out.cpu_percent = cpu.sample();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_sampler_first_call_is_zero() {
        let mut s = CpuSampler::new();
        // No baseline on the first call, so no load can be computed.
        assert_eq!(s.sample(), 0);
    }

    #[test]
    fn cpu_percent_stays_in_range() {
        let mut s = CpuSampler::new();
        for _ in 0..5 {
            let v = s.sample();
            assert!(v <= 100, "占用率越界：{v}");
        }
    }

    #[test]
    fn memory_report_is_plausible() {
        let info = sample_memory_disk();
        assert!(info.mem_percent <= 100);
        assert!(info.mem_total_mb > 0, "应当读出总内存");
        assert!(info.mem_used_mb <= info.mem_total_mb);
    }

    #[test]
    fn disk_report_is_plausible() {
        let info = sample_memory_disk();
        assert!(info.disk_percent <= 100);
        assert!(info.disk_total_gb > 0, "应当读出磁盘容量");
        assert!(info.disk_free_gb <= info.disk_total_gb);
    }
}
