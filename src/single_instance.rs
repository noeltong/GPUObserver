//! 单实例：命名互斥体 + 命名管道。对应 C# SingleInstance。
//! 互斥体 `Local\GPUObserver.Instance.v1`；管道 `GPUObserver.Instance.v1.Pipe`。

use crate::util;
use std::sync::Arc;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_FLAGS_AND_ATTRIBUTES, FILE_GENERIC_WRITE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, PIPE_ACCESS_INBOUND,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, NAMED_PIPE_MODE, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE,
    PIPE_WAIT,
};
use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex};

pub const INSTANCE_NAME: &str = "GPUObserver.Instance.v1";

pub struct SingleInstance {
    mutex: Option<HANDLE>,
    pub is_primary: bool,
}

impl SingleInstance {
    pub fn new(name: &str) -> Self {
        unsafe {
            let wide = crate::ui::win::to_wide(&format!("Local\\{name}"));
            let handle = CreateMutexW(None, true, windows::core::PCWSTR(wide.as_ptr()));
            let is_primary = handle.is_ok()
                && windows::Win32::Foundation::GetLastError()
                    != windows::Win32::Foundation::ERROR_ALREADY_EXISTS;
            SingleInstance {
                mutex: handle.ok(),
                is_primary,
            }
        }
    }

    /// 主实例：启动管道服务器循环（阻塞线程内运行）。
    pub fn start_server(&self, on_message: Arc<dyn Fn(String) + Send + Sync>) {
        if !self.is_primary {
            return;
        }
        let pipe_name = format!("\\\\.\\pipe\\{INSTANCE_NAME}.Pipe");
        std::thread::spawn(move || {
            loop {
                let result = serve_one(&pipe_name, &on_message);
                if let Err(err) = result {
                    util::error("单实例管道读取失败。", Some(&err));
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            }
        });
    }

    /// 次实例：发送一条消息给主实例。
    pub fn send(&self, message: &str) -> bool {
        let pipe_name = format!("\\\\.\\pipe\\{INSTANCE_NAME}.Pipe");
        unsafe {
            let wide = crate::ui::win::to_wide(&pipe_name);
            let handle = CreateFileW(
                windows::core::PCWSTR(wide.as_ptr()),
                FILE_GENERIC_WRITE.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                Default::default(),
                None,
            );
            let Ok(handle) = handle else {
                return false;
            };

            let payload = format!("{message}\r\n");
            let bytes = payload.as_bytes();
            let mut written = 0u32;
            let ok = WriteFile(handle, Some(bytes), Some(&mut written), None).is_ok();
            let _ = CloseHandle(handle);
            ok
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        if let Some(handle) = self.mutex.take() {
            unsafe {
                if self.is_primary {
                    let _ = ReleaseMutex(handle);
                }
                let _ = CloseHandle(handle);
            }
        }
    }
}

fn serve_one(pipe_name: &str, on_message: &Arc<dyn Fn(String) + Send + Sync>) -> Result<(), String> {
    unsafe {
        let wide = crate::ui::win::to_wide(pipe_name);
        // PIPE_ACCESS_INBOUND + FILE_FLAG_FIRST_PIPE_INSTANCE；CurrentUserOnly 由安全描述符近似。
        let open_mode = FILE_FLAGS_AND_ATTRIBUTES(PIPE_ACCESS_INBOUND.0 | 0x0008_0000);
        let pipe_mode = NAMED_PIPE_MODE(PIPE_TYPE_BYTE.0 | PIPE_READMODE_BYTE.0 | PIPE_WAIT.0);
        let handle = CreateNamedPipeW(
            windows::core::PCWSTR(wide.as_ptr()),
            open_mode,
            pipe_mode,
            1,
            4096,
            4096,
            0,
            None,
        );
        if handle.is_invalid() {
            return Err(format!("CreateNamedPipeW 失败: {}", windows::Win32::Foundation::GetLastError().0));
        }

        if !ConnectNamedPipe(handle, None).is_ok() {
            let err = windows::Win32::Foundation::GetLastError();
            if err != windows::Win32::Foundation::ERROR_PIPE_CONNECTED {
                let _ = CloseHandle(handle);
                return Err(format!("ConnectNamedPipe 失败: {}", err.0));
            }
        }

        // 读一行
        let mut buffer = Vec::<u8>::with_capacity(512);
        let mut chunk = [0u8; 256];
        loop {
            let mut read = 0u32;
            if !ReadFile(handle, Some(&mut chunk), Some(&mut read), None).is_ok() || read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read as usize]);
            if buffer.contains(&b'\n') {
                break;
            }
        }
        let _ = windows::Win32::System::Pipes::DisconnectNamedPipe(handle);
        let _ = CloseHandle(handle);

        let text = String::from_utf8_lossy(&buffer).trim().to_string();
        if !text.is_empty() {
            on_message(text);
        }
        Ok(())
    }
}
