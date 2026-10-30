//! SSH 会话：russh 封装，对应 C# 版 Ssh/SshSession.cs、PrivateKeyLoader.cs、ConnectionTester.cs。
//! SSH 连接与密钥读取完全内置，不依赖系统 OpenSSH 或 ssh-agent。

use crate::config::{ServerAuthType, ServerConfig};
use crate::monitor::parser;
use crate::monitor::probe;
use crate::monitor::snapshot::{
    MonResult, MonitorError, MonitorErrorKind, ServerSnapshot,
};
use crate::secrets;
use base64::Engine;
use russh::client::{self, Handle};
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::ChannelMsg;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::timeout;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(15);

pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_status: Option<i32>,
}

struct ClientHandler {
    pinned_fingerprint: Option<String>,
    observed: Arc<Mutex<Option<String>>>,
    mismatch: Arc<AtomicBool>,
    mismatch_observed: Arc<Mutex<Option<String>>>,
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let Some(fingerprint) = fingerprint_sha256(server_public_key) else {
            // 无法计算指纹时宁可拒绝，也不要放行未知密钥。
            return Ok(false);
        };

        *self.observed.lock().unwrap() = Some(fingerprint.clone());
        match &self.pinned_fingerprint {
            None => Ok(true), // 首次连接：信任并记录（TOFU）
            Some(pinned) if pinned == &fingerprint => Ok(true),
            Some(_) => {
                self.mismatch.store(true, Ordering::SeqCst);
                *self.mismatch_observed.lock().unwrap() = Some(fingerprint);
                Ok(false)
            }
        }
    }
}

/// OpenSSH 风格指纹：base64(SHA256(公钥 wire 编码))，去掉尾部 '=' 填充，
/// 不带 `SHA256:` 前缀（与 C# SSH.NET 的 FingerPrintSHA256 一致）。
fn fingerprint_sha256(key: &PublicKeyOrCertificate) -> Option<String> {
    let blob = match key {
        PublicKeyOrCertificate::PublicKey { key, .. } => key.to_bytes().ok()?,
        PublicKeyOrCertificate::Certificate(cert) => cert.to_bytes().ok()?,
    };
    Some(
        base64::engine::general_purpose::STANDARD
            .encode(Sha256::digest(&blob))
            .trim_end_matches('=')
            .to_string(),
    )
}

fn normalize_fingerprint(fingerprint: Option<&str>) -> Option<String> {
    let value = fingerprint?.trim();
    if value.is_empty() {
        return None;
    }
    Some(
        value
            .strip_prefix("SHA256:")
            .or_else(|| value.strip_prefix("sha256:"))
            .unwrap_or(value)
            .trim()
            .to_string(),
    )
}

pub struct SshSession {
    config: ServerConfig,
    host_key_learned: Box<dyn Fn(String) + Send + Sync>,
    handle: Option<Handle<ClientHandler>>,
}

