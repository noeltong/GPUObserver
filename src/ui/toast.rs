//! 系统通知：Windows Toast（WinRT）+ 托盘气泡回退。对应 C# ToastService。
//! 点击激活使用协议 `gpuobserver://`（对应 C# 的 COM activator 转发路径）。

use super::theme;
use super::win;
use crate::monitor::snapshot::IdleNotificationArgs;
use std::sync::Mutex;
use uuid::Uuid;
use windows::core::{HSTRING, Interface};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use windows::Data::Xml::Dom::XmlDocument;

pub const AUMID: &str = "GPUObserver.App";
pub const TOAST_PROTOCOL: &str = "gpuobserver";

/// 最近一次气泡通知对应的服务器（点击气泡时打开对应页签）。
pub static LAST_BALLOON_SERVER: Mutex<Option<Uuid>> = Mutex::new(None);

pub fn take_balloon_server_id() -> Option<Uuid> {
    LAST_BALLOON_SERVER.lock().ok().and_then(|mut guard| guard.take())
}

/// 确保开始菜单存在带 AUMID 的快捷方式（unpackaged 应用显示 Toast 的前提），
/// 并注册 gpuobserver:// 协议（toast 点击激活）。
pub fn ensure_toast_registration() {
    ensure_aumid_shortcut();
    register_protocol();
}

fn exe_path() -> Option<String> {
    std::env::current_exe().ok().map(|p| p.display().to_string())
}

fn start_menu_lnk() -> Option<std::path::PathBuf> {
    let appdata = std::env::var("APPDATA").ok()?;
    Some(
        std::path::PathBuf::from(appdata)
            .join("Microsoft\\Windows\\Start Menu\\Programs")
            .join("GPU 观察.lnk"),
    )
}

/// 手工构造 VT_LPWSTR 的 PROPVARIANT（crate 未提供单字符串 Init 助手）。
fn lpwstr_propvariant(value: &HSTRING) -> PROPVARIANT {
    #[repr(C)]
    struct RawPropVariant {
        vt: u16,
        reserved1: u16,
        reserved2: u16,
        reserved3: u16,
        pointer: *const u16,
        pad: u64,
    }
    assert_eq!(std::mem::size_of::<RawPropVariant>(), std::mem::size_of::<PROPVARIANT>());
    let raw = RawPropVariant {
        vt: 31, // VT_LPWSTR
        reserved1: 0,
        reserved2: 0,
        reserved3: 0,
        pointer: value.as_ptr(),
        pad: 0,
    };
    unsafe { std::mem::transmute(raw) }
}

fn ensure_aumid_shortcut() {
    let Some(target) = start_menu_lnk() else {
        return;
    };
    if target.exists() {
        return;
    }
    let Some(exe) = exe_path() else {
        return;
    };

    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let com_needed = hr.is_ok();

        let result = (|| -> bool {
            let link: IShellLinkW = match CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) {
                Ok(link) => link,
                Err(_) => return false,
            };
            let exe_wide = win::to_wide(&exe);
            if link
                .SetPath(windows::core::PCWSTR(exe_wide.as_ptr()))
                .is_err()
            {
                return false;
            }

            let store: IPropertyStore = match link.cast() {
                Ok(store) => store,
                Err(_) => return false,
            };

            let aumid_key = PROPERTYKEY {
                fmtid: windows::core::GUID::from_u128(0x9F4C2855_9F79_4B39_A8D0_E1D42DE1D5F3),
                pid: 5, // System.AppUserModel.ID
            };
            let aumid = HSTRING::from(AUMID);
            let variant = lpwstr_propvariant(&aumid);
            if store.SetValue(&aumid_key, &variant).is_err() {
                return false;
            }
            let _ = store.Commit();

            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let persist: windows::Win32::System::Com::IPersistFile = match link.cast() {
                Ok(p) => p,
                Err(_) => return false,
            };
            let path_wide = win::to_wide(&target.display().to_string());
            persist
                .Save(windows::core::PCWSTR(path_wide.as_ptr()), true.into())
                .is_ok()
        })();

        if com_needed {
            CoUninitialize();
        }
        let _ = result;
    }
}

/// 注册 HKCU\Software\Classes\gpuobserver 协议。
fn register_protocol() {
    let Some(exe) = exe_path() else {
        return;
    };
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    if let Ok((root, _)) = hkcu.create_subkey("Software\\Classes\\gpuobserver") {
        let _ = root.set_value("", &"URL:GPU 观察");
        let _ = root.set_value("URL Protocol", &"");
    }
    if let Ok((shell_key, _)) = hkcu.create_subkey("Software\\Classes\\gpuobserver\\shell\\open\\command") {
        let _ = shell_key.set_value("", &format!("\"{exe}\" \"%1\""));
    }
}

