//! 界面文案双语表（简体中文 / English）与语言状态。
//! 中文列与 C# 版逐字一致；英文列为 Rust 版新增（有意偏差，由"语言"设置引入）。
//! `t()` 返回当前语言的整表；带参数的文案用 {name} 占位符 + [`fmt`] 填参。
//!
//! 后端（ssh.rs / parser.rs / config.rs）产生的固定中文文案在显示层经
//! [`localize_backend`] 映射，后端代码零改动；未收录的文案原样回退显示。

use std::sync::atomic::{AtomicU8, Ordering};

use crate::config::Language;

static LANG: AtomicU8 = AtomicU8::new(0);

pub fn lang() -> Language {
    if LANG.load(Ordering::Relaxed) == 1 {
        Language::En
    } else {
        Language::ZhCn
    }
}

pub fn set_lang(language: Language) {
    LANG.store(if language == Language::En { 1 } else { 0 }, Ordering::Relaxed);
}

/// 把模板里的 {name} 占位符替换为实参。
pub fn fmt(template: &str, args: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (key, value) in args {
        out = out.replace(&format!("{{{key}}}"), value);
    }
    out
}

pub struct Str {
    // ---- 通用 ----
    pub app_name: &'static str,
    // ---- 启动 / main.rs ----
    pub startup_failed: &'static str,
    pub err_create_main_window: &'static str,
    pub err_load_app_icon: &'static str,
    // ---- 托盘 ----
    pub tray_open_panel: &'static str,
    pub tray_refresh_now: &'static str,
    pub tray_settings_menu: &'static str,
    pub tray_exit: &'static str,
    pub tray_no_servers: &'static str,
    pub tray_servers: &'static str,
    pub tray_errors_suffix: &'static str,
    // ---- 相对时间（theme.format_relative）----
    pub rel_just_now: &'static str,
    pub rel_seconds: &'static str,
    pub rel_minutes: &'static str,
    pub rel_hours: &'static str,
    // ---- 消息框 ----
    pub msg_autostart_failed: &'static str,
    pub msg_save_failed: &'static str,
    pub msg_delete_server: &'static str,
    pub msg_delete_server_title: &'static str,
    pub msg_retrust_title: &'static str,
    pub msg_retrust_body: &'static str,
    // ---- 弹窗 popup ----
    pub popup_footer_no_servers: &'static str,
    pub popup_footer_last_update: &'static str,
    pub popup_footer_waiting: &'static str,
    pub popup_tip_add: &'static str,
    pub popup_tip_settings: &'static str,
    pub popup_tip_refresh: &'static str,
    // ---- 内容面板 content ----
    pub content_refresh_wait: &'static str,
    pub content_refresh_updated: &'static str,
    pub content_error_banner: &'static str,
    pub btn_retry: &'static str,
    pub content_ram: &'static str,
    pub content_vram: &'static str,
    pub content_util: &'static str,
    pub content_chart_sampling: &'static str,
    pub content_legend_5min: &'static str,
    pub content_processes_title: &'static str,
    pub content_processes_count: &'static str,
    pub content_processes_none: &'static str,
    pub content_processes_empty: &'static str,
    pub content_nodata_error: &'static str,
    pub content_nodata_stale: &'static str,
    pub content_nodata_connecting: &'static str,
    pub content_nodata_reading: &'static str,
    pub content_stale_body: &'static str,
    pub content_connecting_body: &'static str,
    pub content_first_body: &'static str,
    pub content_empty_title: &'static str,
    pub content_empty_body: &'static str,
    // ---- 状态 pill（theme.phase_label）----
    pub phase_online: &'static str,
    pub phase_connecting: &'static str,
    pub phase_error: &'static str,
    pub phase_stopped: &'static str,
    // ---- 通用按钮 ----
    pub btn_add_server: &'static str,
    pub btn_edit: &'static str,
    pub btn_delete: &'static str,
    pub btn_save: &'static str,
    pub btn_cancel: &'static str,
    pub btn_show: &'static str,
    // ---- 设置窗口 settings ----
    pub settings_window_title: &'static str,
    pub settings_heading: &'static str,
    pub settings_subtitle: &'static str,
    pub settings_group_servers: &'static str,
    pub settings_hint: &'static str,
    pub settings_group_general: &'static str,
    pub settings_refresh: &'static str,
    pub settings_unit_seconds: &'static str,
    pub settings_refresh_note: &'static str,
    pub settings_enable_notifications: &'static str,
    pub settings_autostart: &'static str,
    pub settings_language: &'static str,
    pub lang_zh: &'static str,
    pub lang_en: &'static str,
    pub settings_storage_note: &'static str,
    pub settings_list_notify_off: &'static str,
    pub settings_test_sent: &'static str,
    pub btn_send_test_notify: &'static str,
    // ---- 服务器编辑 server_edit ----
    pub se_add_title: &'static str,
    pub se_edit_title: &'static str,
    pub se_subtitle: &'static str,
    pub se_name: &'static str,
    pub se_name_cue: &'static str,
    pub se_host: &'static str,
    pub se_port: &'static str,
    pub se_username: &'static str,
    pub se_auth: &'static str,
    pub se_auth_password: &'static str,
    pub se_auth_key_file: &'static str,
    pub se_key_passphrase: &'static str,
    pub btn_browse: &'static str,
    pub se_notify_idle: &'static str,
    pub se_host_key: &'static str,
    pub se_fingerprint_placeholder: &'static str,
    pub btn_retrust: &'static str,
    pub btn_test_connection: &'static str,
    pub se_testing: &'static str,
    pub se_test_success: &'static str,
    pub se_test_ram_ok: &'static str,
    pub se_test_new_fingerprint: &'static str,
    pub se_test_retry_hint: &'static str,
    pub se_note: &'static str,
    pub se_err_saved_password: &'static str,
    pub se_err_saved_passphrase: &'static str,
    pub se_err_host_required: &'static str,
    pub se_err_username_required: &'static str,
    pub se_err_key_required: &'static str,
    pub se_err_key_missing: &'static str,
    pub se_err_ppk: &'static str,
    pub se_err_password_required: &'static str,
    pub se_pick_key_title: &'static str,
    pub se_filter_keys: &'static str,
    pub se_filter_all: &'static str,
    // ---- 通知 toast ----
    pub toast_gpu_joiner: &'static str,
    pub toast_idle_title: &'static str,
    pub toast_idle_metrics: &'static str,
    pub toast_idle_body: &'static str,
    pub toast_test_title: &'static str,
    pub toast_test_body_generic: &'static str,
    pub toast_test_body_server: &'static str,
}

