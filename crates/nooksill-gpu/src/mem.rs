//! In-process memory sampling.
//!
//! `working_set` is resident physical pages (Task Manager's "Memory"),
//! `private` also counts pages swapped to the page file, and `peak_working_set`
//! catches transient spikes that external polling would miss.

use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::Threading::GetCurrentProcess;

/// One memory sample.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemStats {
    /// Current working set in bytes.
    pub working_set: usize,
    /// Peak working set in bytes.
    pub peak_working_set: usize,
    /// Private commit in bytes.
    pub private: usize,
}

impl MemStats {
    /// Read this process's memory counters. Returns zeros on failure: a sampling
    /// error must not crash the process being measured.
    #[must_use]
    pub fn now() -> Self {
        let mut counters = PROCESS_MEMORY_COUNTERS_EX {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            ..Default::default()
        };
        let queried = unsafe {
            GetProcessMemoryInfo(GetCurrentProcess(), (&raw mut counters).cast(), counters.cb)
        };
        if queried.is_err() {
            return Self::default();
        }
        Self {
            working_set: counters.WorkingSetSize,
            peak_working_set: counters.PeakWorkingSetSize,
            private: counters.PrivateUsage,
        }
    }

    #[must_use]
    pub fn ws_mb(self) -> f32 {
        mb(self.working_set)
    }

    #[must_use]
    pub fn peak_ws_mb(self) -> f32 {
        mb(self.peak_working_set)
    }

    #[must_use]
    pub fn private_mb(self) -> f32 {
        mb(self.private)
    }

    /// Single-line summary shared by logs and probe output.
    #[must_use]
    pub fn summary(self) -> String {
        nooksill_core::i18n::fmt(
            "WS {0} MB / peak {1} MB / private {2} MB",
            &[
                format!("{:.1}", self.ws_mb()).as_str(),
                format!("{:.1}", self.peak_ws_mb()).as_str(),
                format!("{:.1}", self.private_mb()).as_str(),
            ],
        )
    }
}

fn mb(bytes: usize) -> f32 {
    bytes as f32 / (1024.0 * 1024.0)
}

/// Difference between two samples, i.e. what one step cost.
#[derive(Clone, Copy, Debug, Default)]
pub struct Delta {
    pub ws_mb: f32,
    pub private_mb: f32,
}

impl Delta {
    #[must_use]
    pub fn between(before: MemStats, after: MemStats) -> Self {
        Self {
            ws_mb: after.ws_mb() - before.ws_mb(),
            private_mb: after.private_mb() - before.private_mb(),
        }
    }

    #[must_use]
    pub fn summary(self) -> String {
        nooksill_core::i18n::fmt(
            "+{0} MB WS / +{1} MB private",
            &[
                format!("{:.1}", self.ws_mb).as_str(),
                format!("{:.1}", self.private_mb).as_str(),
            ],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::MemStats;

    #[test]
    fn 采样到的数值大于零() {
        // Any live process has a working set; zero means the query path is broken.
        let s = MemStats::now();
        assert!(s.working_set > 0, "工作集为 0，查询失败: {s:?}");
        assert!(s.peak_working_set >= s.working_set);
    }

    #[test]
    fn 摘要包含三个口径() {
        let s = MemStats::now();
        let text = s.summary();
        let lower = text.to_lowercase();
        assert!(lower.contains("ws"));
        assert!(lower.contains(&nooksill_core::i18n::t("Peak").to_lowercase()));
        assert!(lower.contains(&nooksill_core::i18n::t("Private").to_lowercase()));
    }
}
