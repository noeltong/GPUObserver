[English](README.md) | 简体中文

# GPU 观察（Rust 版）

Windows 系统托盘应用：通过 SSH 实时监控多台 Linux + NVIDIA 服务器的 GPU 与内存。
本仓库是 `gpu_observe_app`（C# / .NET 8 WinForms 版，独立本地仓库、未公开）的 **Rust 重写**，
界面尺寸、视觉与行为与 C# 版 1:1 对应，配置文件完全互通，可直接替代旧版运行。

## 功能

- **托盘常驻**：左键托盘图标弹出监控面板（贴任务栏角落）；右键弹出菜单（打开面板 / 立即刷新 / 设置… / 退出）。
- **界面语言**：设置 → 常规 → 语言，简体中文 / English 切换，保存后立即生效（无需重启），随配置持久化。
- **多服务器选项卡**：面板顶部每个服务器一个页签（显示状态圆点：绿=已连接、黄=连接中、红=错误）。
- **GPU 监控**：显存占用（紫/黄/红分级）、利用率、温度、功耗，以及近 5 分钟双线走势图（利用率面积图 + 显存折线）。
- **内存监控**：服务器 RAM 使用率卡片。
- **我的 GPU 进程**：仅显示以 SSH 登录用户身份运行的 GPU 进程（PID / 命令 / 显存）。
- **GPU 空闲通知**：当某块 GPU 上当前用户进程全部退出时弹系统通知（全局 + 按服务器开关）。
- **安全**：SSH 连接与密钥解析完全内置（russh，不依赖 ssh.exe/ssh-agent）；密码与口令用 Windows DPAPI（CurrentUser）加密保存；主机密钥 TOFU（首次信任后锁定，变更即阻断，可"重新信任"）。
- 其他：开机自启动（HKCU Run）、单实例（二次启动唤起主窗口）、Toast 点击直达对应服务器页签。

## 与 C# 版的对应关系

| C#（gpu_observe_app） | Rust（本仓库） |
| --- | --- |
| WinForms + GDI+ 自绘 | Win32 + GDI+（`windows` crate）手绘，PerMonitorV2 DPI |
| SSH.NET 2026.0 | russh 0.64（ring 后端） |
| System.Text.Json 配置 | serde_json，同一 `%APPDATA%\GPUObserver\config.json` |
| ProtectedData (DPAPI) | `CryptProtectData/CryptUnprotectData`，熵 `GPUObserver.Secret.v1`（互读，已实测） |
| UWP Toolkit Toast | WinRT Toast + `gpuobserver://` 协议激活 |
| MonitorService / ServerMonitor | `monitor/service.rs` / `monitor/monitor.rs`（tokio 任务） |
| NotificationStateMachine | `monitor/notify_sm.rs`（逐行等价） |
| SampleQueue（300s/512） | `monitor/queue.rs` |
| nvidia-smi 探针 | `monitor/probe_main.sh`（逐字相同，base64 + `/bin/sh` 下发） |

### 尺寸对照（逻辑像素 @96dpi，随 DPI 缩放）

| 窗口 | 尺寸 |
| --- | --- |
| 托盘弹窗 | **520 × 715**（未配置服务器时 520 × 300），圆角 10 |
| GPU 卡片 | 高 158（走势图 44） |
| 内存卡片 | 高 52（进度条高 6） |
| 设置窗口 | 客户区 **680 × 614** |
| 添加/编辑服务器 | 客户区 **600 × 588** |

## 构建与运行

依赖：Rust（MSVC 工具链）+ Windows SDK（rc.exe，用于嵌入图标与清单）。

```powershell
cargo build --release
# 产物：target\release\GPUObserver.exe（单文件，约 4.8 MB）
```

首次运行会自动弹出"添加服务器"对话框。已用 C# 版的机器无需任何迁移：
关闭旧版后直接运行本程序即可读取原有配置（密码 / 主机密钥指纹直接继承）。

> 单实例互斥量与 C# 版同名（`Local\GPUObserver.Instance.v1`），两个版本不能同时运行。

## 行为细节（与 C# 版对齐）

- 刷新间隔全局共享，默认 5 秒（2–300 可配）；轮询完全后台，弹窗关闭不停。
- 失败退避 5s → ×2 → 60s 封顶；认证 / 主机密钥 / 凭据 / 私钥类错误固定 60s。
- 每台服务器一条长连接（keepalive 30s）；探针主命令超时 20s，ps 超时 10s，连接超时 10s。
- 进程归属判定：`ps` 输出的用户名与 SSH 用户名 **区分大小写** 精确相等。
- pmon 不可用或 ps 失败时该轮样本标记"不完整"，空闲通知暂停并重新播种。
- 样本只在内存保留最近 300 秒（上限 512 条），不落盘。
- 弹窗失焦即隐藏；Esc 关闭；300ms 内托盘左键不会重新弹开。

## 测试

移植了 C# xUnit 全套后端测试（解析 / 队列 / 状态机 / 配置 / DPAPI）：

```powershell
cargo test   # 34 个单元测试
```

## 已知差异

- 托盘右键菜单为系统原生样式（C# 为自绘 ContextMenuStrip，字体相同）。
- Toast 点击激活经 `gpuobserver://` 协议（C# 经 COM activator），行为等价：都会打开弹窗并切到对应服务器页签。
- 空状态的大图标直接使用 exe 内嵌图标（同 C#）。

## 许可证

本项目采用 [MIT License](LICENSE) 发布。© 2026 Noel Tong
