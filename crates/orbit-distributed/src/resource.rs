//! Agent system resource sampling (sysinfo): CPU / memory / load

use sysinfo::System;

use crate::types::ResourceSnapshot;

pub struct ResourceCollector {
    sys: System,
}

impl Default for ResourceCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceCollector {
    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_usage();
        Self { sys }
    }

    /// Sample system resources once. Note: CPU usage needs an interval between two samples,
    /// so the caller may sample every 1~2s; a short internal refresh here keeps the first sample valid.
    pub fn sample(&mut self) -> ResourceSnapshot {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        std::thread::sleep(std::time::Duration::from_millis(120));
        self.sys.refresh_cpu_usage();

        let cpu_percent = self.sys.global_cpu_usage() as f64;
        let mem_total = self.sys.total_memory() as f64 / (1024.0 * 1024.0);
        let mem_available = self.sys.available_memory() as f64 / (1024.0 * 1024.0);
        let mem_used = (mem_total - mem_available).max(0.0);

        ResourceSnapshot {
            cpu_percent: (cpu_percent * 10.0).round() / 10.0,
            mem_used_mb: mem_used.round(),
            mem_total_mb: mem_total.round(),
            mem_percent: if mem_total > 0.0 {
                ((mem_used / mem_total * 100.0) * 10.0).round() / 10.0
            } else {
                0.0
            },
            load_avg_1m: loadavg_1m(),
            timestamp_ms: now_ms(),
        }
    }
}

fn loadavg_1m() -> f64 {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/loadavg")
            .ok()
            .and_then(|s| {
                s.split_whitespace()
                    .next()
                    .and_then(|v| v.parse::<f64>().ok())
            })
            .unwrap_or(0.0)
    }
    #[cfg(not(target_os = "linux"))]
    {
        0.0
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
