//! 监控服务：与 C# 版 MonitorService.cs 对应。
//! 每台服务器一个监控任务并行轮询；ApplyConfig 全量重建；
//! 学到主机密钥指纹时原地补丁进当前配置（不重启监控）。
//! 事件统一经分发任务转到 UI 线程队列（对应 C# 事件 + BeginInvoke）。

use super::monitor::MonitorHandle;
use super::notify_sm::NotificationStateMachine;
use super::snapshot::{IdleNotificationArgs, ServerViewState};
use crate::config::AppConfig;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;
use uuid::Uuid;

/// 事件出口：把 AppEvent 投递到 UI 线程（发送后 PostMessage 唤醒）。
pub type EventSink = Arc<dyn Fn(AppEvent) + Send + Sync>;

/// 由分发任务投递到 UI 线程队列的事件。
#[derive(Debug, Clone)]
pub enum AppEvent {
    /// 服务器列表/配置层面变化（对应 C# Changed 事件携带 Guid.Empty）。
    ServersChanged,
    /// 单台服务器状态或新采样更新。
    ServerChanged(Uuid),
    /// 学习到主机密钥指纹（UI 需持久化配置）。
    HostKeyLearned(Uuid, String),
    /// 检测到 GPU 空闲（当前用户进程全部退出）。
    IdleDetected(IdleNotificationArgs),
}

/// 监控任务 → 分发任务的原始事件（带 generation 以丢弃过期任务的事件）。
#[derive(Debug)]
pub(crate) enum RawEvent {
    Snapshot {
        generation: u64,
        server_id: Uuid,
        snapshot: Arc<super::snapshot::ServerSnapshot>,
    },
    StatusChanged {
        generation: u64,
        server_id: Uuid,
    },
    HostKeyLearned {
        generation: u64,
        server_id: Uuid,
        fingerprint: String,
    },
}

pub(crate) enum ServiceCmd {
    ApplyConfig(AppConfig),
    RefreshAll,
    Refresh(Uuid),
    Shutdown,
}

struct Runtime {
    monitor: MonitorHandle,
    notify_sm: NotificationStateMachine,
}

struct ServiceCore {
    generation: u64,
    config: AppConfig,
    runtimes: HashMap<Uuid, Runtime>,
}

pub struct MonitorService {
    core: Arc<Mutex<ServiceCore>>,
    cmd_tx: UnboundedSender<ServiceCmd>,
    dispatcher: Option<JoinHandle<()>>,
}

impl MonitorService {
    pub fn new(runtime: &tokio::runtime::Handle, sink: EventSink) -> Self {
        let (cmd_tx, cmd_rx) = unbounded_channel();
        let (raw_tx, raw_rx) = unbounded_channel();
        let core = Arc::new(Mutex::new(ServiceCore {
            generation: 0,
            config: AppConfig::default(),
            runtimes: HashMap::new(),
        }));

        let dispatcher = runtime.spawn(dispatch_loop(
            core.clone(),
            raw_tx.clone(),
            raw_rx,
            cmd_rx,
            sink,
        ));

        MonitorService {
            core,
            cmd_tx,
            dispatcher: Some(dispatcher),
        }
    }

    pub fn apply_config(&self, config: AppConfig) {
        let _ = self.cmd_tx.send(ServiceCmd::ApplyConfig(config));
    }

    pub fn refresh_all(&self) {
        let _ = self.cmd_tx.send(ServiceCmd::RefreshAll);
    }

    pub fn refresh(&self, server_id: Uuid) {
        let _ = self.cmd_tx.send(ServiceCmd::Refresh(server_id));
    }

    pub fn get_states(&self) -> Vec<ServerViewState> {
        let core = self
            .core
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        core.config
            .servers
            .iter()
            .map(|server| match core.runtimes.get(&server.id) {
                Some(runtime) => {
                    let status = runtime
                        .monitor
                        .shared
                        .status
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .clone();
                    ServerViewState {
                        server_id: server.id,
                        phase: status.phase,
                        error: status.error,
                        history: runtime.monitor.shared.queue.snapshots(),
                    }
                }
                None => ServerViewState {
                    server_id: server.id,
                    phase: super::snapshot::MonitorPhase::Stopped,
                    error: None,
                    history: Vec::new(),
                },
            })
            .collect()
    }

    pub fn get_server(&self, server_id: Uuid) -> Option<crate::config::ServerConfig> {
        let core = self
            .core
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        core.config
            .servers
            .iter()
            .find(|server| server.id == server_id)
            .cloned()
    }

    pub fn config_snapshot(&self) -> AppConfig {
        self.core
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .config
            .clone()
    }

    pub fn shutdown(&mut self) {
        let _ = self.cmd_tx.send(ServiceCmd::Shutdown);
        if let Some(dispatcher) = self.dispatcher.take() {
            dispatcher.abort();
        }
    }
}

