//! GPU 空闲通知状态机：与 C# 版 NotificationStateMachine.cs 逐行对应。
//!
//! 规则：
//! - 进程数据不完整的采样（pmon/ps 失败）抑制本轮，并强制下一轮重新播种；
//! - 两次有效采样间隔超过 300 秒（SampleQueue.Retention）重新播种；
//! - 首次见到的 GPU 只记录状态、不通知；
//! - 只有「上一轮有我的进程 && 本轮为 0」的 GPU 触发，同轮合并、按索引升序；
//! - 通知开关关闭时状态照常更新，只是不返回通知。

use super::queue;
use super::snapshot::ServerSnapshot;
use std::collections::HashMap;

pub struct NotificationStateMachine {
    had_mine: HashMap<i32, bool>,
    last_valid_sample_at: Option<chrono::DateTime<chrono::Utc>>,
    seeded: bool,
    invalid_since_last_valid: bool,
}

impl Default for NotificationStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl NotificationStateMachine {
    pub fn new() -> Self {
        NotificationStateMachine {
            had_mine: HashMap::new(),
            last_valid_sample_at: None,
            seeded: false,
            invalid_since_last_valid: false,
        }
    }

    pub fn evaluate(&mut self, snapshot: &ServerSnapshot, notify_enabled: bool) -> Vec<i32> {
        if !snapshot.process_data_complete {
            self.invalid_since_last_valid = true;
            return Vec::new();
        }

        let must_seed = !self.seeded
            || self.invalid_since_last_valid
            || self.last_valid_sample_at.is_none()
            || snapshot.captured_at.signed_duration_since(self.last_valid_sample_at.unwrap())
                > queue::retention();

        let mut became_idle: Vec<i32> = Vec::new();
        let mut seen_gpus = std::collections::HashSet::new();

        for gpu in &snapshot.gpus {
            seen_gpus.insert(gpu.index);
            let has_mine = snapshot
                .my_process_counts
                .get(&gpu.index)
                .is_some_and(|count| *count > 0);

            let previous = if must_seed {
                None
            } else {
                self.had_mine.get(&gpu.index).copied()
            };

            match previous {
                None => {
                    self.had_mine.insert(gpu.index, has_mine);
                }
                Some(previous) => {
                    if previous && !has_mine {
                        became_idle.push(gpu.index);
                    }
                    self.had_mine.insert(gpu.index, has_mine);
                }
            }
        }

        self.had_mine.retain(|index, _| seen_gpus.contains(index));

        self.last_valid_sample_at = Some(snapshot.captured_at);
        self.invalid_since_last_valid = false;
        self.seeded = true;

        if !notify_enabled || became_idle.is_empty() {
            return Vec::new();
        }

        became_idle.sort_unstable();
        became_idle
    }