fn show_toast_xml(xml: &str, tag: &str) -> Result<(), String> {
    unsafe {
        let doc = XmlDocument::new().map_err(|e| e.to_string())?;
        doc.LoadXml(&HSTRING::from(xml)).map_err(|e| e.to_string())?;
        let toast = ToastNotification::CreateToastNotification(&doc).map_err(|e| e.to_string())?;
        toast.SetTag(&HSTRING::from(tag)).map_err(|e| e.to_string())?;
        toast.SetGroup(&HSTRING::from("gpu-observe")).map_err(|e| e.to_string())?;
        let expiration = chrono::Local::now() + chrono::Duration::minutes(30);
        let date_time = windows::Foundation::DateTime {
            UniversalTime: (expiration.timestamp_micros() - 116_444_736_000_000) * 10,
        };
        if let Ok(reference) = windows::Foundation::PropertyValue::CreateDateTime(date_time) {
            if let Ok(typed) = reference.cast::<windows::Foundation::IReference<windows::Foundation::DateTime>>() {
                let _ = toast.SetExpirationTime(&typed);
            }
        }
        let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))
            .map_err(|e| e.to_string())?;
        notifier.Show(&toast).map_err(|e| e.to_string())
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn toast_xml(title: &str, body: &str, launch: &str) -> String {
    format!(
        "<toast activationType=\"protocol\" launch=\"{launch}\" scenario=\"default\" duration=\"short\">\
         <visual><binding template=\"ToastGeneric\">\
         <text>{}</text><text>{}</text><text placement=\"attribution\">{}</text>\
         </binding></visual></toast>",
        escape(title),
        escape(body),
        escape(super::i18n::t().app_name)
    )
}

pub struct ToastFallback<'a> {
    pub title: &'a str,
    pub body: &'a str,
    pub timeout_ms: u32,
}

/// Show：toast 失败回退气泡（对应 ToastService.Show）。
fn show(
    title: &str,
    body: &str,
    server_id: Option<Uuid>,
    tag: &str,
    launch: String,
    fallback: Option<ToastFallback<'_>>,
) {
    if let Err(err) = show_toast_xml(&toast_xml(title, body, &launch), tag) {
        crate::util::error("显示系统通知失败，改用托盘气泡。", Some(&err));
        *LAST_BALLOON_SERVER.lock().unwrap() = server_id;
        if let Some(fb) = fallback {
            super::app::with_app(|app| app.show_balloon(fb.title, fb.body, fb.timeout_ms));
        }
    }
}

/// 空闲通知（对应 ToastService.ShowIdle 的文案）。
pub fn show_idle(args: &IdleNotificationArgs, _tray_hwnd: HWND, _tray_icon: windows::Win32::UI::WindowsAndMessaging::HICON) {
    let t = super::i18n::t();
    let gpu_text = args
        .gpu_indices
        .iter()
        .map(|index| format!("GPU {index}"))
        .collect::<Vec<_>>()
        .join(t.toast_gpu_joiner);
    let gpu = args
        .snapshot
        .gpus
        .iter()
        .find(|gpu| args.gpu_indices.contains(&gpu.index));
    let metrics = gpu
        .map(|gpu| {
            let util = theme::format_percent(gpu.utilization_percent);
            let used = theme::format_memory(gpu.memory_used_mib);
            let total = theme::format_memory(gpu.memory_total_mib);
            super::i18n::fmt(t.toast_idle_metrics, &[("util", &util), ("used", &used), ("total", &total)])
        })
        .unwrap_or_default();
    let title = super::i18n::fmt(t.toast_idle_title, &[("server", &args.server_label)]);
    let body = super::i18n::fmt(t.toast_idle_body, &[("gpus", &gpu_text), ("metrics", &metrics)]);
    let launch = format!("{TOAST_PROTOCOL}://open-server/{}", args.server_id.as_simple());
    let simple = args.server_id.as_simple().to_string();

    show(
        &title,
        &body,
        Some(args.server_id),
        &format!("idle-{simple}"),
        launch,
        Some(ToastFallback {
            title: &title,
            body: &body,
            timeout_ms: 8000,
        }),
    );
}

/// 测试通知（对应 ToastService.ShowTest）。
pub fn show_test(
    server_id: Option<Uuid>,
    server_label: Option<&str>,
    _tray_hwnd: HWND,
    _tray_icon: windows::Win32::UI::WindowsAndMessaging::HICON,
) {
    let t = super::i18n::t();
    let title = t.toast_test_title;
    let body = match server_label {
        None => t.toast_test_body_generic.to_string(),
        Some(label) => super::i18n::fmt(t.toast_test_body_server, &[("server", label)]),
    };
    let launch = match server_id {
        Some(id) => format!("{TOAST_PROTOCOL}://open-server/{}", id.as_simple()),
        None => format!("{TOAST_PROTOCOL}://show"),
    };

    show(
        title,
        &body,
        server_id,
        "test",
        launch,
        Some(ToastFallback {
            title,
            body: &body,
            timeout_ms: 5000,
        }),
    );
}

/// 解析 toast 点击带来的启动参数（gpuobserver://open-server/<guid>）。
pub fn parse_launch_argument(argument: &str) -> Option<Uuid> {
    let lower = argument.to_lowercase();
    let rest = lower
        .strip_prefix("gpuobserver://open-server/")
        .or_else(|| lower.strip_prefix("gpuobserver://open/"))?;
    Uuid::parse_str(rest.trim_end_matches('/')).ok()
}

pub fn is_launch_argument(argument: &str) -> bool {
    argument
        .to_lowercase()
        .starts_with(&format!("{TOAST_PROTOCOL}://"))
}