impl SshSession {
    pub fn new(config: ServerConfig, host_key_learned: Box<dyn Fn(String) + Send + Sync>) -> Self {
        SshSession {
            config,
            host_key_learned,
            handle: None,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.handle.as_ref().is_some_and(|handle| !handle.is_closed())
    }

    pub async fn connect(&mut self) -> MonResult<()> {
        self.handle = None;

        let observed = Arc::new(Mutex::new(None));
        let mismatch = Arc::new(AtomicBool::new(false));
        let mismatch_observed = Arc::new(Mutex::new(None));
        let handler = ClientHandler {
            pinned_fingerprint: normalize_fingerprint(
                self.config.host_key_fingerprint_sha256.as_deref(),
            ),
            observed: observed.clone(),
            mismatch: mismatch.clone(),
            mismatch_observed: mismatch_observed.clone(),
        };

        let ssh_config = Arc::new(client::Config {
            keepalive_interval: Some(Duration::from_secs(30)),
            ..client::Config::default()
        });

        let connect_result = timeout(
            CONNECT_TIMEOUT,
            client::connect(
                ssh_config,
                (self.config.host.clone(), self.config.port as u16),
                handler,
            ),
        )
        .await;

        let mut handle = match connect_result {
            Err(_) => {
                return Err(MonitorError::timeout("连接服务器超时。"));
            }
            Ok(Err(err)) => {
                if mismatch.load(Ordering::SeqCst) {
                    let observed_fp = mismatch_observed.lock().unwrap().clone();
                    return Err(MonitorError::with_detail(
                        MonitorErrorKind::HostKey,
                        "服务器主机密钥已更改。为安全起见已阻断连接，请在设置中确认后再重新信任。",
                        observed_fp.unwrap_or_default(),
                    ));
                }
                return Err(map_connect_error(&err));
            }
            Ok(Ok(handle)) => handle,
        };

        self.authenticate(&mut handle).await?;

        let observed_fingerprint = observed.lock().unwrap().clone();
        self.handle = Some(handle);

        // TOFU：未记录过指纹时保存本次观察到的指纹。
        let pinned = normalize_fingerprint(self.config.host_key_fingerprint_sha256.as_deref());
        if pinned.is_none() {
            if let Some(fingerprint) = observed_fingerprint {
                self.config.host_key_fingerprint_sha256 = Some(fingerprint.clone());
                (self.host_key_learned)(fingerprint);
            }
        }

        Ok(())
    }

    async fn authenticate(&self, handle: &mut Handle<ClientHandler>) -> MonResult<()> {
        let user = self.config.username.clone();
        match self.config.auth_type {
            ServerAuthType::PrivateKey => {
                let passphrase = decrypt_secret(
                    self.config.protected_key_passphrase.as_deref(),
                    "私钥口令",
                )?;
                let passphrase_ref = if passphrase.is_empty() {
                    None
                } else {
                    Some(passphrase.as_str())
                };
                let key = load_private_key(
                    self.config.private_key_path.as_deref().unwrap_or(""),
                    passphrase_ref,
                )?;
                let hash = handle
                    .best_supported_rsa_hash()
                    .await
                    .map_err(|err| map_run_error(&err))?;
                let key_with_hash = PrivateKeyWithHashAlg::new(Arc::new(key), hash.flatten());
                let result = handle
                    .authenticate_publickey(user, key_with_hash)
                    .await
                    .map_err(|err| map_run_error(&err))?;
                if result.success() {
                    Ok(())
                } else {
                    Err(MonitorError::authentication())
                }
            }
            ServerAuthType::Password => {
                let password = decrypt_secret(self.config.protected_password.as_deref(), "登录密码")?;
                if password.is_empty() {
                    return Err(MonitorError::new(
                        MonitorErrorKind::Credential,
                        "未填写登录密码。",
                    ));
                }

                let result = handle
                    .authenticate_password(&user, &password)
                    .await
                    .map_err(|err| map_run_error(&err))?;
                if result.success() {
                    return Ok(());
                }

                // 与 C# 版一致：同一密码同时注册 password 与 keyboard-interactive，
                // 这里在 password 被拒后回退 keyboard-interactive，所有提示都用同一密码回答。
                let mut response = handle
                    .authenticate_keyboard_interactive_start(&user, None)
                    .await
                    .map_err(|err| map_run_error(&err))?;
                loop {
                    match response {
                        russh::client::KeyboardInteractiveAuthResponse::Success => {
                            return Ok(());
                        }
                        russh::client::KeyboardInteractiveAuthResponse::Failure { .. } => {
                            return Err(MonitorError::authentication());
                        }
                        russh::client::KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                            let answers = vec![password.clone(); prompts.len()];
                            response = handle
                                .authenticate_keyboard_interactive_respond(answers)
                                .await
                                .map_err(|err| map_run_error(&err))?;
                        }
                    }
                }
            }
        }
    }

    pub async fn run(&mut self, command: &str, timeout_duration: Duration) -> MonResult<CommandResult> {
        let Some(handle) = self.handle.as_ref() else {
            return Err(MonitorError::network("SSH 连接已断开。"));
        };
        if handle.is_closed() {
            return Err(MonitorError::network("SSH 连接已断开。"));
        }

        let exec = async {
            let mut channel = handle
                .channel_open_session()
                .await
                .map_err(|err| map_run_error(&err))?;
            channel
                .exec(true, command.as_bytes())
                .await
                .map_err(|err| map_run_error(&err))?;

            let mut stdout: Vec<u8> = Vec::new();
            let mut stderr = String::new();
            let mut exit_status: Option<u32> = None;
            let mut saw_close = false;
            while !saw_close {
                match channel.wait().await {
                    Some(ChannelMsg::Data { data }) => stdout.extend_from_slice(&data),
                    Some(ChannelMsg::ExtendedData { data, .. }) => {
                        stderr.push_str(&String::from_utf8_lossy(&data))
                    }
                    Some(ChannelMsg::ExitStatus { exit_status: code }) => exit_status = Some(code),
                    Some(ChannelMsg::Eof) => {}
                    Some(ChannelMsg::Close) | None => saw_close = true,
                    _ => {}
                }
            }

            Ok(CommandResult {
                stdout: String::from_utf8_lossy(&stdout).into_owned(),
                stderr,
                exit_status: exit_status.map(|value| value as i32),
            })
        };

        match timeout(timeout_duration, exec).await {
            Ok(result) => result,
            Err(_) => Err(MonitorError::timeout("采集命令超时，正在重试。")),
        }
    }