pub const ZH: Str = Str {
    app_name: "GPU 观察",
    startup_failed: "GPU 观察启动失败：{message}",
    err_create_main_window: "无法创建主窗口",
    err_load_app_icon: "无法加载应用图标",
    tray_open_panel: "打开面板",
    tray_refresh_now: "立即刷新",
    tray_settings_menu: "设置…",
    tray_exit: "退出",
    tray_no_servers: "GPU 观察 · 未配置服务器",
    tray_servers: "GPU 观察 · {n} 台服务器",
    tray_errors_suffix: " · {n} 个错误",
    rel_just_now: "刚刚",
    rel_seconds: "{n} 秒前",
    rel_minutes: "{n} 分钟前",
    rel_hours: "{n} 小时前",
    msg_autostart_failed: "无法更新开机自启动设置，请检查当前用户权限。",
    msg_save_failed: "无法保存设置：{err}",
    msg_delete_server: "确定删除服务器 {server} 吗？",
    msg_delete_server_title: "删除服务器",
    msg_retrust_title: "重新信任主机密钥",
    msg_retrust_body: "清除已保存的主机密钥指纹后，下次连接会重新信任服务器提供的主机密钥。仅在确认服务器重装或密钥变更时执行。\r\n\r\n继续？",
    popup_footer_no_servers: "尚未添加服务器",
    popup_footer_last_update: "上次更新 {time}",
    popup_footer_waiting: "等待数据…",
    popup_tip_add: "添加服务器",
    popup_tip_settings: "设置",
    popup_tip_refresh: "立即刷新",
    content_refresh_wait: "每 {n} 秒刷新 · 等待首次数据",
    content_refresh_updated: "每 {n} 秒刷新 · 更新于 {time}",
    content_error_banner: "连接失败：{message}",
    btn_retry: "重试",
    content_ram: "内存",
    content_vram: "显存",
    content_util: "利用率",
    content_chart_sampling: "采样中…",
    content_legend_5min: "近 5 分钟",
    content_processes_title: "我的 GPU 进程",
    content_processes_count: "仅 SSH 用户 · {n}",
    content_processes_none: "无",
    content_processes_empty: "当前用户暂无 GPU 进程",
    content_nodata_error: "无法获取 GPU 数据",
    content_nodata_stale: "采样数据已超过 5 分钟",
    content_nodata_connecting: "正在连接服务器…",
    content_nodata_reading: "正在读取 GPU 数据…",
    content_stale_body: "已清除过期数据，正在等待新的采样。",
    content_connecting_body: "正在建立 SSH 连接并读取 nvidia-smi。",
    content_first_body: "首次采样通常只需要几秒钟。",
    content_empty_title: "还没有 GPU 服务器",
    content_empty_body: "添加一台 Linux + NVIDIA 服务器，开始监控 GPU 与内存。",
    phase_online: "已连接",
    phase_connecting: "连接中",
    phase_error: "连接失败",
    phase_stopped: "已停止",
    btn_add_server: "添加服务器",
    btn_edit: "编辑",
    btn_delete: "删除",
    btn_save: "保存",
    btn_cancel: "取消",
    btn_show: "显示",
    settings_window_title: "设置 - GPU 观察",
    settings_heading: "设置",
    settings_subtitle: "管理监控服务器与通知选项。每个服务器的通知开关在编辑服务器中设置。",
    settings_group_servers: "服务器",
    settings_hint: "提示：服务器标签显示服务器地址；通知按服务器单独开关。",
    settings_group_general: "常规",
    settings_refresh: "刷新间隔",
    settings_unit_seconds: "秒",
    settings_refresh_note: "2–300 秒；所有服务器共用。",
    settings_enable_notifications: "启用 GPU 空闲通知",
    settings_autostart: "开机时自动启动 GPU 观察",
    settings_language: "语言",
    lang_zh: "简体中文",
    lang_en: "English",
    settings_storage_note: "采样数据只在内存中保留最近 300 秒，并用于面板趋势图；不会写入磁盘。",
    settings_list_notify_off: "通知关闭",
    settings_test_sent: "已发送测试通知。",
    btn_send_test_notify: "发送测试通知",
    se_add_title: "添加服务器",
    se_edit_title: "编辑服务器",
    se_subtitle: "SSH 连接与密钥读取完全内置，不依赖系统 OpenSSH 或 ssh-agent。",
    se_name: "名称（可选）",
    se_name_cue: "留空则标签显示主机/IP",
    se_host: "主机 / IP",
    se_port: "端口",
    se_username: "用户名",
    se_auth: "认证方式",
    se_auth_password: "密码",
    se_auth_key_file: "私钥文件",
    se_key_passphrase: "私钥口令",
    btn_browse: "浏览…",
    se_notify_idle: "在 GPU 空闲时通知我（当前用户进程全部退出）",
    se_host_key: "主机密钥",
    se_fingerprint_placeholder: "首次连接时自动记录",
    btn_retrust: "重新信任",
    btn_test_connection: "测试连接",
    se_testing: "正在连接并读取 nvidia-smi…",
    se_test_success: "连接成功：检测到 {n} 块 NVIDIA GPU{ram}。",
    se_test_ram_ok: "，内存信息可用",
    se_test_new_fingerprint: "新指纹：SHA256:{fp}",
    se_test_retry_hint: " 可保存后稍后重试。",
    se_note: "密码与口令使用 Windows DPAPI 加密保存；私钥只保存路径，解锁后的私钥仅存在于当前进程内存中。",
    se_err_saved_password: "已保存的密码无法解密，请重新输入。",
    se_err_saved_passphrase: "已保存的私钥口令无法解密，请重新输入。",
    se_err_host_required: "请填写服务器主机或 IP。",
    se_err_username_required: "请填写 SSH 用户名。",
    se_err_key_required: "请选择私钥文件。",
    se_err_key_missing: "私钥文件不存在。",
    se_err_ppk: "不支持 PuTTY .ppk 私钥，请转换为 OpenSSH 格式。",
    se_err_password_required: "请填写登录密码。",
    se_pick_key_title: "选择 SSH 私钥文件",
    se_filter_keys: "私钥文件",
    se_filter_all: "所有文件",
    toast_gpu_joiner: "、",
    toast_idle_title: "GPU 空闲 · {server}",
    toast_idle_metrics: "当前利用率 {util}，显存 {used} / {total}。",
    toast_idle_body: "{gpus} 上的当前用户进程已全部退出。{metrics} 点击查看服务器详情。",
    toast_test_title: "GPU 观察测试通知",
    toast_test_body_generic: "通知已可用。当当前用户在该 GPU 上的进程全部退出时，会在这里提醒你。",
    toast_test_body_server: "通知已可用。{server} 上的 GPU 进程全部退出时，会在这里提醒你。",
};

