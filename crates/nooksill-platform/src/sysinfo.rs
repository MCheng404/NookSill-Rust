//! System resource sampling: CPU, memory, disk, network, processes, uptime.
//!
//! Counters from `GetSystemTimes` and `GetIfTable2` are cumulative, so load and
//! throughput are deltas of two samples. Every call here is read-only.

use std::time::Instant;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetIfTable2, MIB_IF_TABLE2, MIB_IF_TYPE_LOOPBACK,
};
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32First, Process32Next, PROCESSENTRY32, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
    PDH_MORE_DATA,
};
use windows::Win32::System::SystemInformation::{
    GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX,
};
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
    /// Download rate in KiB/s, from all non-loopback interfaces.
    pub net_rx_kbps: u32,
    /// Upload rate in KiB/s.
    pub net_tx_kbps: u32,
    /// Number of processes in the system snapshot.
    pub process_count: u32,
    /// Seconds since the last system boot.
    pub uptime_secs: u64,
    /// Busiest GPU engine utilization percent; `None` without PDH counters.
    pub gpu_percent: Option<u8>,
    /// CPU temperature in Celsius; `None` when ACPI exposes no usable zone.
    pub cpu_temp_c: Option<u8>,
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

/// Network throughput calculator; the interface table is sampled on each call.
#[derive(Copy, Clone, Debug, Default)]
pub struct NetSampler {
    prev: Option<(u64, u64, Instant)>,
}

impl NetSampler {
    #[must_use]
    pub const fn new() -> Self {
        Self { prev: None }
    }

    /// Returns `(download, upload)` in KiB/s; the first call has no baseline.
    pub fn sample(&mut self) -> (u32, u32) {
        let Some((rx, tx)) = if_counters() else {
            return (0, 0);
        };
        let now = Instant::now();
        let Some((p_rx, p_tx, prev_at)) = self.prev else {
            self.prev = Some((rx, tx, now));
            return (0, 0);
        };
        self.prev = Some((rx, tx, now));
        let Some(elapsed) = now.checked_duration_since(prev_at) else {
            return (0, 0);
        };
        let secs = elapsed.as_secs_f64();
        (rate(rx, p_rx, secs), rate(tx, p_tx, secs))
    }
}

/// PDH query for GPU engine utilization and ACPI thermal zones.
///
/// The counters are English names (`PdhAddEnglishCounterW`), so a localized
/// Windows install keeps working. Both counters are optional: a driver may not
/// expose GPU Engine, and many boards expose no thermal zone at all.
pub struct PdhSampler {
    query: PDH_HQUERY,
    gpu: Option<PDH_HCOUNTER>,
    temp: Option<PDH_HCOUNTER>,
}

impl PdhSampler {
    #[must_use]
    pub fn new() -> Self {
        let mut query = PDH_HQUERY::default();
        // SAFETY: the out-pointer is valid for the call.
        if unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut query) } != 0 {
            return Self {
                query: PDH_HQUERY::default(),
                gpu: None,
                temp: None,
            };
        }
        let gpu = add_counter(query, "\\GPU Engine(*)\\Utilization Percentage");
        let temp = add_counter(query, "\\Thermal Zone Information(*)\\Temperature");
        // The first collect only establishes the baseline for rate counters.
        // SAFETY: `query` came from `PdhOpenQueryW`.
        unsafe {
            PdhCollectQueryData(query);
        }
        Self { query, gpu, temp }
    }

    /// `(gpu percent, cpu temperature)`; either may be `None`.
    pub fn sample(&mut self) -> (Option<u8>, Option<u8>) {
        if self.query.0.is_null() {
            return (None, None);
        }
        // SAFETY: `query` is live until `Drop`.
        if unsafe { PdhCollectQueryData(self.query) } != 0 {
            return (None, None);
        }
        let gpu = self
            .gpu
            .and_then(max_value)
            .map(|v| v.clamp(0.0, 100.0).round() as u8);
        let temp = self.temp.and_then(max_value).and_then(to_celsius);
        (gpu, temp)
    }
}