    pub async fn run_default_timeout(&mut self, command: &str) -> MonResult<CommandResult> {
        self.run(command, DEFAULT_COMMAND_TIMEOUT).await
    }
}

fn decrypt_secret(protected_value: Option<&str>, label: &str) -> MonResult<String> {
    let Some(protected) = protected_value else {
        return Ok(String::new());
    };
    if protected.is_empty() {
        return Ok(String::new());
    }

    secrets::unprotect(protected).map_err(|_| {
        MonitorError::new(
            MonitorErrorKind::Credential,
            format!("无法解密已保存的{label}，请在设置中重新输入。"),
        )
    })
}

/// 与 C# PrivateKeyLoader.Load 对应：路径校验 + PuTTY .ppk 拒绝 + russh 加载。
pub fn load_private_key(path: &str, passphrase: Option<&str>) -> MonResult<russh::keys::PrivateKey> {
    if path.trim().is_empty() {
        return Err(MonitorError::new(
            MonitorErrorKind::PrivateKey,
            "未选择私钥文件。",
        ));
    }

    let full_path = match std::path::absolute(Path::new(path)) {
        Ok(path) => path,
        Err(err) => {
            return Err(MonitorError::with_detail(
                MonitorErrorKind::PrivateKey,
                "私钥文件路径无效。",
                err.to_string(),
            ));
        }
    };

    if !full_path.is_file() {
        return Err(MonitorError::with_detail(
            MonitorErrorKind::PrivateKey,
            "私钥文件不存在。",
            full_path.display().to_string(),
        ));
    }

    if looks_like_putty_key(&full_path) {
        return Err(MonitorError::new(
            MonitorErrorKind::PrivateKey,
            "不支持 PuTTY .ppk 私钥，请先转换为 OpenSSH 格式。",
        ));
    }

    load_secret_key(&full_path, passphrase).map_err(|err| {
        let message = err.to_string();
        if message.to_lowercase().contains("permission") || message.to_lowercase().contains("denied")
        {
            MonitorError::with_detail(
                MonitorErrorKind::PrivateKey,
                "没有权限读取私钥文件。",
                message,
            )
        } else {
            MonitorError::with_detail(
                MonitorErrorKind::PrivateKey,
                "无法加载私钥，请检查格式和口令。",
                message,
            )
        }
    })
}

fn looks_like_putty_key(path: &Path) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    use std::io::Read;
    let mut buffer = [0u8; 64];
    let Ok(read) = file.read(&mut buffer) else {
        return false;
    };
    let header = String::from_utf8_lossy(&buffer[..read]);
    header.contains("PuTTY-User-Key-File")
}

fn map_connect_error(err: &russh::Error) -> MonitorError {
    let detail = err.to_string();

    if matches!(
        err,
        russh::Error::ConnectionTimeout | russh::Error::KeepaliveTimeout | russh::Error::InactivityTimeout
    ) {
        return MonitorError::with_detail(MonitorErrorKind::Timeout, "连接服务器超时。", detail);
    }

    if detail.to_lowercase().contains("auth") {
        return MonitorError::with_detail(
            MonitorErrorKind::Authentication,
            "SSH 认证失败，请检查用户名、密码或私钥。",
            detail,
        );
    }

    if detail.to_lowercase().contains("host key") {
        return MonitorError::with_detail(
            MonitorErrorKind::HostKey,
            "服务器主机密钥验证失败。",
            detail,
        );
    }

    if matches!(err, russh::Error::IO(_)) {
        return MonitorError::with_detail(
            MonitorErrorKind::Network,
            "无法连接服务器，请检查地址、端口和网络。",
            detail,
        );
    }

    MonitorError::with_detail(MonitorErrorKind::Unknown, "SSH 连接失败。", detail)
}

fn map_run_error(err: &russh::Error) -> MonitorError {
    let detail = err.to_string();
    match err {
        russh::Error::IO(_) | russh::Error::Disconnect | russh::Error::HUP => {
            MonitorError::with_detail(
                MonitorErrorKind::Network,
                "SSH 连接中断，正在重连。",
                detail,
            )
        }
        _ if detail.to_lowercase().contains("auth") => MonitorError::authentication(),
        _ => MonitorError::with_detail(MonitorErrorKind::Unknown, "监控失败，正在重试。", detail),
    }
}

#[derive(Debug, Clone)]
pub struct ConnectionTestResult {
    pub connected: bool,
    pub gpu_count: i32,
    pub ram_available: bool,
    pub message: String,
    pub fingerprint: Option<String>,
    pub error: Option<MonitorError>,
}