pub const EN: Str = Str {
    app_name: "GPU Observer",
    startup_failed: "GPU Observer failed to start: {message}",
    err_create_main_window: "Failed to create the main window",
    err_load_app_icon: "Failed to load the app icon",
    tray_open_panel: "Open panel",
    tray_refresh_now: "Refresh now",
    tray_settings_menu: "Settings…",
    tray_exit: "Exit",
    tray_no_servers: "GPU Observer · no servers configured",
    tray_servers: "GPU Observer · {n} server(s)",
    tray_errors_suffix: " · {n} error(s)",
    rel_just_now: "just now",
    rel_seconds: "{n}s ago",
    rel_minutes: "{n}m ago",
    rel_hours: "{n}h ago",
    msg_autostart_failed: "Could not update the start-with-Windows setting. Check the current user's permissions.",
    msg_save_failed: "Could not save settings: {err}",
    msg_delete_server: "Delete server {server}?",
    msg_delete_server_title: "Delete server",
    msg_retrust_title: "Re-trust host key",
    msg_retrust_body: "After the saved host key fingerprint is cleared, the next connection will trust the host key presented by the server. Only do this when you are sure the server was reinstalled or its key changed.\r\n\r\nContinue?",
    popup_footer_no_servers: "No servers yet",
    popup_footer_last_update: "Last update {time}",
    popup_footer_waiting: "Waiting for data…",
    popup_tip_add: "Add server",
    popup_tip_settings: "Settings",
    popup_tip_refresh: "Refresh now",
    content_refresh_wait: "Refresh every {n}s · waiting for first data",
    content_refresh_updated: "Refresh every {n}s · updated at {time}",
    content_error_banner: "Connection failed: {message}",
    btn_retry: "Retry",
    content_ram: "RAM",
    content_vram: "VRAM",
    content_util: "Load",
    content_chart_sampling: "Sampling…",
    content_legend_5min: "Last 5 min",
    content_processes_title: "My GPU processes",
    content_processes_count: "SSH user only · {n}",
    content_processes_none: "None",
    content_processes_empty: "No GPU processes for the current user",
    content_nodata_error: "Cannot get GPU data",
    content_nodata_stale: "Sample data is older than 5 minutes",
    content_nodata_connecting: "Connecting to server…",
    content_nodata_reading: "Reading GPU data…",
    content_stale_body: "Stale data has been cleared; waiting for new samples.",
    content_connecting_body: "Establishing the SSH connection and reading nvidia-smi.",
    content_first_body: "The first sample usually takes only a few seconds.",
    content_empty_title: "No GPU servers yet",
    content_empty_body: "Add a Linux + NVIDIA server to start monitoring GPU and memory.",
    phase_online: "Connected",
    phase_connecting: "Connecting",
    phase_error: "Error",
    phase_stopped: "Stopped",
    btn_add_server: "Add server",
    btn_edit: "Edit",
    btn_delete: "Delete",
    btn_save: "Save",
    btn_cancel: "Cancel",
    btn_show: "Show",
    settings_window_title: "Settings - GPU Observer",
    settings_heading: "Settings",
    settings_subtitle: "Manage monitored servers and notification options. Per-server notification switches are set in Edit server.",
    settings_group_servers: "Servers",
    settings_hint: "Tip: server tabs show the server address; notifications are switched per server.",
    settings_group_general: "General",
    settings_refresh: "Refresh rate",
    settings_unit_seconds: "sec",
    settings_refresh_note: "2–300 seconds; shared by all servers.",
    settings_enable_notifications: "Enable GPU idle notifications",
    settings_autostart: "Start GPU Observer when Windows starts",
    settings_language: "Language",
    lang_zh: "简体中文",
    lang_en: "English",
    settings_storage_note: "Samples are kept in memory for the last 300 seconds for the panel trend chart; nothing is written to disk.",
    settings_list_notify_off: "Notifications off",
    settings_test_sent: "Test notification sent.",
    btn_send_test_notify: "Send test notification",
    se_add_title: "Add server",
    se_edit_title: "Edit server",
    se_subtitle: "SSH connection and key reading are fully built in — no system OpenSSH or ssh-agent needed.",
    se_name: "Name",
    se_name_cue: "Leave empty to show host/IP as the label",
    se_host: "Host / IP",
    se_port: "Port",
    se_username: "Username",
    se_auth: "Auth method",
    se_auth_password: "Password",
    se_auth_key_file: "Private key file",
    se_key_passphrase: "Key passphrase",
    btn_browse: "Browse…",
    se_notify_idle: "Notify me when the GPUs are idle (all current-user processes have exited)",
    se_host_key: "Host key",
    se_fingerprint_placeholder: "Recorded automatically on first connection",
    btn_retrust: "Re-trust",
    btn_test_connection: "Test connection",
    se_testing: "Connecting and reading nvidia-smi…",
    se_test_success: "Connected: {n} NVIDIA GPU(s) detected{ram}.",
    se_test_ram_ok: ", memory info available",
    se_test_new_fingerprint: "New fingerprint: SHA256:{fp}",
    se_test_retry_hint: " You can save it and retry later.",
    se_note: "Passwords and passphrases are encrypted with Windows DPAPI; only the key path is stored, and the unlocked key exists only in this process's memory.",
    se_err_saved_password: "The saved password could not be decrypted. Please re-enter it.",
    se_err_saved_passphrase: "The saved key passphrase could not be decrypted. Please re-enter it.",
    se_err_host_required: "Please enter the server host or IP.",
    se_err_username_required: "Please enter the SSH username.",
    se_err_key_required: "Please choose a private key file.",
    se_err_key_missing: "The private key file does not exist.",
    se_err_ppk: "PuTTY .ppk keys are not supported. Please convert to OpenSSH format.",
    se_err_password_required: "Please enter the login password.",
    se_pick_key_title: "Choose an SSH private key file",
    se_filter_keys: "Private key files",
    se_filter_all: "All files",
    toast_gpu_joiner: ", ",
    toast_idle_title: "GPU idle · {server}",
    toast_idle_metrics: "Utilization {util}, VRAM {used} / {total}.",
    toast_idle_body: "All current-user processes on {gpus} have exited. {metrics} Click to view server details.",
    toast_test_title: "GPU Observer test notification",
    toast_test_body_generic: "Notifications are ready. When all processes of the current user on a GPU have exited, you will be notified here.",
    toast_test_body_server: "Notifications are ready. When all GPU processes on {server} have exited, you will be notified here.",
};