impl Drop for MonitorService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

async fn dispatch_loop(
    core: Arc<Mutex<ServiceCore>>,
    raw_tx: UnboundedSender<RawEvent>,
    mut raw_rx: UnboundedReceiver<RawEvent>,
    mut cmd_rx: UnboundedReceiver<ServiceCmd>,
    sink: EventSink,
) {
    let mut prune_interval = tokio::time::interval(std::time::Duration::from_secs(1));
    prune_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => match cmd {
                Some(ServiceCmd::ApplyConfig(config)) => apply_config(&core, &raw_tx, &sink, config),
                Some(ServiceCmd::RefreshAll) => {
                    let core_guard = core.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    for runtime in core_guard.runtimes.values() {
                        runtime.monitor.request_refresh();
                    }
                }
                Some(ServiceCmd::Refresh(server_id)) => {
                    let core_guard = core.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    if let Some(runtime) = core_guard.runtimes.get(&server_id) {
                        runtime.monitor.request_refresh();
                    }
                }
                Some(ServiceCmd::Shutdown) | None => break,
            },
            raw = raw_rx.recv() => match raw {
                Some(event) => handle_raw_event(&core, &sink, event),
                None => break,
            },
            _ = prune_interval.tick() => {
                let core_guard = core.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                for runtime in core_guard.runtimes.values() {
                    runtime.monitor.shared.queue.prune();
                }
            }
        }
    }

    // 关闭所有监控任务。
    let monitors: Vec<MonitorHandle> = {
        let mut core_guard = core.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        core_guard.runtimes.drain().map(|(_, runtime)| runtime.monitor).collect()
    };
    for monitor in monitors {
        monitor.shutdown_and_abort();
    }
}

fn apply_config(
    core: &Arc<Mutex<ServiceCore>>,
    raw_tx: &UnboundedSender<RawEvent>,
    sink: &EventSink,
    config: AppConfig,
) {
    let mut core_guard = core.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    // 与 C# 一致：先全部停掉旧监控，再全部启动新监控。
    let old: Vec<MonitorHandle> = core_guard
        .runtimes
        .drain()
        .map(|(_, runtime)| runtime.monitor)
        .collect();
    for monitor in old {
        monitor.shutdown_and_abort();
    }

    core_guard.generation += 1;
    let generation = core_guard.generation;
    core_guard.config = config.clone();

    for server in &config.servers {
        let monitor = super::monitor::spawn_monitor(
            server.clone(),
            config.refresh_seconds,
            generation,
            raw_tx.clone(),
        );
        core_guard.runtimes.insert(
            server.id,
            Runtime {
                monitor,
                notify_sm: NotificationStateMachine::new(),
            },
        );
    }

    drop(core_guard);
    (sink)(AppEvent::ServersChanged);
}

fn handle_raw_event(core: &Arc<Mutex<ServiceCore>>, sink: &EventSink, event: RawEvent) {
    match event {
        RawEvent::Snapshot {
            generation,
            server_id,
            snapshot,
        } => {
            let prepared = {
                let mut core_guard = core.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                if core_guard.generation != generation {
                    return;
                }
                let Some(server) = core_guard
                    .config
                    .servers
                    .iter()
                    .find(|server| server.id == server_id) else {
                    return;
                };
                let notify_enabled =
                    core_guard.config.notifications_enabled && server.notifications_enabled;
                let label = server.display_host();

                let Some(runtime) = core_guard.runtimes.get_mut(&server_id) else {
                    return;
                };
                let idle_gpus = runtime.notify_sm.evaluate(&snapshot, notify_enabled);
                Some((label, idle_gpus))
            };

            (sink)(AppEvent::ServerChanged(server_id));
            if let Some((label, idle_gpus)) = prepared {
                if !idle_gpus.is_empty() {
                    (sink)(AppEvent::IdleDetected(IdleNotificationArgs {
                        server_id,
                        server_label: label,
                        gpu_indices: idle_gpus,
                        snapshot,
                    }));
                }
            }
        }
        RawEvent::StatusChanged { generation, server_id } => {
            let core_guard = core.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if core_guard.generation != generation {
                return;
            }
            (sink)(AppEvent::ServerChanged(server_id));
        }
        RawEvent::HostKeyLearned {
            generation,
            server_id,
            fingerprint,
        } => {
            let mut core_guard = core.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if core_guard.generation != generation {
                return;
            }
            // 原地补丁：不重启监控，只更新当前配置里的指纹。
            if let Some(server) = core_guard
                .config
                .servers
                .iter_mut()
                .find(|server| server.id == server_id)
            {
                server.host_key_fingerprint_sha256 = Some(fingerprint.clone());
            }
            drop(core_guard);
            (sink)(AppEvent::HostKeyLearned(server_id, fingerprint));
        }
    }
}
