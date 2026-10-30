//! 采样队列：内存 FIFO，保留最近 300 秒、硬上限 512 条（与 C# SampleQueue 一致）。
//! 这是样本唯一的存放处：不写磁盘；入队、每次读取和监控服务定时器都会做清理。

use super::snapshot::ServerSnapshot;
use chrono::{DateTime, TimeDelta, Utc};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub const RETENTION_SECS: i64 = 300;
pub const MAX_SAMPLES: usize = 512;

pub fn retention() -> TimeDelta {
    TimeDelta::seconds(RETENTION_SECS)
}

pub struct SampleQueue {
    samples: Mutex<VecDeque<Arc<ServerSnapshot>>>,
}

impl Default for SampleQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl SampleQueue {
    pub fn new() -> Self {
        SampleQueue {
            samples: Mutex::new(VecDeque::new()),
        }
    }

    pub fn count(&self) -> usize {
        self.samples
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    pub fn enqueue(&self, sample: Arc<ServerSnapshot>) {
        let mut samples = self
            .samples
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        samples.push_back(sample);
        prune_core(&mut samples, Utc::now());
    }

    pub fn prune(&self) {
        self.prune_at(Utc::now());
    }

    pub fn prune_at(&self, now: DateTime<Utc>) {
        let mut samples = self
            .samples
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        prune_core(&mut samples, now);
    }

    pub fn snapshots(&self) -> Vec<Arc<ServerSnapshot>> {
        self.snapshots_at(None)
    }

    pub fn snapshots_at(&self, now: Option<DateTime<Utc>>) -> Vec<Arc<ServerSnapshot>> {
        let mut samples = self
            .samples
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        prune_core(&mut samples, now.unwrap_or_else(Utc::now));
        samples.iter().cloned().collect()
    }

    pub fn latest(&self) -> Option<Arc<ServerSnapshot>> {
        self.latest_at(None)
    }

    pub fn latest_at(&self, now: Option<DateTime<Utc>>) -> Option<Arc<ServerSnapshot>> {
        let mut samples = self
            .samples
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        prune_core(&mut samples, now.unwrap_or_else(Utc::now));
        samples.back().cloned()
    }

    pub fn clear(&self) {
        self.samples
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }
}

fn prune_core(samples: &mut VecDeque<Arc<ServerSnapshot>>, now: DateTime<Utc>) {
    let cutoff = now - retention();
    while samples
        .front()
        .is_some_and(|sample| sample.captured_at < cutoff)
    {
        samples.pop_front();
    }

    while samples.len() > MAX_SAMPLES {
        samples.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitor::snapshot::{GpuProcessTelemetry, GpuTelemetry, RamTelemetry};
    use std::collections::HashMap;
    use uuid::Uuid;

    fn snapshot(timestamp: DateTime<Utc>, gpu_indices: &[i32], counts: &[(i32, i32)]) -> Arc<ServerSnapshot> {
        let gpus: Vec<GpuTelemetry> = gpu_indices
            .iter()
            .map(|index| GpuTelemetry {
                index: *index,
                uuid: format!("uuid-{index}"),
                name: format!("GPU {index}"),
                utilization_percent: Some(10.0),
                memory_used_mib: Some(100),
                memory_total_mib: Some(1000),
                temperature_c: Some(40.0),
                power_w: Some(50.0),
                power_limit_w: Some(100.0),
            })
            .collect();

        let mut my_process_counts = HashMap::new();
        let mut processes = Vec::new();
        for (gpu, count) in counts {
            my_process_counts.insert(*gpu, *count);
            for i in 0..*count {
                processes.push(GpuProcessTelemetry {
                    gpu_index: *gpu,
                    pid: 1000 + i,
                    user: "alice".into(),
                    command: "python".into(),
                    memory_mib: Some(100),
                });
            }
        }

        Arc::new(ServerSnapshot {
            server_id: Uuid::new_v4(),
            captured_at: timestamp,
            gpus,
            ram: Some(RamTelemetry {
                total_mib: 1000,
                used_mib: 200,
                available_mib: 800,
            }),
            processes,
            process_data_complete: true,
            my_process_counts,
            warning: None,
        })
    }

    #[test]
    fn drops_samples_older_than_300_seconds() {
        let queue = SampleQueue::new();
        let start = Utc::now();

        queue.enqueue(snapshot(start, &[1], &[(1, 1)]));
        queue.enqueue(snapshot(start + TimeDelta::seconds(100), &[1], &[(1, 1)]));
        queue.enqueue(snapshot(start + TimeDelta::seconds(250), &[1], &[(1, 1)]));

        let later = start + TimeDelta::seconds(301);
        let visible = queue.snapshots_at(Some(later));

        assert_eq!(visible.len(), 2);
        assert!(visible
            .iter()
            .all(|sample| later - sample.captured_at <= retention()));
    }

    #[test]
    fn does_not_retain_anything_beyond_300_seconds_in_memory() {
        let queue = SampleQueue::new();
        let now = Utc::now();
        for i in 0..30 {
            queue.enqueue(snapshot(now - TimeDelta::seconds(i * 5), &[1], &[(1, 0)]));
        }

        assert_eq!(queue.count(), 30);
        queue.prune_at(now + TimeDelta::seconds(600));

        assert_eq!(queue.count(), 0);
        assert!(queue.snapshots_at(Some(now + TimeDelta::seconds(600))).is_empty());
    }

    #[test]
    fn enforces_hard_cap() {
        let queue = SampleQueue::new();
        let now = Utc::now();
        for i in 0..(MAX_SAMPLES + 20) {
            queue.enqueue(snapshot(now + TimeDelta::milliseconds(i as i64), &[1], &[(1, 0)]));
        }

        assert_eq!(queue.count(), MAX_SAMPLES);
    }
}
