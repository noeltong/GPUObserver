//! 探针输出解析：与 C# 版 Monitoring/Parsing/{CsvLine,ProbeRecords,ProbeParser}.cs 对齐。
//! 注意语义细节：引号感知 CSV、`[N/A]`/`Not Supported`/`-` 视为空值、
//! 数字走 invariant 解析、Long 走银行家舍入、用户名 Ordinal 区分大小写匹配。

use super::snapshot::{GpuProcessTelemetry, GpuTelemetry, RamTelemetry, ServerSnapshot};
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ComputeAppRecord {
    pub gpu_uuid: String,
    pub pid: i32,
    pub process_name: String,
    pub memory_mib: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PmonRecord {
    pub gpu_index: i32,
    pub pid: i32,
}

#[derive(Debug, Clone)]
pub struct PsRecord {
    pub pid: i32,
    pub user: String,
    pub command: String,
}

#[derive(Debug, Default)]
pub struct ProbeMainResult {
    pub gpus: Vec<GpuTelemetry>,
    pub compute_apps: Vec<ComputeAppRecord>,
    pub pmon: Vec<PmonRecord>,
    pub warnings: Vec<String>,
    pub pmon_available: bool,
    pub nvsmi_missing: bool,
    pub ram: Option<RamTelemetry>,
}

impl ProbeMainResult {
    pub fn collect_gpu_pids(&self) -> Vec<i32> {
        let mut seen = HashSet::new();
        self.pmon
            .iter()
            .map(|item| item.pid)
            .chain(self.compute_apps.iter().map(|item| item.pid))
            .filter(|pid| *pid > 0)
            .filter(|pid| seen.insert(*pid))
            .collect()
    }
}

/// 引号感知的 CSV 拆分（`""` 转义为字面引号），每个字段做 trim。
pub fn csv_split(line: &str) -> Vec<String> {
    let mut values = Vec::new();
    if line.is_empty() {
        return values;
    }

    let chars: Vec<char> = line.chars().collect();
    let mut current = String::new();
    let mut in_quotes = false;

    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            if in_quotes && i + 1 < chars.len() && chars[i + 1] == '"' {
                current.push('"');
                i += 1;
            } else {
                in_quotes = !in_quotes;
            }
        } else if c == ',' && !in_quotes {
            values.push(current.trim().to_string());
            current.clear();
        } else {
            current.push(c);
        }
        i += 1;
    }

    values.push(current.trim().to_string());
    values
}

mod parse_util {
    fn is_null_marker(text: &str) -> bool {
        text.eq_ignore_ascii_case("N/A")
            || text.eq_ignore_ascii_case("Not Supported")
            || text == "-"
    }

    fn clean<'a>(value: Option<&'a str>) -> Option<String> {
        let value = value?;
        if value.trim().is_empty() {
            return None;
        }
        let text = value.trim().trim_matches(|c| c == '[' || c == ']');
        if is_null_marker(text) {
            return None;
        }
        Some(text.to_string())
    }

    pub fn f64(value: Option<&str>) -> Option<f64> {
        clean(value).and_then(|text| text.parse::<f64>().ok())
    }

    pub fn long(value: Option<&str>) -> Option<i64> {
        let text = clean(value)?;
        if let Ok(parsed) = text.parse::<i64>() {
            return Some(parsed);
        }
        if let Ok(as_double) = text.parse::<f64>() {
            // C# Math.Round 默认银行家舍入（MidpointRounding.ToEven）。
            return Some(as_double.round_ties_even() as i64);
        }
        None
    }

    pub fn int(value: Option<&str>) -> Option<i32> {
        let parsed = long(value)?;
        if parsed >= i32::MIN as i64 && parsed <= i32::MAX as i64 {
            Some(parsed as i32)
        } else {
            None
        }
    }
}