/// 当前语言的文案表。字段齐全由编译器保证（两份 const 同构）。
pub fn t() -> &'static Str {
    if lang() == Language::En {
        &EN
    } else {
        &ZH
    }
}

/// 后端固定中文文案 → 当前语言。未命中原样返回（zh 模式恒原样）。
/// 键必须与 ssh.rs / parser.rs / config.rs 里的字面量保持一致；后端改文案时同步这里。
pub fn localize_backend(text: &str) -> String {
    if lang() == Language::ZhCn || text.is_empty() {
        return text.to_string();
    }
    if let Some(en) = backend_exact(text) {
        return en.to_string();
    }
    // 组合模式：内层递归翻译。
    if let Some(inner) = text.strip_prefix("SSH 连接成功，但读取 GPU 数据失败：") {
        return format!("SSH connected, but reading GPU data failed: {}", localize_backend(inner));
    }
    if let Some(label) = text
        .strip_prefix("无法解密已保存的")
        .and_then(|rest| rest.strip_suffix("，请在设置中重新输入。"))
    {
        let label_en = match label {
            "私钥口令" => "key passphrase",
            "登录密码" => "login password",
            other => other,
        };
        return format!("The saved {label_en} could not be decrypted. Please re-enter it in settings.");
    }
    text.to_string()
}