impl std::fmt::Debug for PdhSampler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PdhSampler")
            .field("gpu", &self.gpu.is_some())
            .field("temp", &self.temp.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for PdhSampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PdhSampler {
    fn drop(&mut self) {
        if !self.query.0.is_null() {
            // SAFETY: the query is closed exactly once, here.
            unsafe {
                PdhCloseQuery(self.query);
            }
        }
    }
}

fn add_counter(query: PDH_HQUERY, path: &str) -> Option<PDH_HCOUNTER> {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let mut counter = PDH_HCOUNTER::default();
    // SAFETY: the path is NUL-terminated and outlives the call.
    let rc = unsafe { PdhAddEnglishCounterW(query, PCWSTR(wide.as_ptr()), 0, &mut counter) };
    (rc == 0).then_some(counter)
}

/// Highest valid value across all instances of a wildcard counter.
///
/// GPU Engine instances are per process and per engine; summing them is not a
/// total (Microsoft's own answer says so), so the busiest engine is reported.
fn max_value(counter: PDH_HCOUNTER) -> Option<f64> {
    let (mut size, mut count) = (0_u32, 0_u32);
    // SAFETY: a size query with no buffer is the documented first step.
    let rc = unsafe {
        PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, None)
    };
    if rc != PDH_MORE_DATA || count == 0 {
        return None;
    }
    // `size` counts bytes and includes the instance-name strings that `szName`
    // points into; a `count`-sized item array would be too small and corrupt
    // the heap. Round up to 8-byte words so the item structs stay aligned.
    let words = (size as usize).div_ceil(8).max(1);
    let mut buffer = vec![0_u64; words];
    let base = buffer.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
    // SAFETY: the buffer holds at least `size` bytes, which is what PDH asked for.
    let rc = unsafe {
        PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, Some(base))
    };
    if rc != 0 {
        return None;
    }
    // SAFETY: PDH filled `count` consecutive items at `base`.
    let items = unsafe { std::slice::from_raw_parts(base, count as usize) };
    items
        .iter()
        .filter(|item| item.FmtValue.CStatus == 0)
        // SAFETY: PDH_FMT_DOUBLE makes the double member the active one.
        .map(|item| unsafe { item.FmtValue.Anonymous.doubleValue })
        .filter(|v| v.is_finite())
        .reduce(f64::max)
}

/// Normalize a thermal value: Kelvin, 0.1 K or Celsius depending on firmware.
fn to_celsius(value: f64) -> Option<u8> {
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    let celsius = if value > 1000.0 {
        value / 10.0 - 273.15
    } else if value > 200.0 {
        value - 273.15
    } else {
        value
    };
    // Plausible package temperatures only; placeholder values are rejected.
    (20.0..=110.0)
        .contains(&celsius)
        .then(|| celsius.round() as u8)
}

/// Bytes per second to KiB/s; a counter reset or a zero interval reads zero.
fn rate(current: u64, previous: u64, secs: f64) -> u32 {
    if secs <= 0.0 || current <= previous {
        return 0;
    }
    let kbps = (current - previous) as f64 / secs / 1024.0;
    u32::try_from(kbps.round() as u64).unwrap_or(u32::MAX)
}

/// Sum interface byte counters, excluding loopback.
///
/// Virtual adapters count too: the panel reports network activity, not a
/// per-adapter breakdown.
fn if_counters() -> Option<(u64, u64)> {
    let mut raw = std::ptr::null_mut();
    // SAFETY: the out-pointer is valid; the buffer is freed by the guard below.
    if unsafe { GetIfTable2(&mut raw) }.is_err() || raw.is_null() {
        return None;
    }
    struct Table(*mut MIB_IF_TABLE2);
    impl Drop for Table {
        fn drop(&mut self) {
            // SAFETY: this pointer came from `GetIfTable2` and is freed once.
            unsafe { FreeMibTable(self.0 as *const core::ffi::c_void) }
        }
    }
    let guard = Table(raw);
    // SAFETY: `GetIfTable2` returns `NumEntries` initialized rows.
    let table = unsafe { &*guard.0 };
    let rows = table.Table.as_ptr();
    let (mut rx, mut tx) = (0_u64, 0_u64);
    for i in 0..table.NumEntries as usize {
        // SAFETY: `i < NumEntries`, inside the allocated row array.
        let row = unsafe { &*rows.add(i) };
        if row.Type == MIB_IF_TYPE_LOOPBACK {
            continue;
        }
        rx = rx.saturating_add(row.InOctets);
        tx = tx.saturating_add(row.OutOctets);
    }
    Some((rx, tx))
}

