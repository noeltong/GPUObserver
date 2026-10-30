//! SSH 探针命令：与 C# 版 Monitoring/Parsing/ProbeCommand.cs 逐字一致。
//! 服务器可能把 exec 交给 fish 等非 POSIX 登录 shell，因此把 POSIX 脚本
//! base64 后交给 /bin/sh 执行，GPU 探测不依赖登录 shell 语法。

use base64::Engine;
use std::collections::HashSet;

/// 探针 POSIX 脚本（独立文件便于与一次性验证程序共享同一份内容）。
pub const MAIN_SCRIPT: &str = include_str!("probe_main.sh");

pub fn build_main_command() -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(MAIN_SCRIPT.as_bytes());
    format!("printf '%s' '{encoded}' | base64 -d | /bin/sh")
}

pub fn build_ps_command(pids: &[i32]) -> Option<String> {
    let mut seen = HashSet::new();
    let normalized: Vec<i32> = pids
        .iter()
        .copied()
        .filter(|pid| *pid > 0)
        .filter(|pid| seen.insert(*pid))
        .collect();
    if normalized.is_empty() {
        return None;
    }

    let list = normalized
        .iter()
        .map(|pid| pid.to_string())
        .collect::<Vec<_>>()
        .join(",");
    // user 列默认宽 8 字符，长用户名会被截断成 "longuser+" 导致归属匹配失败；
    // 显式加宽到 32（procps 的 :N 列宽语法），用户名完整输出。
    Some(format!(
        "env LC_ALL=C ps -o pid=,user:32=,comm= -p {list}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_command_runs_posix_script_through_sh_for_fish_login_shell() {
        let command = build_main_command();
        let prefix = "printf '%s' '";
        let suffix = "' | base64 -d | /bin/sh";

        assert!(command.starts_with(prefix));
        assert!(command.ends_with(suffix));

        let encoded = &command[prefix.len()..command.len() - suffix.len()];
        assert!(
            encoded
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'='),
            "base64 段只应包含标准 base64 字符: {encoded}"
        );

        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .expect("valid base64");
        assert_eq!(String::from_utf8(decoded).unwrap(), MAIN_SCRIPT);
    }

    #[test]
    fn ps_command_only_contains_numeric_pids() {
        let command = build_ps_command(&[10, 20, 20, -5, 0]).expect("pids present");

        assert!(command.starts_with("env LC_ALL=C ps"));
        assert!(command.contains("user:32="), "user 列必须加宽避免截断: {command}");
        assert!(command.contains("-p 10,20"));
        assert!(build_ps_command(&[]).is_none());
    }
}