fn backend_exact(zh: &str) -> Option<&'static str> {
    Some(match zh {
        // ssh.rs 连接/会话
        "连接服务器超时。" => "Connection to the server timed out.",
        "服务器主机密钥已更改。为安全起见已阻断连接，请在设置中确认后再重新信任。" => {
            "The server's host key has changed. The connection was blocked for safety. Verify it in settings and then re-trust the host key."
        }
        "SSH 认证失败，请检查用户名、密码或私钥。" => "SSH authentication failed. Check the username, password, or private key.",
        "未填写登录密码。" => "The login password is not set.",
        "SSH 连接已断开。" => "The SSH connection was lost.",
        "采集命令超时，正在重试。" => "The probe command timed out; retrying.",
        "服务器主机密钥验证失败。" => "Server host key verification failed.",
        "无法连接服务器，请检查地址、端口和网络。" => "Cannot connect to the server. Check the address, port, and network.",
        "SSH 连接失败。" => "SSH connection failed.",
        "SSH 连接中断，正在重连。" => "The SSH connection was interrupted; reconnecting.",
        "监控失败，正在重试。" => "Monitoring failed; retrying.",
        // ssh.rs 私钥加载
        "未选择私钥文件。" => "No private key file selected.",
        "私钥文件路径无效。" => "The private key file path is invalid.",
        "私钥文件不存在。" => "The private key file does not exist.",
        "不支持 PuTTY .ppk 私钥，请先转换为 OpenSSH 格式。" => "PuTTY .ppk keys are not supported. Convert to OpenSSH format first.",
        "没有权限读取私钥文件。" => "No permission to read the private key file.",
        "无法加载私钥，请检查格式和口令。" => "Could not load the private key. Check its format and passphrase.",
        // ssh.rs 采集
        "未找到 nvidia-smi。" => "nvidia-smi was not found.",
        "未读取到 GPU 信息。" => "No GPU information was read.",
        "未找到 nvidia-smi，请确认服务器已安装 NVIDIA 驱动。" => "nvidia-smi was not found. Make sure the NVIDIA driver is installed on the server.",
        "未读取到 GPU 信息，请检查 NVIDIA 驱动和 nvidia-smi。" => "No GPU information was read. Check the NVIDIA driver and nvidia-smi.",
        // ssh.rs 测试连接
        "SSH 连接成功，但服务器上未找到 nvidia-smi。请确认 NVIDIA 驱动已安装。" => {
            "SSH connected, but nvidia-smi was not found on the server. Make sure the NVIDIA driver is installed."
        }
        "SSH 连接成功，但没有读取到 GPU 信息。" => "SSH connected, but no GPU information was read.",
        // parser.rs 警告（snapshot.warning）
        "未找到 nvidia-smi，无法读取 GPU 信息。" => "nvidia-smi was not found; GPU information cannot be read.",
        "无法获取完整的 GPU 进程列表（pmon 不可用），空闲通知已暂停。" => {
            "The full GPU process list is unavailable (pmon unavailable); idle notifications are paused."
        }
        "无法解析 GPU 进程所属用户，空闲通知已暂停。" => "Could not resolve the owners of GPU processes; idle notifications are paused.",
        // config.rs 加载失败
        "配置文件无法读取，已备份并使用默认设置。" => {
            "The configuration file could not be read. It has been backed up and default settings are being used."
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 语言是进程级全局状态：串行化所有触碰它的测试，避免并行竞态。
    static LANG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn zh_mode_returns_original() {
        let _guard = LANG_LOCK.lock().unwrap();
        set_lang(Language::ZhCn);
        assert_eq!(localize_backend("SSH 连接已断开。"), "SSH 连接已断开。");
        assert_eq!(localize_backend("未知的新文案"), "未知的新文案");
    }

    #[test]
    fn en_mode_maps_exact_and_composites() {
        let _guard = LANG_LOCK.lock().unwrap();
        set_lang(Language::En);
        assert_eq!(localize_backend("SSH 连接已断开。"), "The SSH connection was lost.");
        assert_eq!(
            localize_backend("SSH 连接成功，但读取 GPU 数据失败：采集命令超时，正在重试。"),
            "SSH connected, but reading GPU data failed: The probe command timed out; retrying."
        );
        assert_eq!(
            localize_backend("无法解密已保存的私钥口令，请在设置中重新输入。"),
            "The saved key passphrase could not be decrypted. Please re-enter it in settings."
        );
        assert_eq!(
            localize_backend("无法解密已保存的登录密码，请在设置中重新输入。"),
            "The saved login password could not be decrypted. Please re-enter it in settings."
        );
        assert_eq!(localize_backend("未来新增的未知文案"), "未来新增的未知文案");
        set_lang(Language::ZhCn);
    }

    #[test]
    fn fmt_replaces_placeholders() {
        assert_eq!(fmt("每 {n} 秒 · {time}", &[("n", "5"), ("time", "12:00")]), "每 5 秒 · 12:00");
        assert_eq!(fmt("无占位符", &[]), "无占位符");
    }
}
