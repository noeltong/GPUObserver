//! 日志：与 C# 版 Util/Log.cs 对应。
//! 写入 %APPDATA%\GPUObserver\logs\app.log，1MiB 轮转为 app.old.log。

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::OnceLock;

const MAX_BYTES: u64 = 1024 * 1024;

fn gate() -> &'static Mutex<()> {
    static GATE: OnceLock<Mutex<()>> = OnceLock::new();
    GATE.get_or_init(|| Mutex::new(()))
}

pub fn app_data_dir() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_default();
    PathBuf::from(base).join("GPUObserver")
}

fn log_dir() -> PathBuf {
    app_data_dir().join("logs")
}

pub fn info(message: &str) {
    write("INFO", message, None);
}

pub fn error(message: &str, detail: Option<&str>) {
    write("ERROR", message, detail);
}

/// Remove credential-like material before anything reaches a persistent log.
pub fn redact_sensitive(input: &str) -> String {
    let mut out = input.to_string();
    for marker in [
        "password",
        "passwd",
        "token",
        "authorization",
        "bearer",
        "secret",
        "private_key",
        "-----begin",
    ] {
        let lower = out.to_ascii_lowercase();
        let mut start = 0;
        while let Some(pos) = lower[start..].find(marker) {
            let pos = start + pos;
            let end = out[pos..]
                .find(['\n', '\r', ' ', ';', '|'])
                .map(|i| pos + i)
                .unwrap_or(out.len());
            out.replace_range(pos..end, "<redacted>");
            start = pos + "<redacted>".len();
            if start >= out.len() {
                break;
            }
        }
    }
    out
}

fn write(level: &str, message: &str, detail: Option<&str>) {
    let _guard = gate()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let result = (|| -> std::io::Result<()> {
        let dir = log_dir();
        fs::create_dir_all(&dir)?;
        let path = dir.join("app.log");
        rotate_if_needed(&path)?;

        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f %:z");
        let mut line = format!("{timestamp} [{level}] {}", redact_sensitive(message));
        if let Some(detail) = detail {
            line.push_str(" | ");
            line.push_str(&redact_sensitive(detail));
        }
        line.push('\n');

        let mut file = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)?;
        file.write_all(line.as_bytes())
    })();

    if let Err(err) = result {
        // 诊断日志绝不能拖垮托盘应用。
        let _ = err;
    }
}

fn rotate_if_needed(path: &std::path::Path) -> std::io::Result<()> {
    let Ok(meta) = fs::metadata(path) else {
        return Ok(());
    };
    if meta.len() < MAX_BYTES {
        return Ok(());
    }

    let old = path.with_file_name("app.old.log");
    let _ = fs::remove_file(&old);
    fs::rename(path, old)
}