pub fn parse_nvidia_gpus(lines: &[String]) -> Vec<GpuTelemetry> {
    let mut by_index: HashMap<i32, GpuTelemetry> = HashMap::new();

    for raw_line in lines {
        let line = raw_line.trim();
        if line.is_empty() || line.contains("NVSMI_MISSING") {
            continue;
        }

        let fields = csv_split(line);
        if fields.len() < 5 {
            continue;
        }

        let Some(index) = parse_util::int(Some(fields[0].as_str())) else {
            continue;
        };

        let uuid = fields.get(1).cloned().unwrap_or_default();
        let name = fields
            .get(2)
            .cloned()
            .unwrap_or_else(|| format!("GPU {index}"));
        let utilization = parse_util::f64(fields.get(3).map(|s| s.as_str()));
        let memory_used = parse_util::long(fields.get(4).map(|s| s.as_str()));
        let memory_total = parse_util::long(fields.get(5).map(|s| s.as_str()));
        let temperature = parse_util::f64(fields.get(6).map(|s| s.as_str()));
        let power = parse_util::f64(fields.get(7).map(|s| s.as_str()));
        let power_limit = parse_util::f64(fields.get(8).map(|s| s.as_str()));

        by_index.insert(
            index,
            GpuTelemetry {
                index,
                uuid,
                name,
                utilization_percent: utilization,
                memory_used_mib: memory_used,
                memory_total_mib: memory_total,
                temperature_c: temperature,
                power_w: power,
                power_limit_w: power_limit,
            },
        );
    }

    let mut gpus: Vec<GpuTelemetry> = by_index.into_values().collect();
    gpus.sort_by_key(|gpu| gpu.index);
    gpus
}

pub fn parse_compute_apps(lines: &[String]) -> Vec<ComputeAppRecord> {
    let mut by_key: HashMap<(String, i32), ComputeAppRecord> = HashMap::new();

    for raw_line in lines {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        let fields = csv_split(line);
        if fields.len() < 2 {
            continue;
        }

        let pid = parse_util::int(Some(fields[1].as_str()));
        let uuid = fields[0].clone();
        let Some(pid) = pid else {
            continue;
        };
        if pid <= 0 || uuid.trim().is_empty() {
            continue;
        }

        let has_memory = fields.len() >= 4;
        let memory = if has_memory {
            parse_util::long(fields.last().map(|s| s.as_str()))
        } else {
            None
        };
        let name_field_count = if fields.len() > 2 {
            fields.len() - if has_memory { 3 } else { 2 }
        } else {
            0
        };
        let name = if name_field_count > 0 {
            fields[2..2 + name_field_count].join(",").trim().to_string()
        } else {
            String::new()
        };

        by_key.insert(
            (uuid.clone(), pid),
            ComputeAppRecord {
                gpu_uuid: uuid,
                pid,
                process_name: name,
                memory_mib: memory,
            },
        );
    }

    by_key.into_values().collect()
}

pub fn parse_pmon(lines: &[String]) -> Vec<PmonRecord> {
    let mut seen: HashSet<(i32, i32)> = HashSet::new();

    for raw_line in lines {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 2 {
            continue;
        }

        let gpu = parse_util::int(Some(fields[0]));
        let pid = parse_util::int(Some(fields[1]));
        match (gpu, pid) {
            (Some(gpu), Some(pid)) if gpu >= 0 && pid > 0 => {
                seen.insert((gpu, pid));
            }
            _ => {}
        }
    }

    let mut pairs: Vec<(i32, i32)> = seen.into_iter().collect();
    pairs.sort();
    pairs
        .into_iter()
        .map(|(gpu_index, pid)| PmonRecord { gpu_index, pid })
        .collect()
}

pub fn parse_ps(lines: &[String]) -> HashMap<i32, PsRecord> {
    let mut result = HashMap::new();

    for raw_line in lines {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        // 与 C# Split(null, 3, RemoveEmptyEntries) 一致：
        // 前两段按空白分隔，命令保留其余子串（内部空白不拆）。
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.len() < 2 {
            continue;
        }

        let pid = parse_util::int(Some(tokens[0]));
        let Some(pid) = pid else {
            continue;
        };
        if pid <= 0 {
            continue;
        }

        let user = tokens[1].trim().to_string();
        let command = if tokens.len() > 2 {
            skip_whitespace_fields(line, 2).trim().to_string()
        } else {
            String::new()
        };

        result.insert(
            pid,
            PsRecord {
                pid,
                user,
                command,
            },
        );
    }

    result
}

