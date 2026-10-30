//! 单服务器监控循环：与 C# 版 ServerMonitor.cs 对应。
//! 成功后按全局刷新间隔休眠；失败按 5s→×2→60s 退避，
//! 认证/主机密钥/凭据/私钥类持久错误固定 60s；手动刷新可打断休眠但不会并发轮询。

use super::queue::SampleQueue;
use super::service::RawEvent;
use super::snapshot::{MonitorStatus, ServerSnapshot};
use crate::config::ServerConfig;
use crate::ssh::{self, SshSession};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

pub enum MonitorCmd {
    Refresh,
    Shutdown,
}

/// 监控任务的共享读侧：UI 线程读取状态与历史。
pub struct MonitorShared {
    pub status: Mutex<MonitorStatus>,
    pub queue: SampleQueue,
    pub stopped: AtomicBool,
}

impl MonitorShared {
    pub fn new(server_id: uuid::Uuid) -> Self {
        MonitorShared {
            status: Mutex::new(MonitorStatus::stopped(server_id)),
            queue: SampleQueue::new(),
            stopped: AtomicBool::new(false),
        }
    }
}

pub struct MonitorHandle {
    pub server_id: uuid::Uuid,
    pub shared: Arc<MonitorShared>,
    cmd_tx: UnboundedSender<MonitorCmd>,
    join: JoinHandle<()>,
    cancelled: Arc<AtomicBool>,
}

impl MonitorHandle {
    pub fn request_refresh(&self) {
        let _ = self.cmd_tx.send(MonitorCmd::Refresh);
    }

    pub fn shutdown_and_abort(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        let _ = self.cmd_tx.send(MonitorCmd::Shutdown);
        self.join.abort();
    }
}

/// 启动监控任务（对应 C# ServerMonitor.Start + RunAsync）。
pub fn spawn_monitor(
    config: ServerConfig,
    refresh_seconds: i32,
    generation: u64,
    raw_tx: UnboundedSender<RawEvent>,
) -> MonitorHandle {
    let server_id = config.id;
    let shared = Arc::new(MonitorShared::new(server_id));
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let cancelled = Arc::new(AtomicBool::new(false));

    let task_shared = shared.clone();
    let join = tokio::spawn(run_monitor(
        config,
        refresh_seconds,
        generation,
        raw_tx,
        cmd_rx,
        task_shared,
        cancelled.clone(),
    ));

    MonitorHandle {
        server_id,
        shared,
        cmd_tx,
        join,
        cancelled,
    }
}

async fn run_monitor(
    config: ServerConfig,
    refresh_seconds: i32,
    generation: u64,
    raw_tx: UnboundedSender<RawEvent>,
    mut cmd_rx: UnboundedReceiver<MonitorCmd>,
    shared: Arc<MonitorShared>,
    cancelled: Arc<AtomicBool>,
) {
    let server_id = config.id;
    let refresh_seconds = refresh_seconds.clamp(2, 300) as f64;
    let mut backoff_secs: f64 = 5.0;
    let mut session: Option<SshSession> = None;
    let mut last_logged_error: Option<String> = None;

    // 与 C# Start() 一致：任务启动即进入 Connecting。
    update_status(&shared, &raw_tx, generation, MonitorStatus::connecting(server_id));

    'main: loop {
        let mut success = false;

        let poll_result = poll_cycle(&mut session, &config, &raw_tx, generation).await;
        match poll_result {
            Ok(snapshot) => {
                shared.queue.enqueue(snapshot.clone());
                backoff_secs = 5.0;
                last_logged_error = None;
                success = true;
                update_status(&shared, &raw_tx, generation, MonitorStatus::online(server_id));
                let _ = raw_tx.send(RawEvent::Snapshot {
                    generation,
                    server_id,
                    snapshot,
                });
            }
            Err(err) => {
                // 任何失败都丢弃会话，下一轮重建（与 C# DisposeSession 一致）。
                session = None;
                if last_logged_error.as_deref() != Some(err.message.as_str()) {
                    last_logged_error = Some(err.message.clone());
                    crate::util::error(
                        &format!("服务器 {} 监控失败：{}", config.display_host(), err.message),
                        err.detail.as_deref(),
                    );
                }

                update_status(
                    &shared,
                    &raw_tx,
                    generation,
                    MonitorStatus::failed(server_id, err.clone()),
                );

                let persistent = matches!(
                    err.kind,
                    super::snapshot::MonitorErrorKind::Authentication
                        | super::snapshot::MonitorErrorKind::HostKey
                        | super::snapshot::MonitorErrorKind::Credential
                        | super::snapshot::MonitorErrorKind::PrivateKey
                );

                backoff_secs = (backoff_secs * 2.0).min(60.0);
                if persistent {
                    backoff_secs = 60.0;
                }
            }
        }

        let delay_secs = if success {
            refresh_seconds
        } else {
            backoff_secs
        };

        // DelayWithWake：手动刷新打断休眠；轮询期间收到的刷新也要立即生效。
        loop {
            match cmd_rx.try_recv() {
                Ok(MonitorCmd::Shutdown) => break 'main,
                Ok(MonitorCmd::Refresh) => break,
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                    tokio::select! {
                        cmd = cmd_rx.recv() => match cmd {
                            Some(MonitorCmd::Refresh) => break,
                            Some(MonitorCmd::Shutdown) | None => break 'main,
                        },
                        _ = tokio::time::sleep(std::time::Duration::from_secs_f64(delay_secs)) => break,
                    }
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break 'main,
            }
        }

        if cancelled.load(Ordering::SeqCst) {
            break 'main;
        }
    }

    // finally：丢弃会话、修剪历史、置为 Stopped。
    drop(session);
    shared.queue.prune();
    shared.stopped.store(true, Ordering::SeqCst);
    update_status(&shared, &raw_tx, generation, MonitorStatus::stopped(server_id));
}

async fn poll_cycle(
    session: &mut Option<SshSession>,
    config: &ServerConfig,
    raw_tx: &UnboundedSender<RawEvent>,
    generation: u64,
) -> Result<Arc<ServerSnapshot>, super::snapshot::MonitorError> {
    let needs_connect = !session.as_ref().is_some_and(|s| s.is_connected());
    if needs_connect {
        *session = None;
        let raw_tx_clone = raw_tx.clone();
        let learned_server_id = config.id;
        let learned_generation = generation;
        let mut new_session = SshSession::new(
            config.clone(),
            Box::new(move |fingerprint| {
                // 学习到主机密钥指纹：通知分发器补丁进当前配置并持久化。
                let _ = raw_tx_clone.send(RawEvent::HostKeyLearned {
                    generation: learned_generation,
                    server_id: learned_server_id,
                    fingerprint,
                });
            }),
        );
        new_session.connect().await?;
        *session = Some(new_session);
    }

    let active = session.as_mut().expect("session connected");
    ssh::poll_snapshot(active, config).await
}

fn update_status(
    shared: &MonitorShared,
    raw_tx: &UnboundedSender<RawEvent>,
    generation: u64,
    status: MonitorStatus,
) {
    let server_id = status.server_id;
    let changed = {
        let mut guard = shared
            .status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.same_signal(&status) {
            false
        } else {
            *guard = status;
            true
        }
    };

    if changed {
        let _ = raw_tx.send(RawEvent::StatusChanged {
            generation,
            server_id,
        });
    }
}
