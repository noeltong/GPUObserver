//! GPU 观察（Rust 版）：SSH 服务器 GPU/内存监控托盘应用。
//! 入口：参数解析（--autostart / gpuobserver:// 协议激活）→ 单实例 → tokio 后台运行时
//! → COM STA → 注册窗口类 → App 装配 → 消息循环。

// 无控制台窗口（GUI 子系统）；诊断信息走 %APPDATA%\GPUObserver\logs\app.log。
#![windows_subsystem = "windows"]

mod autostart;
mod config;
mod monitor;
mod secrets;
mod single_instance;
mod ssh;
mod ui;
mod util;

use std::sync::mpsc;
use std::sync::Arc;
use ui::app::{UiMsg, WM_APP_EVENT};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, CS_DROPSHADOW};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let start_hidden = args
        .iter()
        .any(|arg| arg.eq_ignore_ascii_case("--autostart"));
    let launch_argument = args
        .iter()
        .find(|arg| ui::toast::is_launch_argument(arg))
        .cloned();

    // 隐藏诊断模式：对每台服务器跑 探针+ps 全链路，结果写入 %APPDATA%\GPUObserver\diag.txt。
    if args.iter().any(|arg| arg.eq_ignore_ascii_case("--diag")) {
        run_diag();
        return;
    }

    // 单实例：次实例转发消息后退出（对应 C# Program.Main）。
    let instance = single_instance::SingleInstance::new(single_instance::INSTANCE_NAME);
    if !instance.is_primary {
        if !start_hidden {
            instance.send("show");
            // toast 点击通过协议激活拉起本进程：稍等并转发目标服务器。
            if let Some(argument) = &launch_argument {
                if let Some(server_id) = ui::toast::parse_launch_argument(argument) {
                    instance.send(&format!("open-server:{server_id}"));
                }
            }
        }
        return;
    }

    // tokio 后台运行时（SSH/监控循环）
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("无法创建 tokio 运行时");
    let runtime_handle = runtime.handle().clone();
    std::thread::spawn(move || {
        runtime.block_on(std::future::pending::<()>());
    });

    // COM STA（文件对话框 / Shell 快捷方式 / Toast）
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );
    }

    ui::gfx::startup_gdiplus();
    ui::dialog::init_common_controls();
    ui::toast::ensure_toast_registration();

    // 注册窗口类
    let classes: [(&str, ui::win::WndProcType, u32); 6] = [
        (ui::app::APP_WINDOW_CLASS, ui::app::app_wndproc, 0),
        (
            ui::app::POPUP_WINDOW_CLASS,
            ui::popup::popup_wndproc,
            CS_DROPSHADOW.0,
        ),
        (
            ui::app::CONTENT_WINDOW_CLASS,
            ui::content::content_wndproc,
            0,
        ),
        (ui::app::TOOLTIP_CLASS, ui::popup::tooltip_wndproc, 0),
        (
            ui::app::SETTINGS_WINDOW_CLASS,
            ui::settings::settings_wndproc,
            0,
        ),
        (
            ui::app::SERVER_EDIT_WINDOW_CLASS,
            ui::server_edit::server_edit_wndproc,
            0,
        ),
    ];
    for (name, proc, style) in classes {
        let ok = ui::win::register_class(name, proc, 0, style);
        if !ok {
            util::error(
                &format!("注册窗口类 {name} 失败（GetLastError={}）。", unsafe {
                    windows::Win32::Foundation::GetLastError().0
                }),
                None,
            );
        }
    }

    // UI 消息通道：事件/管道消息 → 主线程队列 + PostMessage 唤醒。
    let (ui_tx, ui_rx) = mpsc::channel::<UiMsg>();

    // HWND 不是 Send，用原始值跨线程传递。
    let app_hwnd_cell = Arc::new(std::sync::Mutex::new(None::<isize>));
    let event_sink = {
        let tx = ui_tx.clone();
        let hwnd_cell = app_hwnd_cell.clone();
        Arc::new(move |event: monitor::service::AppEvent| {
            if tx.send(UiMsg::Event(event)).is_ok() {
                if let Some(raw) = *hwnd_cell.lock().unwrap() {
                    unsafe {
                        let _ = PostMessageW(
                            Some(windows::Win32::Foundation::HWND(raw as *mut _)),
                            WM_APP_EVENT,
                            WPARAM(0),
                            LPARAM(0),
                        );
                    }
                }
            }
        })
    };
    let pipe_sink = {
        let tx = ui_tx.clone();
        let hwnd_cell = app_hwnd_cell.clone();
        Arc::new(move |message: String| {
            if tx.send(UiMsg::Pipe(message)).is_ok() {
                if let Some(raw) = *hwnd_cell.lock().unwrap() {
                    unsafe {
                        let _ = PostMessageW(
                            Some(windows::Win32::Foundation::HWND(raw as *mut _)),
                            WM_APP_EVENT,
                            WPARAM(0),
                            LPARAM(0),
                        );
                    }
                }
            }
        })
    };

    let app_result = ui::app::App::new(
        runtime_handle,
        start_hidden,
        launch_argument,
        ui_rx,
        event_sink,
    );
    let mut app = match app_result {
        Ok(app) => app,
        Err(err) => {
            util::error("应用启动失败。", Some(&err));
            show_startup_error(&err);
            return;
        }
    };

    *app_hwnd_cell.lock().unwrap() = Some(app.app_hwnd.0 as isize);
    instance.start_server(pipe_sink);

    ui::app::register_app(&mut app);

    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app.run();
    }))
    .ok();

    unsafe {
        windows::Win32::System::Com::CoUninitialize();
    }
}

