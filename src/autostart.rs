//! 开机自启动：HKCU Run 键。对应 C# Autostart。

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE_NAME: &str = "GPUObserver";

pub fn is_enabled() -> bool {
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    hkcu.open_subkey(RUN_KEY)
        .ok()
        .and_then(|key| {
            key.get_value::<String, _>(VALUE_NAME)
                .ok()
                .filter(|value: &String| !value.is_empty())
        })
        .is_some()
}

pub fn set_enabled(enabled: bool) -> bool {
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let Ok((key, _)) = hkcu.create_subkey(RUN_KEY) else {
        crate::util::error("设置开机自启动失败。", None);
        return false;
    };

    if enabled {
        let Ok(exe_path) = std::env::current_exe() else {
            crate::util::error("设置开机自启动失败：无法定位程序路径。", None);
            return false;
        };
        let value = format!("\"{}\" --autostart", exe_path.display());
        if let Err(err) = key.set_value(VALUE_NAME, &value) {
            crate::util::error("设置开机自启动失败。", Some(&err.to_string()));
            return false;
        }
    } else if let Err(err) = key.delete_value(VALUE_NAME) {
        if err.kind() != std::io::ErrorKind::NotFound {
            crate::util::error("设置开机自启动失败。", Some(&err.to_string()));
            return false;
        }
    }
    true
}
