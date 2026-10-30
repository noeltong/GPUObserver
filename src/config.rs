//! 配置模型与持久化：与 C# 版 Config/AppConfig.cs、ConfigStore.cs 完全同构，
//! 读写同一个 %APPDATA%\GPUObserver\config.json。

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::util::app_data_dir;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerAuthType {
    Password,
    PrivateKey,
}

impl Default for ServerAuthType {
    fn default() -> Self {
        ServerAuthType::Password
    }
}

fn default_schema_version() -> i32 {
    1
}

fn default_refresh_seconds() -> i32 {
    5
}

fn default_true() -> bool {
    true
}

fn default_port() -> i32 {
    22
}

fn default_guid() -> Uuid {
    Uuid::new_v4()
}

/// 界面语言。Rust 版新增字段；C# 版（System.Text.Json 默认跳过未知成员）读取时忽略，
/// 但 C# 版保存配置会丢掉该字段，届时回落 zh-CN。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Language {
    #[default]
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "en-US")]
    En,
}

impl Language {
    pub fn is_default(&self) -> bool {
        *self == Language::ZhCn
    }
}

/// 反序列化兜底：Language 字段出现未知值/非字符串值时回落默认，
/// 绝不让整份配置因这一个字段被判损坏（触发备份+重置）。
fn deserialize_language<'de, D>(deserializer: D) -> Result<Language, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::String(s)) if s == "en-US" => Language::En,
        _ => Language::ZhCn,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AppConfig {
    #[serde(default = "default_schema_version")]
    pub schema_version: i32,
    #[serde(default = "default_refresh_seconds")]
    pub refresh_seconds: i32,
    #[serde(default = "default_true")]
    pub notifications_enabled: bool,
    #[serde(default)]
    pub start_with_windows: bool,
    #[serde(default)]
    pub servers: Vec<ServerConfig>,
    #[serde(
        default,
        deserialize_with = "deserialize_language",
        skip_serializing_if = "Language::is_default"
    )]
    pub language: Language,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            schema_version: 1,
            refresh_seconds: 5,
            notifications_enabled: true,
            start_with_windows: false,
            servers: Vec::new(),
            language: Language::ZhCn,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ServerConfig {
    #[serde(default = "default_guid")]
    pub id: Uuid,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: i32,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub auth_type: ServerAuthType,
    #[serde(default)]
    pub protected_password: Option<String>,
    #[serde(default)]
    pub private_key_path: Option<String>,
    #[serde(default)]
    pub protected_key_passphrase: Option<String>,
    #[serde(default = "default_true")]
    pub notifications_enabled: bool,
    #[serde(default)]
    pub host_key_fingerprint_sha256: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            id: Uuid::new_v4(),
            name: None,
            host: String::new(),
            port: 22,
            username: String::new(),
            auth_type: ServerAuthType::Password,
            protected_password: None,
            private_key_path: None,
            protected_key_passphrase: None,
            notifications_enabled: true,
            host_key_fingerprint_sha256: None,
        }
    }
}

impl ServerConfig {
    pub fn display_host(&self) -> String {
        if self.port == 22 {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    pub fn display_user_host(&self) -> String {
        format!("{}@{}", self.username, self.display_host())
    }

    pub fn display_label(&self) -> String {
        match &self.name {
            Some(name) if !name.trim().is_empty() => name.trim().to_string(),
            _ => self.display_host(),
        }
    }
}

pub struct ConfigLoadResult {
    pub config: AppConfig,
    pub created_new: bool,
    pub warning: Option<String>,
}

pub fn config_directory() -> std::path::PathBuf {
    app_data_dir()
}

pub fn config_path() -> std::path::PathBuf {
    config_directory().join("config.json")
}

pub fn load() -> ConfigLoadResult {
    let path = config_path();
    let contents = std::fs::read_to_string(&path);

    match contents {
        Ok(json) => match serde_json::from_str::<AppConfig>(&json) {
            Ok(mut config) => {
                normalize(&mut config);
                ConfigLoadResult {
                    config,
                    created_new: false,
                    warning: None,
                }
            }
            Err(err) => {
                let warning = "配置文件无法读取，已备份并使用默认设置。";
                backup_corrupt_file();
                crate::util::error(warning, Some(&err.to_string()));
                let mut config = AppConfig::default();
                normalize(&mut config);
                ConfigLoadResult {
                    config,
                    created_new: false,
                    warning: Some(warning.to_string()),
                }
            }
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let mut config = AppConfig::default();
            normalize(&mut config);
            ConfigLoadResult {
                config,
                created_new: true,
                warning: None,
            }
        },
        Err(err) => {
            let warning = "配置文件无法读取，已备份并使用默认设置。";
            backup_corrupt_file();
            crate::util::error(warning, Some(&err.to_string()));
            let mut config = AppConfig::default();
            normalize(&mut config);
            ConfigLoadResult {
                config,
                created_new: false,
                warning: Some(warning.to_string()),
            }
        }
    }
}

/// 原子写入：先写 config.json.tmp，再覆盖改名（与 C# 版一致）。
pub fn save(config: &mut AppConfig) -> std::io::Result<()> {
    normalize(config);
    let dir = config_directory();
    std::fs::create_dir_all(&dir)?;

    let path = config_path();
    let temp_path = {
        let mut p = path.clone().into_os_string();
        p.push(".tmp");
        std::path::PathBuf::from(p)
    };

    let write_result = (|| -> std::io::Result<()> {
        use std::io::Write;
        let json = serde_json::to_string_pretty(config)
            .map_err(|err| std::io::Error::other(err.to_string()))?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temp_path)?;
        file.write_all(json.as_bytes())?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp_path, &path)
    })();

