# GPU 观察（GPUObserver）

一个常驻 Windows 系统托盘的 GPU 监控工具。通过 SSH 连接多台 Linux + NVIDIA 服务器，在贴近托盘的气泡面板中按服务器标签展示 GPU 利用率、显存、温度、功耗与服务器内存，并可在当前用户的 GPU 进程全部退出时发送 Windows 通知。

- 平台：Windows 10 1809+ / Windows 11，x64
- 目标服务器：Linux + NVIDIA（依赖 `nvidia-smi`）
- 界面：简体中文，浅色 Windows 11 风格
- 数据：只在内存中保留最近 300 秒，不写入磁盘历史
- SSH：完全内置，不调用系统 `ssh.exe`，不依赖 ssh-agent

## 功能特性

| 功能 | 说明 |
| --- | --- |
| 多服务器 | 每台服务器一个标签，显示主机/IP 和连接状态点，标签过多时可用左右箭头翻页 |
| 定时刷新 | 后台常驻刷新，默认 5 秒，可在设置中调整 2–300 秒 |
| GPU 指标 | 利用率、显存已用/总量与百分比、温度、功耗 |
| 服务器内存 | 已用/总量与百分比（服务器无 `free` 时隐藏） |
| 我的进程 | 当前 SSH 用户在每块 GPU 上的进程、PID、命令和显存占用 |
| 300 秒趋势 | 每块 GPU 的利用率与显存双线迷你趋势图 |
| GPU 空闲通知 | 当前用户在某块 GPU 上的进程全部退出时通知，可在全局和单服务器维度开关 |
| 托盘交互 | 左键打开面板；右键菜单包含打开面板、立即刷新、设置和退出 |
| 首次运行 | 自动显示面板并打开“添加服务器” |
| 开机自启 | 可选的 HKCU Run 自启动，不需要管理员权限 |
| 单实例 | 重复启动会唤起已有实例，不会出现多个托盘图标 |

## 界面与操作

- **左键点击托盘图标**：打开或关闭气泡面板。
- **右键点击托盘图标**：打开面板、立即刷新、设置…、退出。
- **服务器标签**：显示主机/IP（非 22 端口时显示 `主机:端口`），左侧圆点表示在线、连接中或错误。
- **面板内容**：服务器状态、内存、GPU 卡片、300 秒双线趋势、当前用户的 GPU 进程。
- **设置入口**：面板右上角齿轮或托盘右键菜单。
- **关闭面板**：点击面板外部或按 `Esc`。

> Windows 11 默认会把新托盘图标收进隐藏图标区。点击任务栏时钟旁的 `^`，找到“GPU 观察”，可将其拖到任务栏固定显示。

## 系统要求

### 运行

