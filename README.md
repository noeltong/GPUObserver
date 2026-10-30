[简体中文](README_CN.md) | English

# GPU Observer (Rust)

A Windows system tray app that monitors GPU and memory usage on multiple Linux + NVIDIA servers over SSH in real time.
This repository is a **Rust rewrite** of `gpu_observe_app` (the C# / .NET 8 WinForms version, kept in a separate local repository that is not published):
the UI dimensions, visuals, and behavior match the C# version 1:1, and the config file is fully interoperable,
so it can replace the old version outright.

## Features

- **Tray resident**: left-click the tray icon to open the monitoring panel (docked to the taskbar corner); right-click for the menu (Open panel / Refresh now / Settings… / Exit).
- **Interface language**: Settings → General → Language — Simplified Chinese / English. Changes take effect immediately (no restart) and persist in the config.
- **Multi-server tabs**: one tab per server at the top of the panel (with a status dot: green = connected, yellow = connecting, red = error).
- **GPU monitoring**: VRAM usage (purple/amber/red tiers), utilization, temperature, power draw, plus a 5-minute dual-line trend chart (utilization area + VRAM line).
- **Memory monitoring**: a card for server RAM usage.
- **My GPU processes**: shows only GPU processes running as the SSH login user (PID / command / VRAM).
- **GPU idle notifications**: a system notification fires when all current-user processes on a GPU have exited (global + per-server switches).
- **Security**: SSH connection and key parsing are fully built in (russh — no ssh.exe/ssh-agent dependency); passwords and passphrases are encrypted with Windows DPAPI (CurrentUser); host keys use TOFU (pinned on first connection, blocked on change, with "Re-trust").
- Also: start with Windows (HKCU Run), single instance (launching a second time brings up the panel), and Toast click-through straight to the matching server tab.

## Correspondence with the C# version

| C# (gpu_observe_app) | Rust (this repo) |
| --- | --- |
| WinForms + GDI+ custom drawing | Win32 + GDI+ hand-drawn (`windows` crate), PerMonitorV2 DPI |
| SSH.NET 2026.0 | russh 0.64 (ring backend) |
| System.Text.Json config | serde_json, same `%APPDATA%\GPUObserver\config.json` |
| ProtectedData (DPAPI) | `CryptProtectData/CryptUnprotectData`, entropy `GPUObserver.Secret.v1` (cross-readable, verified) |
| UWP Toolkit Toast | WinRT Toast + `gpuobserver://` protocol activation |
| MonitorService / ServerMonitor | `monitor/service.rs` / `monitor/monitor.rs` (tokio tasks) |
| NotificationStateMachine | `monitor/notify_sm.rs` (line-by-line equivalent) |
| SampleQueue (300s/512) | `monitor/queue.rs` |
| nvidia-smi probe | `monitor/probe_main.sh` (byte-identical, delivered via base64 + `/bin/sh`) |

### Size reference (logical pixels @96dpi, scales with DPI)

| Window | Size |
| --- | --- |
| Tray popup | **520 × 715** (520 × 300 with no servers), corner radius 10 |
| GPU card | Height 158 (chart 44) |
| Memory card | Height 52 (bar height 6) |
| Settings window | Client area **680 × 614** |
| Add/edit server | Client area **600 × 588** |

## Build & run

Requires Rust (MSVC toolchain) + Windows SDK (rc.exe, for embedding the icon and manifest).

```powershell
cargo build --release
# Output: target\release\GPUObserver.exe (single file, ~4.8 MB)
```

On first run the "Add server" dialog opens automatically. If you have used the C# version, no migration is needed:
close the old app and run this one — it reads the existing config as-is (passwords / host key fingerprints carry over).

> The single-instance mutex is shared with the C# version (`Local\GPUObserver.Instance.v1`); the two versions cannot run at the same time.

## Behavior details (aligned with the C# version)

- The refresh interval is global, default 5 seconds (configurable 2–300); polling runs fully in the background and never stops when the panel is closed.
- Failure backoff 5s → ×2 → capped at 60s; authentication / host key / credential / private-key errors are fixed at 60s.
- One long-lived connection per server (keepalive 30s); probe command timeout 20s, ps timeout 10s, connect timeout 10s.
- Process ownership: the `ps` username must equal the SSH username **case-sensitively**.
- When pmon is unavailable or ps fails, the sample is marked "incomplete", idle notifications pause, and the state machine re-seeds.
- Samples are kept in memory for the last 300 seconds only (max 512), never written to disk.
- The popup hides when it loses focus; Esc closes it; a tray left-click within 300ms of hiding will not reopen it.

## Tests

The full C# xUnit backend test suite was ported (parsing / queue / state machine / config / DPAPI):

```powershell
cargo test   # 34 unit tests
```

## Known differences

- The tray context menu uses the native system style (C# uses a custom-drawn ContextMenuStrip with the same font).
- Toast click activation goes through the `gpuobserver://` protocol (C# uses a COM activator) — behavior is equivalent: both open the popup and switch to the matching server tab.
- The empty-state icon uses the icon embedded in the exe directly (same as C#).

## License

Released under the [MIT License](LICENSE). © 2026 Noel Tong