/// 跳过前 `count` 个空白分隔的字段，返回剩余的原始子串（保留其中的空白）。
fn skip_whitespace_fields(line: &str, count: usize) -> &str {
    let mut remaining = line;
    let mut skipped = 0;
    while skipped < count {
        let trimmed_start = remaining.trim_start();
        let Some(next_ws) = trimmed_start.find(char::is_whitespace) else {
            return "";
        };
        remaining = &trimmed_start[next_ws..];
        skipped += 1;
    }
    remaining.trim_start()
}

pub fn parse_free(lines: &[String]) -> Option<RamTelemetry> {
    for raw_line in lines {
        let line = raw_line.trim();
        if !line.starts_with("Mem:") {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }

        let total = parse_util::long(parts.get(1).copied());
        let used = parse_util::long(parts.get(2).copied());
        let available = parse_util::long(parts.get(6).copied())
            .or_else(|| match (total, used) {
                (Some(total), Some(used)) => Some(total - used),
                _ => None,
            });

        if let (Some(total), Some(used), Some(available)) = (total, used, available) {
            if total > 0 && used >= 0 && available >= 0 {
                return Some(RamTelemetry {
                    total_mib: total,
                    used_mib: used.min(total),
                    available_mib: available,
                });
            }
        }
    }

    None
}

const MARKERS: [&str; 5] = ["@@GPU@@", "@@APPS@@", "@@PMON@@", "@@MEM@@", "@@END@@"];

pub fn parse_probe(stdout: &str) -> ProbeMainResult {
    let mut sections: HashMap<&str, Vec<String>> = HashMap::new();
    let mut pmon_rc_text: Option<String> = None;
    let mut current: Option<&str> = None;

    let normalized = stdout.replace("\r\n", "\n");
    for raw_line in normalized.split('\n') {
        let line = raw_line.trim_end_matches('\r');
        if line.starts_with("@@PMONRC@@") {
            pmon_rc_text = Some(line["@@PMONRC@@".len()..].trim().to_string());
            current = None;
            continue;
        }

        if let Some(marker) = MARKERS.iter().find(|marker| **marker == line) {
            current = Some(marker);
            sections.entry(marker).or_default();
            continue;
        }

        if let Some(marker) = current {
            sections.entry(marker).or_default().push(line.to_string());
        }
    }

    let mut result = ProbeMainResult::default();
    let gpu_lines = sections.get("@@GPU@@").cloned().unwrap_or_default();
    if gpu_lines
        .iter()
        .any(|line| line.contains("NVSMI_MISSING"))
    {
        result.nvsmi_missing = true;
        result
            .warnings
            .push("未找到 nvidia-smi，无法读取 GPU 信息。".to_string());
    }

    result.gpus = parse_nvidia_gpus(&gpu_lines);
    result.compute_apps = parse_compute_apps(&sections.get("@@APPS@@").cloned().unwrap_or_default());
    result.pmon = parse_pmon(&sections.get("@@PMON@@").cloned().unwrap_or_default());

    let pmon_rc = parse_util::int(pmon_rc_text.as_deref());
    result.pmon_available = pmon_rc == Some(0);
    if !result.pmon_available && !result.nvsmi_missing {
        result
            .warnings
            .push("无法获取完整的 GPU 进程列表（pmon 不可用），空闲通知已暂停。".to_string());
    }

    result.ram = parse_free(&sections.get("@@MEM@@").cloned().unwrap_or_default());
    if result.gpus.is_empty() && !result.nvsmi_missing {
        result.warnings.push("未读取到 GPU 信息。".to_string());
    }

    result
}