/// 与 C# ConnectionTester.TestAsync 对应：连接 + 探针一次，返回可展示的检测结果。
pub async fn test_connection(config: &ServerConfig) -> ConnectionTestResult {
    let mut session = SshSession::new(config.clone(), Box::new(|_| {}));

    if let Err(err) = session.connect().await {
        return ConnectionTestResult {
            connected: false,
            gpu_count: 0,
            ram_available: false,
            message: err.message.clone(),
            fingerprint: config.host_key_fingerprint_sha256.clone(),
            error: Some(err),
        };
    }

    let fingerprint = config.host_key_fingerprint_sha256.clone();

    let probe_result = session
        .run(&probe::build_main_command(), Duration::from_secs(20))
        .await;
    match probe_result {
        Ok(command_result) => {
            let probe = parser::parse_probe(&command_result.stdout);

            if probe.nvsmi_missing {
                return ConnectionTestResult {
                    connected: true,
                    gpu_count: 0,
                    ram_available: probe.ram.is_some(),
                    message: "SSH 连接成功，但服务器上未找到 nvidia-smi。请确认 NVIDIA 驱动已安装。"
                        .to_string(),
                    fingerprint,
                    error: Some(MonitorError::new(
                        MonitorErrorKind::NvidiaSmiMissing,
                        "未找到 nvidia-smi。",
                    )),
                };
            }

            if probe.gpus.is_empty() {
                return ConnectionTestResult {
                    connected: true,
                    gpu_count: 0,
                    ram_available: probe.ram.is_some(),
                    message: "SSH 连接成功，但没有读取到 GPU 信息。".to_string(),
                    fingerprint,
                    error: Some(MonitorError::with_detail(
                        MonitorErrorKind::Command,
                        "未读取到 GPU 信息。",
                        command_result.stderr,
                    )),
                };
            }

            let ram_text = if probe.ram.is_some() {
                "，内存信息可用"
            } else {
                ""
            };
            ConnectionTestResult {
                connected: true,
                gpu_count: probe.gpus.len() as i32,
                ram_available: probe.ram.is_some(),
                message: format!(
                    "连接成功：检测到 {} 块 NVIDIA GPU{ram_text}。",
                    probe.gpus.len()
                ),
                fingerprint,
                error: None,
            }
        }
        Err(err) => {
            let message = format!("SSH 连接成功，但读取 GPU 数据失败：{}", err.message);
            ConnectionTestResult {
                connected: true,
                gpu_count: 0,
                ram_available: false,
                message: message.clone(),
                fingerprint,
                error: Some(MonitorError::with_detail(
                    MonitorErrorKind::Command,
                    message,
                    err.message,
                )),
            }
        }
    }
}

/// 供监控循环复用：执行探针并组装 ServerSnapshot（对应 C# ServerMonitor.PollAsync）。
pub async fn poll_snapshot(session: &mut SshSession, config: &ServerConfig) -> MonResult<Arc<ServerSnapshot>> {
    let main_result = session
        .run(&probe::build_main_command(), Duration::from_secs(20))
        .await?;
    let probe_result = parser::parse_probe(&main_result.stdout);

    if probe_result.nvsmi_missing {
        return Err(MonitorError::with_detail(
            MonitorErrorKind::NvidiaSmiMissing,
            "未找到 nvidia-smi，请确认服务器已安装 NVIDIA 驱动。",
            main_result.stderr.trim().to_string(),
        ));
    }

    if probe_result.gpus.is_empty() {
        return Err(MonitorError::with_detail(
            MonitorErrorKind::Command,
            "未读取到 GPU 信息，请检查 NVIDIA 驱动和 nvidia-smi。",
            main_result.stderr.trim().to_string(),
        ));
    }

    let pids = probe_result.collect_gpu_pids();
    let mut ps_succeeded = true;
    let mut ps_records = std::collections::HashMap::new();
    if !pids.is_empty() {
        if let Some(ps_command) = probe::build_ps_command(&pids) {
            match session.run(&ps_command, Duration::from_secs(10)).await {
                Ok(ps_result) => {
                    // 与 C# 一致：退出码 0 或缺失都算成功。
                    ps_succeeded = ps_result.exit_status.unwrap_or(0) == 0;
                    if ps_succeeded {
                        let lines: Vec<String> =
                            ps_result.stdout.split('\n').map(str::to_string).collect();
                        ps_records = parser::parse_ps(&lines);
                    }
                }
                Err(err) => {
                    ps_succeeded = false;
                    crate::util::error(
                        &format!("服务器 {} 进程信息读取失败。", config.display_host()),
                        Some(&err.message),
                    );
                }
            }
        }
    }

    Ok(Arc::new(parser::build_snapshot(
        config.id,
        &config.username,
        probe_result,
        &ps_records,
        ps_succeeded,
        chrono::Utc::now(),
    )))
}