    pub fn reset(&mut self) {
        self.had_mine.clear();
        self.last_valid_sample_at = None;
        self.seeded = false;
        self.invalid_since_last_valid = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitor::snapshot::{GpuProcessTelemetry, GpuTelemetry, RamTelemetry};
    use chrono::{DateTime, TimeDelta, Utc};
    use std::collections::HashMap;
    use uuid::Uuid;

    fn make_snapshot(
        timestamp: DateTime<Utc>,
        gpu_indices: &[i32],
        counts: Option<&[(i32, i32)]>,
        complete: bool,
    ) -> ServerSnapshot {
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

        let pairs: Vec<(i32, i32)> = counts
            .map(|list| list.to_vec())
            .unwrap_or_else(|| gpu_indices.iter().map(|index| (*index, 0)).collect());

        let mut my_process_counts = HashMap::new();
        let mut processes = Vec::new();
        for (gpu, count) in pairs {
            my_process_counts.insert(gpu, count);
            for i in 0..count {
                processes.push(GpuProcessTelemetry {
                    gpu_index: gpu,
                    pid: 1000 + i,
                    user: "alice".into(),
                    command: "python".into(),
                    memory_mib: Some(100),
                });
            }
        }

        ServerSnapshot {
            server_id: Uuid::new_v4(),
            captured_at: timestamp,
            gpus,
            ram: Some(RamTelemetry {
                total_mib: 1000,
                used_mib: 200,
                available_mib: 800,
            }),
            processes,
            process_data_complete: complete,
            my_process_counts,
            warning: None,
        }
    }

    #[test]
    fn seeds_then_notifies_on_transition() {
        let mut machine = NotificationStateMachine::new();
        let now = Utc::now();

        assert!(machine
            .evaluate(&make_snapshot(now, &[0], Some(&[(0, 1)]), true), true)
            .is_empty());
        assert_eq!(
            machine.evaluate(&make_snapshot(now + TimeDelta::seconds(5), &[0], Some(&[(0, 0)]), true), true),
            vec![0]
        );
        assert!(machine
            .evaluate(&make_snapshot(now + TimeDelta::seconds(10), &[0], Some(&[(0, 0)]), true), true)
            .is_empty());
    }

    #[test]
    fn coalesces_multiple_gpus() {
        let mut machine = NotificationStateMachine::new();
        let now = Utc::now();

        machine.evaluate(
            &make_snapshot(now, &[0, 1], Some(&[(0, 1), (1, 1)]), true),
            true,
        );
        let idle = machine.evaluate(
            &make_snapshot(now + TimeDelta::seconds(5), &[0, 1], Some(&[(0, 0), (1, 0)]), true),
            true,
        );

        assert_eq!(idle, vec![0, 1]);
    }

    #[test]
    fn invalid_data_suppresses_and_reseeds() {
        let mut machine = NotificationStateMachine::new();
        let now = Utc::now();

        machine.evaluate(&make_snapshot(now, &[0], Some(&[(0, 1)]), true), true);
        assert!(machine
            .evaluate(&make_snapshot(now + TimeDelta::seconds(5), &[0], None, false), true)
            .is_empty());
        assert!(machine
            .evaluate(&make_snapshot(now + TimeDelta::seconds(10), &[0], Some(&[(0, 0)]), true), true)
            .is_empty());

        machine.evaluate(&make_snapshot(now + TimeDelta::seconds(15), &[0], Some(&[(0, 1)]), true), true);
        assert_eq!(
            machine.evaluate(&make_snapshot(now + TimeDelta::seconds(20), &[0], Some(&[(0, 0)]), true), true),
            vec![0]
        );
    }

    #[test]
    fn disabled_updates_state_without_notifying() {
        let mut machine = NotificationStateMachine::new();
        let now = Utc::now();

        machine.evaluate(&make_snapshot(now, &[0], Some(&[(0, 1)]), true), true);
        assert!(machine
            .evaluate(&make_snapshot(now + TimeDelta::seconds(5), &[0], Some(&[(0, 0)]), true), false)
            .is_empty());
        assert!(machine
            .evaluate(&make_snapshot(now + TimeDelta::seconds(10), &[0], Some(&[(0, 0)]), true), true)
            .is_empty());
    }

    #[test]
    fn gap_over_300_seconds_reseeds() {
        let mut machine = NotificationStateMachine::new();
        let now = Utc::now();

        machine.evaluate(&make_snapshot(now, &[0], Some(&[(0, 1)]), true), true);
        let idle_after_gap = machine.evaluate(
            &make_snapshot(now + TimeDelta::seconds(301), &[0], Some(&[(0, 0)]), true),
            true,
        );

        assert!(idle_after_gap.is_empty());
    }

    #[test]
    fn new_gpu_does_not_notify() {
        let mut machine = NotificationStateMachine::new();
        let now = Utc::now();

        machine.evaluate(&make_snapshot(now, &[0], Some(&[(0, 1)]), true), true);
        let transitions = machine.evaluate(
            &make_snapshot(now + TimeDelta::seconds(5), &[0, 1], Some(&[(0, 1), (1, 0)]), true),
            true,
        );

        assert!(transitions.is_empty());
    }
}
