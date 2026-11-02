use std::time::{Duration, Instant};

use crate::state::SharedSnapshot;

pub async fn sample_loop(snapshot: SharedSnapshot) {
    let mut sampler = ProcessSampler::new();
    loop {
        tokio::time::sleep(Duration::from_secs(3)).await;
        if let Some(sample) = sampler.sample() {
            let mut current = snapshot.write().await;
            current.metrics.process_cpu_percent = sample.cpu_percent;
            current.metrics.process_memory_bytes = sample.memory_bytes;
        }
    }
}

struct ProcessSampler {
    last_at: Instant,
    last_cpu_100ns: u64,
}

struct Sample {
    cpu_percent: f64,
    memory_bytes: u64,
}

impl ProcessSampler {
    fn new() -> Self {
        Self {
            last_at: Instant::now(),
            last_cpu_100ns: process_cpu_100ns().unwrap_or(0),
        }
    }

    fn sample(&mut self) -> Option<Sample> {
        let now = Instant::now();
        let cpu = process_cpu_100ns()?;
        let wall_seconds = now.duration_since(self.last_at).as_secs_f64();
        if wall_seconds <= 0.0 {
            return None;
        }
        let cpu_seconds = cpu.saturating_sub(self.last_cpu_100ns) as f64 / 10_000_000.0;
        self.last_at = now;
        self.last_cpu_100ns = cpu;
        Some(Sample {
            cpu_percent: (cpu_seconds / wall_seconds * 100.0).clamp(0.0, 999.0),
            memory_bytes: process_memory_bytes().unwrap_or(0),
        })
    }
}

fn process_cpu_100ns() -> Option<u64> {
    use windows_sys::Win32::{
        Foundation::FILETIME,
        System::Threading::{GetCurrentProcess, GetProcessTimes},
    };
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let succeeded = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    };
    if succeeded == 0 {
        return None;
    }
    let as_u64 =
        |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    Some(as_u64(kernel).saturating_add(as_u64(user)))
}

fn process_memory_bytes() -> Option<u64> {
    use windows_sys::Win32::System::{
        ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2,
        },
        Threading::GetCurrentProcess,
    };
    let mut counters = PROCESS_MEMORY_COUNTERS_EX2 {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32,
        ..PROCESS_MEMORY_COUNTERS_EX2::default()
    };
    let succeeded = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX2).cast::<PROCESS_MEMORY_COUNTERS>(),
            counters.cb,
        )
    };
    if succeeded == 0 {
        return None;
    }
    let bytes = if counters.PrivateWorkingSetSize > 0 {
        counters.PrivateWorkingSetSize
    } else {
        counters.WorkingSetSize
    };
    Some(bytes as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_memory_uses_a_nonzero_private_working_set() {
        assert!(process_memory_bytes().is_some_and(|bytes| bytes > 0));
    }
}