/// Number of processes from a Toolhelp snapshot; 0 when the snapshot fails.
#[must_use]
pub fn process_count() -> u32 {
    // SAFETY: no process id means "all processes".
    let Ok(snap) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return 0;
    };
    struct Snapshot(HANDLE);
    impl Drop for Snapshot {
        fn drop(&mut self) {
            // SAFETY: `snap` was returned by `CreateToolhelp32Snapshot`.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
    let guard = Snapshot(snap);
    let mut entry = PROCESSENTRY32 {
        dwSize: std::mem::size_of::<PROCESSENTRY32>() as u32,
        ..Default::default()
    };
    // SAFETY: the entry is initialized and the snapshot handle is live.
    if unsafe { Process32First(guard.0, &mut entry) }.is_err() {
        return 0;
    }
    let mut count = 1_u32;
    // SAFETY: same entry, same live handle; the API advances it in place.
    while unsafe { Process32Next(guard.0, &mut entry) }.is_ok() {
        count = count.saturating_add(1);
    }
    count
}

/// Seconds since system boot.
#[must_use]
pub fn uptime_secs() -> u64 {
    // SAFETY: no preconditions.
    (unsafe { GetTickCount64() }) / 1000
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

/// Full sample: every resource the panel can show.
pub fn sample_all(cpu: &mut CpuSampler, net: &mut NetSampler, pdh: &mut PdhSampler) -> SysInfo {
    let mut out = sample_memory_disk();
    out.cpu_percent = cpu.sample();
    let (rx, tx) = net.sample();
    out.net_rx_kbps = rx;
    out.net_tx_kbps = tx;
    out.process_count = process_count();
    out.uptime_secs = uptime_secs();
    let (gpu, temp) = pdh.sample();
    out.gpu_percent = gpu;
    out.cpu_temp_c = temp;
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

    #[test]
    fn rate_handles_zeros_resets_and_normal_traffic() {
        assert_eq!(rate(2048, 2048, 1.0), 0);
        assert_eq!(rate(100, 200, 1.0), 0, "计数器回绕不该产生巨大速率");
        assert_eq!(rate(2048, 0, 0.0), 0);
        assert_eq!(rate(1024 * 1024, 0, 1.0), 1024);
    }

    #[test]
    fn network_sampler_first_call_has_no_rate() {
        assert_eq!(NetSampler::new().sample(), (0, 0));
    }

    #[test]
    fn process_count_and_uptime_are_plausible() {
        assert!(process_count() > 0, "应当能枚举进程");
        assert!(uptime_secs() > 0, "应当能读出开机时长");
    }

    #[test]
    fn thermal_values_are_normalized_and_filtered() {
        assert_eq!(to_celsius(350.15), Some(77), "开尔文");
        assert_eq!(to_celsius(3501.5), Some(77), "0.1 开尔文");
        assert_eq!(to_celsius(55.0), Some(55), "摄氏度");
        assert_eq!(to_celsius(0.0), None);
        assert_eq!(to_celsius(1270.0), None, "换算后为负值，应丢弃");
        assert_eq!(to_celsius(555.0), None, "超出合理区间，应丢弃");
    }

    #[test]
    fn pdh_sampler_is_optional_and_never_panics() {
        let mut sampler = PdhSampler::new();
        let (gpu, temp) = sampler.sample();
        if let Some(v) = gpu {
            assert!(v <= 100);
        }
        if let Some(v) = temp {
            assert!((20..=110).contains(&v));
        }
    }
}