pub fn build_snapshot(
    server_id: Uuid,
    username: &str,
    main: ProbeMainResult,
    ps_records: &HashMap<i32, PsRecord>,
    ps_succeeded: bool,
    captured_at: DateTime<Utc>,
) -> ServerSnapshot {
    let mut warnings = main.warnings.clone();
    if !main.pmon_available {
        warnings.push("无法获取完整的 GPU 进程列表（pmon 不可用），空闲通知已暂停。".to_string());
    }
    if !ps_succeeded {
        warnings.push("无法解析 GPU 进程所属用户，空闲通知已暂停。".to_string());
    }

    let process_data_complete = main.pmon_available && ps_succeeded;

    let mut gpu_index_by_uuid: HashMap<String, i32> = HashMap::new();
    for gpu in &main.gpus {
        if !gpu.uuid.trim().is_empty() {
            gpu_index_by_uuid.insert(gpu.uuid.to_lowercase(), gpu.index);
        }
    }

    let mut keys: HashSet<(i32, i32)> = HashSet::new();
    let mut compute_memory: HashMap<(i32, i32), Option<i64>> = HashMap::new();
    let mut compute_names: HashMap<(i32, i32), String> = HashMap::new();

    for record in &main.pmon {
        keys.insert((record.gpu_index, record.pid));
    }

    for app in &main.compute_apps {
        let Some(gpu_index) = gpu_index_by_uuid.get(&app.gpu_uuid.to_lowercase()) else {
            continue;
        };
        let key = (*gpu_index, app.pid);
        keys.insert(key);
        compute_memory.insert(key, app.memory_mib);
        compute_names.insert(key, app.process_name.clone());
    }

    let mut counts: HashMap<i32, i32> = HashMap::new();
    for gpu in &main.gpus {
        counts.insert(gpu.index, 0);
    }

    let mut processes: Vec<GpuProcessTelemetry> = Vec::new();
    if ps_succeeded {
        let mut sorted_keys: Vec<(i32, i32)> = keys.into_iter().collect();
        sorted_keys.sort();

        for key @ (gpu, pid) in sorted_keys {
            let Some(ps) = ps_records.get(&pid) else {
                // 该 pid 多半在 pmon 与 ps 之间退出了。让下一个采样决定，
                // 不要把它变成一次假的状态迁移。
                continue;
            };

            if ps.user != username {
                continue;
            }

            let command = if ps.command.trim().is_empty() {
                compute_names
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| format!("pid {pid}"))
            } else {
                ps.command.clone()
            };
            let memory = compute_memory.get(&key).copied().flatten();

            processes.push(GpuProcessTelemetry {
                gpu_index: gpu,
                pid,
                user: ps.user.clone(),
                command,
                memory_mib: memory,
            });
            if let Some(count) = counts.get_mut(&gpu) {
                *count += 1;
            }
        }
    }

    // 与 C# 的 warnings.Distinct() 对应：保序去重后拼接。
    let mut seen = HashSet::new();
    warnings.retain(|warning| seen.insert(warning.clone()));

    ServerSnapshot {
        server_id,
        captured_at,
        gpus: main.gpus,
        ram: main.ram,
        processes,
        process_data_complete,
        my_process_counts: counts,
        warning: if warnings.is_empty() {
            None
        } else {
            Some(warnings.join(" "))
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_line_handles_quotes_and_commas() {
        let fields = csv_split("0, GPU-abc, \"NVIDIA RTX, Special\", 45, 1024, 24576, 62, 210.5, 350");

        assert_eq!(fields.len(), 9);
        assert_eq!(fields[2], "NVIDIA RTX, Special");
    }

    #[test]
    fn nvidia_gpu_parser_parses_values_and_na() {
        let lines: Vec<String> = vec![
            "0, GPU-1, NVIDIA GeForce RTX 4090, 55, 8192, 24564, 64, 120.5, [N/A]".into(),
            "1, GPU-2, NVIDIA A100, [N/A], [N/A], 81920, [N/A], [N/A], 400".into(),
        ];

        let gpus = parse_nvidia_gpus(&lines);

        assert_eq!(gpus.len(), 2);
        let first = &gpus[0];
        assert_eq!(first.utilization_percent, Some(55.0));
        assert_eq!(first.memory_used_mib, Some(8192));
        assert_eq!(first.memory_total_mib, Some(24564));
        assert_eq!(first.power_w, Some(120.5));
        assert_eq!(first.power_limit_w, None);
        let percent = first.memory_percent().unwrap();
        assert!((percent - 33.35).abs() < 0.005, "memory percent {percent}");

        let second = &gpus[1];
        assert_eq!(second.utilization_percent, None);
        assert_eq!(second.memory_used_mib, None);
        assert_eq!(second.power_limit_w, Some(400.0));
    }

    #[test]
    fn compute_apps_parser_handles_comma_in_name_and_memory() {
        let rows = parse_compute_apps(&[
            "GPU-1, 1234, python, 1024".into(),
            "GPU-2, 5678, \"trainer, distributed\", 2048".into(),
            "GPU-1, N/A, other, N/A".into(),
        ]);

        assert_eq!(rows.len(), 2);
        assert!(rows
            .iter()
            .any(|row| row.pid == 1234 && row.memory_mib == Some(1024)));
        let second = rows.iter().find(|row| row.pid == 5678).unwrap();
        assert_eq!(second.process_name, "trainer, distributed");
        assert_eq!(second.memory_mib, Some(2048));
    }

    #[test]
    fn pmon_parser_ignores_headers_and_deduplicates() {
        let rows = parse_pmon(&[
            "# gpu pid type sm mem enc dec command".into(),
            "# Idx # C/G % % % % name".into(),
            "0 111 C 50 12 0 0 python".into(),
            "0 111 C 50 12 0 0 python".into(),
            "1 222 G 10 3 0 0 Xorg".into(),
            "1 - - - - - - -".into(),
        ]);

        assert_eq!(rows.len(), 2);
        assert!(rows.contains(&PmonRecord { gpu_index: 0, pid: 111 }));
        assert!(rows.contains(&PmonRecord { gpu_index: 1, pid: 222 }));
    }

    #[test]
    fn ps_parser_parses_user_and_command() {
        let records = parse_ps(&[
            " 111 alice python3".to_string(),
            " 222 bob   /usr/bin/trainer --epochs 10".to_string(),
            "bad line".to_string(),
            " 333 carol".to_string(),
        ]);

        assert_eq!(records.len(), 3);
        assert_eq!(records[&111].user, "alice");
        assert_eq!(records[&222].command, "/usr/bin/trainer --epochs 10");
        assert_eq!(records[&333].user, "carol");
        assert_eq!(records[&333].command, "");
    }

    #[test]
    fn free_parser_parses_memory_row() {
        let ram = parse_free(&[
            "               total        used        free      shared  buff/cache   available".into(),
            "Mem:           64000       12000        2000        100       50000       49000".into(),
            "Swap:          16384           0       16384".into(),
        ]);

        let ram = ram.expect("ram row parsed");
        assert_eq!(ram.total_mib, 64000);
        assert_eq!(ram.used_mib, 12000);
        assert_eq!(ram.available_mib, 49000);
        assert_eq!(ram.percent(), 18.75);
    }

    #[test]
    fn probe_parser_splits_sections() {
        let output = "@@GPU@@\n\
             0, GPU-1, RTX 4090, 50, 8000, 24564, 60, 200, 350\n\
             @@APPS@@\n\
             GPU-1, 1234, python, 8000\n\
             @@PMON@@\n\
             # gpu pid type sm mem enc dec command\n\
             0 1234 C 10 30 0 0 python\n\
             @@PMONRC@@0\n\
             @@MEM@@\n\
                            total        used        free      shared  buff/cache   available\n\
             Mem:           64000       12000        2000         100       50000       49000\n\
             Swap:          16384           0       16384\n\
             @@END@@\n";

        let result = parse_probe(output);

        assert!(!result.nvsmi_missing);
        assert!(result.pmon_available);
        assert_eq!(result.gpus.len(), 1);
        assert_eq!(result.compute_apps.len(), 1);
        assert_eq!(result.pmon.len(), 1);
        assert!(result.ram.is_some());
        assert_eq!(result.collect_gpu_pids().len(), 1);
    }

    #[test]
    fn probe_parser_marks_nvsmi_missing() {
        let output = "@@GPU@@\nNVSMI_MISSING\n@@APPS@@\n@@PMON@@\n@@PMONRC@@127\n@@MEM@@\nMEM_MISSING\n@@END@@\n";

        let result = parse_probe(output);

        assert!(result.nvsmi_missing);
        assert!(!result.pmon_available);
        assert!(result.gpus.is_empty());
        assert!(result.ram.is_none());
        assert!(!result.warnings.is_empty());
    }

    fn gpu(index: i32, uuid: &str, name: &str) -> GpuTelemetry {
        GpuTelemetry {
            index,
            uuid: uuid.to_string(),
            name: name.to_string(),
            utilization_percent: Some(50.0),
            memory_used_mib: Some(1000),
            memory_total_mib: Some(24564),
            temperature_c: Some(50.0),
            power_w: Some(100.0),
            power_limit_w: Some(350.0),
        }
    }

    #[test]
    fn snapshot_builder_filters_to_current_user_and_computes_counts() {
        let uuid1 = "GPU-uuid-1";
        let uuid2 = "GPU-uuid-2";
        let mut main = ProbeMainResult {
            pmon_available: true,
            ram: Some(RamTelemetry {
                total_mib: 64000,
                used_mib: 12000,
                available_mib: 49000,
            }),
            ..ProbeMainResult::default()
        };
        main.gpus = vec![gpu(0, uuid1, "RTX 4090"), gpu(1, uuid2, "RTX 4090")];
        main.pmon = vec![
            PmonRecord { gpu_index: 0, pid: 100 },
            PmonRecord { gpu_index: 1, pid: 200 },
            PmonRecord { gpu_index: 0, pid: 300 },
        ];
        main.compute_apps = vec![
            ComputeAppRecord { gpu_uuid: uuid1.into(), pid: 100, process_name: "python".into(), memory_mib: Some(6000) },
            ComputeAppRecord { gpu_uuid: uuid2.into(), pid: 200, process_name: "trainer".into(), memory_mib: Some(1000) },
            ComputeAppRecord { gpu_uuid: uuid1.into(), pid: 300, process_name: "other".into(), memory_mib: Some(2000) },
        ];

        let ps: HashMap<i32, PsRecord> = HashMap::from([
            (100, PsRecord { pid: 100, user: "alice".into(), command: "python".into() }),
            (200, PsRecord { pid: 200, user: "bob".into(), command: "trainer".into() }),
            (300, PsRecord { pid: 300, user: "alice".into(), command: "jupyter".into() }),
        ]);

        let snapshot = build_snapshot(Uuid::new_v4(), "alice", main, &ps, true, Utc::now());

        assert!(snapshot.process_data_complete);
        assert_eq!(snapshot.processes.len(), 2);
        assert_eq!(snapshot.processes.iter().filter(|p| p.gpu_index == 0).count(), 2);
        assert_eq!(snapshot.my_process_counts[&0], 2);
        assert_eq!(snapshot.my_process_counts[&1], 0);
        assert!(snapshot
            .processes
            .iter()
            .any(|p| p.pid == 100 && p.memory_mib == Some(6000)));
        assert!(!snapshot.processes.iter().any(|p| p.pid == 200));
    }

    #[test]
    fn snapshot_builder_invalid_pmon_suppresses_complete() {
        let mut main = ProbeMainResult {
            pmon_available: false,
            ..ProbeMainResult::default()
        };
        main.gpus = vec![gpu(0, "u", "GPU")];

        let snapshot = build_snapshot(Uuid::new_v4(), "alice", main, &HashMap::new(), true, Utc::now());

        assert!(!snapshot.process_data_complete);
        assert!(snapshot.warning.is_some());
    }
}
