# GPU 观察（Rust 版）— 工作区说明

Windows 托盘应用：经 SSH 监控多台 Linux+NVIDIA 服务器的 GPU/内存。它是 C# 版（`<CSharp_reference_repository>`，参照实现，改动 UI/后端行为前先对照那里的源码）的 1:1 重写：**界面尺寸、行为、配置文件必须与 C# 版保持互操作**。产品文档见 `README.md`。

## 构建与测试

```bash
export PATH="$PATH:$HOME/.cargo/bin"   # Git Bash 里 cargo 不在 PATH，必须先加
cargo build --release    # 产物 target/release/GPUObserver.exe（单文件 GUI 程序）
cargo test               # 34 个单元测试（从 C# xUnit 移植 + 配置语言/i18n 映射测试）
```

- 依赖 `windows` crate 0.62；rc.exe（Windows SDK）用于 `build.rs` 嵌入图标+清单，缺失会构建失败。
- `russh` 必须用 ring 后端（`default-features=false, features=["ring","rsa","flate2"]`）——默认的 aws-lc 需要 NASM，本机没装。

## 架构与分层

- **后端**（可在任意线程跑）：`config.rs`（serde JSON，PascalCase）｜`secrets.rs`（DPAPI）｜`ssh.rs`（russh：TOFU 指纹/密码+keyboard-interactive/私钥）｜`monitor/`（`probe.rs` 探针命令、`parser.rs` 解析、`queue.rs` 300s/512 采样队列、`notify_sm.rs` 空闲状态机、`monitor.rs` 每服务器轮询任务、`service.rs` 事件分发）。
- **UI**（全部在主线程）：`ui/`（`gfx.rs` GDI+/GDI 封装、`theme.rs` 颜色/字体/格式化、`i18n.rs` 中英双语文案表+语言状态、`popup.rs`+`content.rs` 弹窗与内容面板、`tray.rs` 托盘、`dialog.rs`/`settings.rs`/`server_edit.rs` 对话框、`app.rs` 调度器）。
- **线程/事件流**：tokio 运行时驻后台线程跑 SSH；监控事件经 `service.rs` 的 EventSink → `std::mpsc` + `PostMessageW(WM_APP_EVENT)` 唤醒主线程，由 `app.rs` `drain_messages` 消费。窗口状态用 `GWLP_USERDATA` 裸指针存 Box；跨窗口调用一律走 `ui::app::with_app(...)`（thread_local）。

## 与 C# 版互操作的红线（改这些会破坏无缝替代）

- 配置 `%APPDATA%\GPUObserver\config.json` 与 C# 共用：字段 PascalCase、枚举序列化为字符串、原子写（tmp+rename）。Rust 版新增 `Language` 字段（"zh-CN"/"en-US"）：C# 读取时忽略未知字段（安全），但 C# 保存配置会丢掉它；默认 zh-CN 时该键不写出。
- DPAPI 熵 `GPUObserver.Secret.v1`（CurrentUser，flags 0）不可改，否则双方密码互相读不了。
- 主机密钥指纹 = OpenSSH 风格 base64(SHA256(公钥 wire 编码))，**去掉尾部 `=` 填充**（SSH.NET 的习惯，russh 侧要 `.trim_end_matches('=')`）。
- 单实例互斥量 `Local\GPUObserver.Instance.v1` 与 C# 同名——两个版本不能同时运行（测试 GUI 前先 tasklist 确认）。
- `monitor/probe_main.sh` 是 nvidia-smi 探针脚本的唯一来源（`include_str!`），必须与 C# `ProbeCommand.MainScript` 逐字一致。
- **对 C# 的有意偏差**：ps 命令用 `ps -o pid=,user:32=,comm=`（C# 是 `user=`）——ps 默认 user 列宽 8 字符会把长用户名截断成 `longuser+`，导致进程归属匹配失败、无进程显示且不通知（用户三台服务器实机踩过）。改探针命令前先想清楚。
- `--diag` 隐藏启动参数：对每台服务器跑 连接→探针→ps→快照 全链路，结果写 `%APPDATA%\GPUObserver\diag.txt`——排查"进程/通知不工作"先用它。
- UI 固定尺寸（逻辑像素）：弹窗 520×715（空态 520×300）、GPU 卡高 158、设置窗 680×614（原 680×580，新增"语言"行所致）、服务器编辑 600×588。
- 内容窗口的滚动条必须来自 WS_VSCROLL 样式（`ShowScrollBar` 只能开关样式里存在的条）；WS_VSCROLL 窗口的 GetClientRect 已排除滚动条区域，绘制时不要再手工扣宽度。

## 已知坑

- `windows` crate 0.62 符号位置反直觉：`InvalidateRect` 在 `Gdi`；`SetFocus`/`TrackMouseEvent`/`EnableWindow` 在 `UI::Input::KeyboardAndMouse`；`WM_MOUSELEAVE`/`SetScrollInfo`/`ShowScrollBar`/`EM_SETPASSWORDCHAR` 在 `UI::Controls`；`PROPERTYKEY` 在 `Foundation`；`ReadFile`/`WriteFile`/`ConnectNamedPipe` 需要 `Win32_System_IO` feature。新增 API 前先查 crate 源码位置。
- `main.rs` 的 `#![windows_subsystem = "windows"]` 不能删，否则运行会弹控制台窗口。
- GUI 子系统程序没有控制台输出，诊断全靠 `%APPDATA%\GPUObserver\logs\app.log`（`util.rs`，1MB 轮转）；托盘添加失败/首次定位/首次绘制都有日志，排查 UI 问题先读它。
- Win11 会把新 exe 路径的托盘图标收进溢出区（^ 展开处），不是 bug。

## 约定

- 用户名归属判定（进程是否"我的"）：ps 输出 user 与 SSH 用户名**区分大小写**比较（与 C# Ordinal 一致）。
- 数字解析走 invariant，`[N/A]`/`Not Supported`/`-` → null，Long 用银行家舍入（`round_ties_even`）——见 `parser.rs`，勿"简化"。
- 采样数据只在内存（300s/512 条），不落盘；除 config 外不写任何文件。
- 界面文案走 `ui/i18n.rs` 双语表（`t()` 取表 + `fmt` 填 `{name}` 占位符）：中文列与 C# 版逐字一致，英文列为 Rust 版新增；**新增界面文案必须同时补 ZH/EN 两个 const**（漏一个即编译错误）。语言存 config 的 `Language` 字段，设置保存后经 `save_and_apply` 即时生效（自绘窗口重绘；对话框/托盘菜单/toast 按需重建，天然生效）。
- 后端（ssh.rs/parser.rs/config.rs）产生、直接上屏的固定中文文案，在显示层经 `i18n::localize_backend` 映射；**改后端这些文案时必须同步 i18n.rs 的映射表**（未收录的文案在英文界面回退显示中文原文，不报错）。日志/`--diag`/`.lnk` 文件名/注册表 AUMID 保持中文不动。