fn show_startup_error(message: &str) {
    ui::dialog::message_box(
        windows::Win32::Foundation::HWND::default(),
        &ui::i18n::fmt(ui::i18n::t().startup_failed, &[("message", message)]),
        ui::i18n::t().app_name,
        ui::dialog::MessageBoxKind::Error,
        ui::dialog::MessageBoxButtons::Ok,
    );
}

/// 进程链路诊断：每台服务器依次 连接 → 探针 → 解析 → ps → 快照，
/// 全部中间量写入 diag.txt（含原始输出前几行），用于定位“进程不显示/不通知”。
fn run_diag() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("diag: tokio");
    let mut report = String::new();

    runtime.block_on(async {
        let load = config::load();
        report.push_str(&format!(
            "刷新间隔: {} 秒，共 {} 台服务器\n",
            load.config.refresh_seconds,
            load.config.servers.len()
        ));

        for server in &load.config.servers {
            report.push_str(&format!("\n===== server-{} =====\n", server.id));

            let mut session = ssh::SshSession::new(server.clone(), Box::new(|_| {}));
            if let Err(err) = session.connect().await {
                report.push_str(&format!("连接失败: kind={:?} {}\n", err.kind, err.message));
                continue;
            }
            report.push_str("连接: 成功\n");

            let main_result = match session
                .run(&ssh_probe_command(), std::time::Duration::from_secs(20))
                .await
            {
                Ok(result) => result,
                Err(err) => {
                    report.push_str(&format!("探针失败: kind={:?} {}\n", err.kind, err.message));
                    continue;
                }
            };

            let parsed = monitor::parser::parse_probe(&main_result.stdout);
            report.push_str(&format!(
                "GPU 数: {}  pmon可用: {}  nvsmi缺失: {}\n",
                parsed.gpus.len(),
                parsed.pmon_available,
                parsed.nvsmi_missing
            ));
            for gpu in &parsed.gpus {
                report.push_str(&format!(
                    "  GPU {} {}: util={:?} mem={:?}/{:?}\n",
                    gpu.index,
                    gpu.name,
                    gpu.utilization_percent,
                    gpu.memory_used_mib,
                    gpu.memory_total_mib
                ));
            }

            let apps_section = extract_section(&main_result.stdout, "@@APPS@@");
            let pmon_section = extract_section(&main_result.stdout, "@@PMON@@");
            report.push_str(&format!(
                "compute-apps: {} 行\n",
                apps_section
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .count()
            ));
            report.push_str(&format!(
                "pmon: {} 行\n",
                pmon_section
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .count()
            ));

            let pids = parsed.collect_gpu_pids();
            report.push_str(&format!(
                "收集到 PID: {} 个 → {:?}\n",
                pids.len(),
                &pids[..pids.len().min(20)]
            ));

            let mut ps_records = std::collections::HashMap::new();
            let mut ps_ok = true;
            if let Some(ps_command) = monitor::probe::build_ps_command(&pids) {
                match session
                    .run(&ps_command, std::time::Duration::from_secs(10))
                    .await
                {
                    Ok(ps) => {
                        ps_ok = ps.exit_status.unwrap_or(0) == 0;
                        let lines: Vec<String> =
                            ps.stdout.split('\n').map(str::to_string).collect();
                        report.push_str(&format!(
                            "ps: 退出码={:?} 输出 {} 行\n",
                            ps.exit_status,
                            ps.stdout.lines().count()
                        ));
                        if ps_ok {
                            ps_records = monitor::parser::parse_ps(&lines);
                        }
                    }
                    Err(err) => {
                        ps_ok = false;
                        report.push_str(&format!("ps 失败: kind={:?} {}\n", err.kind, err.message));
                    }
                }
            } else {
                report.push_str("无 PID，未运行 ps\n");
            }

            let matched = ps_records
                .values()
                .filter(|r| r.user == server.username)
                .count();
            report.push_str(&format!(
                "ps 记录: {} 条；匹配记录: {} 条\n",
                ps_records.len(),
                matched
            ));

            let snapshot = monitor::parser::build_snapshot(
                server.id,
                &server.username,
                parsed,
                &ps_records,
                ps_ok,
                chrono::Utc::now(),
            );
            report.push_str(&format!(
                "最终快照: 进程 {} 条  数据完整={}  警告={:?}\n",
                snapshot.processes.len(),
                snapshot.process_data_complete,
                snapshot.warning
            ));
        }
    });

    let path = util::app_data_dir().join("diag.txt");
    let _ = std::fs::write(&path, &report);
}

/// 从探针原始输出中截取某分段标记之后的内容（@@PMONRC@@ 或下一标记为止）。
fn extract_section(stdout: &str, marker: &str) -> String {
    let normalized = stdout.replace("\r\n", "\n");
    let Some(start) = normalized.find(marker) else {
        return String::new();
    };
    let rest = &normalized[start + marker.len()..];
    let end = rest
        .find("@@")
        .map(|i| {
            if rest[i..].starts_with("@@PMONRC@@") {
                i
            } else {
                i
            }
        })
        .unwrap_or(rest.len());
    rest[..end].to_string()
}

/// 与 probe.rs 相同的主探针命令（避免为诊断引入额外依赖）。
fn ssh_probe_command() -> String {
    monitor::probe::build_main_command()
}