    if write_result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    write_result
}

pub fn normalize(config: &mut AppConfig) {
    config.schema_version = config.schema_version.max(1);
    let refresh = if config.refresh_seconds <= 0 {
        5
    } else {
        config.refresh_seconds
    };
    config.refresh_seconds = refresh.clamp(2, 300);

    let mut ids = std::collections::HashSet::new();
    for server in &mut config.servers {
        server.name = match &server.name {
            Some(name) if !name.trim().is_empty() => Some(name.trim().to_string()),
            _ => None,
        };
        server.host = server.host.trim().to_string();
        server.username = server.username.trim().to_string();
        server.private_key_path = match &server.private_key_path {
            Some(path) if !path.trim().is_empty() => Some(path.trim().to_string()),
            _ => None,
        };
        if !(1..=65535).contains(&server.port) {
            server.port = 22;
        }
        if server.id == Uuid::nil() || !ids.insert(server.id) {
            server.id = Uuid::new_v4();
            ids.insert(server.id);
        }
    }
}

fn backup_corrupt_file() {
    let path = config_path();
    if !path.exists() {
        return;
    }

    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let backup = config_directory().join(format!("config.corrupt-{timestamp}.json"));
    let _ = std::fs::rename(&path, &backup);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_label_prefers_name_and_falls_back_to_host() {
        let mut server = ServerConfig {
            host: "10.0.0.5".to_string(),
            username: "alice".to_string(),
            ..ServerConfig::default()
        };

        assert_eq!(server.display_label(), "10.0.0.5");
        server.name = Some("训练机".to_string());
        assert_eq!(server.display_label(), "训练机");
        server.name = Some("   ".to_string());
        assert_eq!(server.display_label(), "10.0.0.5");
        server.port = 2222;
        assert_eq!(server.display_label(), "10.0.0.5:2222");
    }

    #[test]
    fn normalize_clamps_refresh_and_ports() {
        let mut config = AppConfig {
            refresh_seconds: 0,
            servers: vec![ServerConfig {
                port: 70000,
                ..ServerConfig::default()
            }],
            ..AppConfig::default()
        };
        normalize(&mut config);

        assert_eq!(config.refresh_seconds, 5);
        assert_eq!(config.servers[0].port, 22);
    }

    #[test]
    fn json_round_trip_uses_csharp_schema() {
        let mut config = AppConfig::default();
        config.servers.push(ServerConfig {
            name: Some("训练机".into()),
            host: "10.0.0.5".into(),
            username: "alice".into(),
            ..ServerConfig::default()
        });

        let json = serde_json::to_string_pretty(&config).unwrap();
        assert!(json.contains("\"SchemaVersion\""));
        assert!(json.contains("\"RefreshSeconds\""));
        assert!(json.contains("\"NotificationsEnabled\""));
        assert!(json.contains("\"AuthType\": \"Password\""));

        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.servers[0].host, "10.0.0.5");
        assert_eq!(parsed.servers[0].name.as_deref(), Some("训练机"));
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let json = r#"{"Servers":[{"Host":"h","Username":"u"}]}"#;
        let parsed: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.refresh_seconds, 5);
        assert!(parsed.notifications_enabled);
        assert_eq!(parsed.servers[0].port, 22);
        assert_eq!(parsed.servers[0].auth_type, ServerAuthType::Password);
        assert!(parsed.servers[0].notifications_enabled);
        assert_eq!(parsed.language, Language::ZhCn);
    }

    #[test]
    fn language_defaults_round_trips_and_never_breaks_config() {
        // 缺字段 → 默认 zh-CN；默认值不写出该键（与 C# 版输出保持一致）。
        let parsed: AppConfig = serde_json::from_str(r#"{"Servers":[]}"#).unwrap();
        assert_eq!(parsed.language, Language::ZhCn);
        let json = serde_json::to_string(&AppConfig::default()).unwrap();
        assert!(!json.contains("Language"));

        // en-US 往返。
        let mut config = AppConfig::default();
        config.language = Language::En;
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"Language\":\"en-US\""));
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.language, Language::En);

        // 未知值/非字符串值/null 一律回落 zh-CN，不报错。
        for raw in [r#"{"Language":"fr-FR"}"#, r#"{"Language":42}"#, r#"{"Language":null}"#] {
            let parsed: AppConfig = serde_json::from_str(raw).unwrap();
            assert_eq!(parsed.language, Language::ZhCn);
        }
    }
}
