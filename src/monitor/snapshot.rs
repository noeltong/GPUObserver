//! 监控数据模型：与 C# 版 Monitoring/Snapshot.cs 对应。

use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct GpuTelemetry {
    pub index: i32,
    pub uuid: String,
    pub name: String,
    pub utilization_percent: Option<f64>,
    pub memory_used_mib: Option<i64>,
    pub memory_total_mib: Option<i64>,
    pub temperature_c: Option<f64>,
    pub power_w: Option<f64>,
    pub power_limit_w: Option<f64>,
}

impl GpuTelemetry {
    pub fn memory_percent(&self) -> Option<f64> {
        match (self.memory_used_mib, self.memory_total_mib) {
            (Some(used), Some(total)) if total > 0 => {
                Some((used as f64 * 100.0 / total as f64).clamp(0.0, 100.0))
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RamTelemetry {
    pub total_mib: i64,
    pub used_mib: i64,
    pub available_mib: i64,
}

impl RamTelemetry {
    pub fn percent(&self) -> f64 {
        if self.total_mib > 0 {
            (self.used_mib as f64 * 100.0 / self.total_mib as f64).clamp(0.0, 100.0)
        } else {
            0.0
        }
    }
}

#[derive(Debug, Clone)]
pub struct GpuProcessTelemetry {
    pub gpu_index: i32,
    pub pid: i32,
    pub user: String,
    pub command: String,
    pub memory_mib: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ServerSnapshot {
    pub server_id: Uuid,
    pub captured_at: DateTime<Utc>,
    pub gpus: Vec<GpuTelemetry>,
    pub ram: Option<RamTelemetry>,
    pub processes: Vec<GpuProcessTelemetry>,
    pub process_data_complete: bool,
    pub my_process_counts: HashMap<i32, i32>,
    pub warning: Option<String>,
}

impl ServerSnapshot {
    pub fn my_process_count(&self) -> usize {
        self.processes.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorPhase {
    Stopped,
    Connecting,
    Online,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorErrorKind {
    None,
    Network,
    Authentication,
    HostKey,
    Credential,
    PrivateKey,
    Timeout,
    NvidiaSmiMissing,
    Command,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorError {
    pub kind: MonitorErrorKind,
    pub message: String,
    pub detail: Option<String>,
}

impl MonitorError {
    pub fn new(kind: MonitorErrorKind, message: impl Into<String>) -> Self {
        MonitorError {
            kind,
            message: message.into(),
            detail: None,
        }
    }

    pub fn with_detail(
        kind: MonitorErrorKind,
        message: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        MonitorError {
            kind,
            message: message.into(),
            detail: Some(detail.into()),
        }
    }

    pub fn network(message: impl Into<String>) -> Self {
        Self::new(MonitorErrorKind::Network, message)
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(MonitorErrorKind::Timeout, message)
    }

    pub fn authentication() -> Self {
        Self::new(
            MonitorErrorKind::Authentication,
            "SSH 认证失败，请检查用户名、密码或私钥。",
        )
    }
}

pub type MonResult<T> = Result<T, MonitorError>;

#[derive(Debug, Clone)]
pub struct MonitorStatus {
    pub server_id: Uuid,
    pub phase: MonitorPhase,
    pub error: Option<MonitorError>,
    pub changed_at: DateTime<Utc>,
}

impl MonitorStatus {
    pub fn stopped(server_id: Uuid) -> Self {
        MonitorStatus {
            server_id,
            phase: MonitorPhase::Stopped,
            error: None,
            changed_at: Utc::now(),
        }
    }

    pub fn connecting(server_id: Uuid) -> Self {
        MonitorStatus {
            server_id,
            phase: MonitorPhase::Connecting,
            error: None,
            changed_at: Utc::now(),
        }
    }

    pub fn online(server_id: Uuid) -> Self {
        MonitorStatus {
            server_id,
            phase: MonitorPhase::Online,
            error: None,
            changed_at: Utc::now(),
        }
    }

    pub fn failed(server_id: Uuid, error: MonitorError) -> Self {
        MonitorStatus {
            server_id,
            phase: MonitorPhase::Error,
            error: Some(error),
            changed_at: Utc::now(),
        }
    }

    /// 与 C# UpdateStatus 的去重判断一致：仅相位 + 错误种类 + 错误消息决定是否视为变化。
    pub fn same_signal(&self, other: &MonitorStatus) -> bool {
        self.phase == other.phase
            && self.error.as_ref().map(|e| (e.kind, e.message.clone()))
                == other.error.as_ref().map(|e| (e.kind, e.message.clone()))
    }
}

#[derive(Debug, Clone)]
pub struct ServerViewState {
    pub server_id: Uuid,
    pub phase: MonitorPhase,
    pub error: Option<MonitorError>,
    pub history: Vec<Arc<ServerSnapshot>>,
}

impl ServerViewState {
    pub fn latest(&self) -> Option<&Arc<ServerSnapshot>> {
        self.history.last()
    }
}

#[derive(Debug, Clone)]
pub struct IdleNotificationArgs {
    pub server_id: Uuid,
    pub server_label: String,
    pub gpu_indices: Vec<i32>,
    pub snapshot: Arc<ServerSnapshot>,
}