- Windows 10 1809+ 或 Windows 11，x64
- [.NET 8 Desktop Runtime](https://dotnet.microsoft.com/download/dotnet/8.0)（发布版为框架依赖部署）
- 目标服务器：
  - Linux，OpenSSH Server 可登录
  - NVIDIA 驱动与 `nvidia-smi`
  - 标准 `ps`；`free` 可选
  - 一个可正常登录的 SSH 账号
  - 登录 shell 可以是 bash、zsh、fish 等；采集脚本通过 `/bin/sh` 执行

程序是每用户应用，不需要管理员权限。目标服务器不需要安装额外 Agent。

### 构建

- Windows（WinForms 项目）
- .NET 8 SDK
- 可访问 NuGet 的网络

主要 NuGet 依赖：`SSH.NET 2026.0.0`、`Microsoft.Toolkit.Uwp.Notifications 7.1.3`、`System.Security.Cryptography.ProtectedData 8.0.0`；测试使用 xUnit。

## 快速开始

### 使用已发布版本

1. 保持 `publish\GPUObserver` 整个目录完整，双击其中的 `GPUObserver.exe`。依赖 DLL 与 exe 同目录，不能只复制 exe。
2. 首次启动会自动显示空面板并弹出“添加服务器”。
3. 填写服务器信息并保存后，对应标签开始定时刷新。
4. 如果托盘图标没有出现在任务栏，请从 Windows 11 的隐藏图标区拖出。

> Git 仓库不包含 `publish/`（构建产物被 `.gitignore` 排除）。如果克隆后该目录不存在，请按下一节从源码构建。

### 从源码构建与运行

在仓库根目录执行：

```bash
dotnet build GPUObserver.sln
dotnet run --project GPUObserver/GPUObserver.csproj
```

运行测试：

```bash
dotnet test GPUObserver.Tests/GPUObserver.Tests.csproj
```

发布框架依赖版本（推荐，体积小）：

```bash
dotnet publish GPUObserver/GPUObserver.csproj -c Release -r win-x64 --self-contained false -o publish/GPUObserver
```

如果目标机器没有 .NET 8 Desktop Runtime，可发布自包含的**文件夹**版本：

```bash
dotnet publish GPUObserver/GPUObserver.csproj -c Release -r win-x64 --self-contained true -p:PublishSingleFile=false -o publish/GPUObserver-portable
```

> 不要使用 single-file 或裁剪发布：Windows 通知依赖磁盘上的程序集与 COM 激活注册，单文件容易破坏通知点击激活。

## 添加服务器

可以在三个位置添加服务器：首次运行的自动弹窗、设置中的“添加服务器”、面板标签旁的 `+`。

需要填写：

- **主机 / IP**：服务器地址或域名
- **端口**：默认 22
- **用户名**：SSH 登录用户
- **认证方式**
  - **密码**：支持密码认证和键盘交互认证回退
  - **私钥文件**：支持 OpenSSH/PEM 私钥，可选私钥口令；不支持 PuTTY `.ppk`，请先转换为 OpenSSH 格式
- **通知开关**：是否对该服务器启用 GPU 空闲通知

建议先点“测试连接”。测试会验证 SSH 登录、主机密钥、`nvidia-smi` 和内存信息，并报告可读的中文错误。

### 关于公钥认证

本程序不会生成密钥对，也不连接系统 ssh-agent。请把已有密钥对应的**公钥**加入服务器的 `~/.ssh/authorized_keys`，然后在程序中选择对应**私钥文件**。如果私钥原本在 ssh-agent 中，请先导出为文件。

### 主机密钥

首次连接会记录服务器的 SHA-256 主机密钥指纹（TOFU）。指纹变化时连接会被阻断，并提示你确认。若确认是服务器重装或主动更换密钥，请在“编辑服务器”中点击“重新信任”，再重新连接。

## GPU 空闲通知

通知条件为：**同一块 GPU 上，当前 SSH 登录用户的进程数从大于 0 变为 0**。

- 首次成功采样只用于建立状态，不会立即通知。
- 同一轮采样中多块 GPU 同时结束会合并为一条通知。
- 如果进程数据不完整（例如 `nvidia-smi pmon` 或 `ps` 失败），不会触发通知，面板会显示暂停原因。
- 通知受全局开关和单服务器开关共同控制。
- 点击通知会打开程序并定位到对应服务器标签。
- 设置中提供“发送测试通知”。

程序只在运行期间监控；退出后不会再产生新的空闲通知。系统级通知受 Windows“通知”开关和“勿扰/专注助手”影响。

## 数据保留与隐私

- 每台服务器一个内存 FIFO 队列，采样最多保留 **300 秒**；过期数据在入队、存取和后台定时清理时删除。
- 队列最多 512 条，防止异常时钟或手动刷新导致无限增长。
- 300 秒数据只用于面板趋势图；不写入磁盘、不写日志、不导出。
- 面板只从队列读取；最新数据超过 300 秒时不再显示旧指标。
- 退出程序后，所有采样立即消失。
- GPU/内存/进程采样不会写入日志；日志只记录错误与关键事件。
- 进程明细同样只存在于内存采样中，最多随采样保留 300 秒。

> 采样数据不写入磁盘；`%APPDATA%` 下只保存配置和错误日志，也不会将服务器配置放进 Git 仓库。

## 配置与凭据

| 内容 | 位置 |
| --- | --- |
| 配置 | `%APPDATA%\GPUObserver\config.json` |
| 日志 | `%APPDATA%\GPUObserver\logs\app.log` |
| 界面语言 | 简体中文 |
| 发布物 | `publish\GPUObserver\`（被 `.gitignore` 排除） |

配置内容：

- 主机/IP、端口、用户名、认证方式：明文
- 密码、私钥口令：使用 Windows DPAPI（CurrentUser）加密
- 固定后的主机密钥 SHA-256 指纹

DPAPI 凭据只能在原 Windows 用户环境下解密；换用户或换机器后需要重新输入密码/口令。若配置文件损坏，程序会备份原文件并使用默认设置启动。

添加服务器后，配置文件位于仓库外部，因此不会被 Git 追踪。不要手动把 `config.json` 复制进仓库。

## 安全设计

- SSH 传输与密钥解析完全由内置托管 SSH 引擎完成，不调用系统 OpenSSH 二进制、不依赖系统 ssh-agent。
- 支持密码认证与键盘交互认证回退；支持 OpenSSH/PEM 私钥；不支持 PuTTY `.ppk`。
- 密码与私钥口令以 DPAPI 加密保存；日志不记录凭据。
- 主机密钥采用 TOFU 固定，变更时阻断并允许在人工确认后重新信任。
- 远程采集脚本是固定静态命令，服务器地址、用户名等配置不会拼接进 shell 命令；PID 仅接受数字。
- 程序私钥文件只保存路径；每次连接在应用内读取，不复制私钥文件，不上传到任何云端。

## 工作原理

```text
Windows 托盘 / 气泡面板 (WinForms)
        │
        ▼
MonitorService ──► ServerMonitor（每台服务器一个后台循环）
        │                    │
        │                    ├─ SSH.NET：密码 / OpenSSH 私钥
        │                    ├─ 固定采集脚本：nvidia-smi / pmon / compute-apps / ps / free
        │                    └─ 解析器：GPU、进程、内存
        │
        ├─ SampleQueue：最近 300 秒纯内存队列
        ├─ NotificationStateMachine：单卡进程退出状态机
        └─ ToastService：Windows 通知，失败时回退托盘气泡
```

> SSH 服务端会用登录 shell 解析执行请求。为避免 fish 等非 POSIX shell 无法执行采集脚本，程序会把静态 POSIX 脚本进行 base64 编码后交给 `/bin/sh` 执行；`ps` 命令也通过 `env` 设置语言环境，不依赖登录 shell 的语法。

### 目录结构

```text
GPUObserver.sln
GPUObserver/
  App/              托盘上下文、单实例、开机自启
  Config/           配置模型、原子读写、DPAPI 加密
  Monitoring/       监控服务、服务器循环、300 秒队列、通知状态机
  Monitoring/Parsing/ nvidia-smi / pmon / ps / free 解析
  Ssh/              内置 SSH 会话、私钥加载、连接测试
  Notifications/    Windows 通知与点击激活
  UI/               气泡面板、标签、GPU 卡片、趋势图、设置与服务器对话框
  Util/             最小化日志
  Assets/app.ico
GPUObserver.Tests/   解析、队列、通知、DPAPI、私钥、渲染冒烟测试
tools/make-icon.ps1 生成应用图标
```

关键实现：

- [SampleQueue.cs](GPUObserver/Monitoring/SampleQueue.cs)：300 秒内存队列与淘汰
- [ServerMonitor.cs](GPUObserver/Monitoring/ServerMonitor.cs)：连接、采集、重连与退避
- [NotificationStateMachine.cs](GPUObserver/Monitoring/NotificationStateMachine.cs)：单卡进程退出通知
- [ServerContentPanel.cs](GPUObserver/UI/ServerContentPanel.cs)：GPU 卡片、进程列表与趋势图渲染
- [PrivateKeyLoader.cs](GPUObserver/Ssh/PrivateKeyLoader.cs)：OpenSSH/PEM 私钥加载

## 测试

```bash
dotnet test GPUObserver.Tests/GPUObserver.Tests.csproj
```

测试覆盖：

- `nvidia-smi` CSV、pmon、compute-apps、ps、free 与引号/逗号解析
- 300 秒队列淘汰、硬上限、过期数据不残留
- 通知状态机的初始化、忙→闲、无效数据抑制、300 秒重置、多卡合并
- DPAPI 加解密
- OpenSSH/PEM 私钥读取与 `.ppk` 拒绝
- GPU 卡片、内存、进程与双线趋势的渲染冒烟测试（会在系统临时目录生成预览 PNG）

真实服务器上的 SSH 认证、主机密钥变更、真实 `nvidia-smi` 数据、网络中断重连和实际进程退出通知，需要使用你自己的 Linux + NVIDIA 服务器进行端到端验证。

## 已知限制

- 仅支持 Linux + NVIDIA；不支持 AMD ROCm、Windows SSH 服务器。
- 仅统计 SSH 登录用户自己的 GPU 进程；通过 `sudo` 切换到其他用户的任务不会被计入。
- 不支持 PuTTY `.ppk`；不支持系统 ssh-agent。
- 不支持 GPU 历史归档、导出或云端同步；最多保留 300 秒内存数据。
- 不提供安装包、代码签名、自动更新或暗色主题。
- 面板趋势图依赖采样连续性；刚启动时“采样中…”是正常状态。
- 部分驱动或权限环境可能不支持 `nvidia-smi pmon`，此时进程列表可能不完整，空闲通知会暂停，面板会显示警告。
- 通知遵循 Windows 的通知与勿扰设置，不会绕过系统静音策略。
- 自启动和通知注册与程序路径相关；移动发布目录后需重新开启自启动，并在新路径首次显示通知时重建通知注册。
- 高 DPI、多显示器、Windows 10 下的完整兼容性尚未做全面回归，当前主要在 Windows 11 x64 环境验证。

## 常见问题

**任务栏找不到图标？**  
Windows 11 会将新图标放入隐藏图标区。点击任务栏时钟旁的 `^`，把“GPU 观察”拖到任务栏。

**测试通知没有出现？**  
检查 Windows 设置中的通知开关和勿扰模式；也可在设置中再次点“发送测试通知”。程序在通知 API 异常时会回退为托盘气泡。

**连接测试失败？**  
依次检查 IP/端口、SSH 用户名、密码或私钥文件、私钥口令、服务器防火墙，以及 `authorized_keys` 是否配置了对应公钥。程序只支持 OpenSSH/PEM 私钥，不支持 `.ppk`。

**提示主机密钥已更改？**  
在“编辑服务器”中确认后点击“重新信任主机密钥”，再重新连接。请只在确认服务器重装或主动换钥时这样做。

**GPU 数据为空？**  
确认服务器已安装 NVIDIA 驱动，且当前用户能执行 `nvidia-smi`。程序会尝试常见安装路径。

**进程列表为空或通知暂停？**  
通常是 `nvidia-smi pmon` 或 `ps` 不可用/权限不足。GPU 与内存数据仍可显示，但为避免误报，空闲通知会暂停。

**如何重置配置？**  
退出程序，删除 `%APPDATA%\GPUObserver\config.json`，重新启动并按首次运行流程添加服务器。已保存的密码和口令无法重新导出，需要重新输入。

**换电脑后配置还能用吗？**  
主机、用户名等配置可以复用，但 DPAPI 加密的密码和口令无法在新机器/新用户下解密，需要重新输入。

## 许可证

仓库当前未附带 `LICENSE` 文件。如需公开发布或允许他人复用，请自行补充合适的许可证。
