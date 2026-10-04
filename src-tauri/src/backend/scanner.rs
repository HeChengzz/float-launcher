use crate::backend::item::Item;
use crate::backend::lnk::resolve_lnk_target;
use crate::backend::paths::{desktop_path, start_menu_dirs};
use std::collections::HashMap;
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;
use walkdir::WalkDir;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn is_lnk(p: &Path) -> bool {
    p.extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("lnk"))
        .unwrap_or(false)
}

fn is_exe(p: &Path) -> bool {
    p.extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("exe"))
        .unwrap_or(false)
}

/// 判断一个 .lnk 是否"指向文件夹"
/// - 目标能被解析出来，且是目录 → true
/// - 解析失败或目标不是目录 → false（保守起见，宁可显示也不漏）
fn lnk_points_to_dir(lnk_path: &Path) -> bool {
    let target = resolve_lnk_target(lnk_path);
    // 解析失败时 resolve_lnk_target 会返回原路径，此时不是目录
    target.is_dir()
}

/// 桌面上的可启动项：.lnk + .exe
/// - .lnk 指向文件夹的会被排除
pub fn scan_desktop_shortcuts() -> Vec<Item> {
    let desktop = desktop_path();
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(&desktop) else {
        return out;
    };

    for e in rd.flatten() {
        let p = e.path();

        // 目录直接跳过
        if p.is_dir() {
            continue;
        }

        if is_lnk(&p) {
            // ★ 指向文件夹的 .lnk 不进软件面板
            if lnk_points_to_dir(&p) {
                continue;
            }
        } else if !is_exe(&p) {
            // 既不是 .lnk 也不是 .exe，跳过
            continue;
        }

        let name = p
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }

        out.push(Item {
            name,
            path: p.to_string_lossy().into_owned(),
            is_dir: false,
            kind: if is_lnk(&p) { "shortcut".into() } else { "exe".into() },
        });
    }

    out.sort_by_key(|i| i.name.to_lowercase());
    out
}

/// 开始菜单 Programs 目录下的所有 .lnk（去重）
pub fn scan_start_apps() -> Vec<Item> {
    let mut seen: HashMap<String, String> = HashMap::new();
    for d in start_menu_dirs() {
        if !d.is_dir() {
            continue;
        }
        for entry in WalkDir::new(&d).into_iter().filter_map(|e| e.ok()) {
            let p = entry.path();
            if !is_lnk(p) {
                continue;
            }
            let name = p
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if name.is_empty() {
                continue;
            }
            seen.entry(name)
                .or_insert_with(|| p.to_string_lossy().into_owned());
        }
    }
    let mut out: Vec<Item> = seen
        .into_iter()
        .map(|(name, path)| Item {
            name,
            path,
            is_dir: false,
            kind: "app".into(),
        })
        .collect();
    out.sort_by_key(|i| i.name.to_lowercase());
    out
}

/// UWP / 微软商店应用（走 PowerShell Get-StartApps）
pub fn scan_uwp_apps() -> Vec<Item> {
    let script = "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8;\
                  Get-StartApps | ConvertTo-Json -Compress";
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    let Ok(out) = output else { return Vec::new(); };
    if !out.status.success() {
        return Vec::new();
    }

    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() {
        return Vec::new();
    }

    let text = if text.starts_with('[') {
        text
    } else {
        format!("[{text}]")
    };

    let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&text) else {
        return Vec::new();
    };

    arr.into_iter()
        .filter_map(|v| {
            let name = v.get("Name")?.as_str()?.trim().to_string();
            let appid = v.get("AppID")?.as_str()?.trim().to_string();
            if name.is_empty() || appid.is_empty() {
                return None;
            }
            let is_uwp = appid.contains('!');
            Some(Item {
                name,
                path: format!("shell:AppsFolder\\{appid}"),
                is_dir: false,
                kind: if is_uwp { "uwp".into() } else { "app".into() },
            })
        })
        .collect()
}

/// 桌面内容：返回 (folders, files)
/// **文件面板只放"普通文件"**，.lnk / .exe 都归软件面板
pub fn scan_desktop_contents() -> (Vec<Item>, Vec<Item>) {
    let desktop = desktop_path();
    let mut folders = Vec::new();
    let mut files = Vec::new();
    let Ok(rd) = fs::read_dir(&desktop) else {
        return (folders, files);
    };

    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        let lower = name.to_lowercase();

        if lower == "desktop.ini" || lower == "thumbs.db" {
            continue;
        }

        if p.is_dir() {
            folders.push(Item {
                name,
                path: p.to_string_lossy().into_owned(),
                is_dir: true,
                kind: "folder".into(),
            });
            continue;
        }

        // .lnk / .exe → 归软件面板
        if is_lnk(&p) || is_exe(&p) {
            continue;
        }

        files.push(Item {
            name,
            path: p.to_string_lossy().into_owned(),
            is_dir: false,
            kind: "file".into(),
        });
    }
    folders.sort_by_key(|i| i.name.to_lowercase());
    files.sort_by_key(|i| i.name.to_lowercase());
    (folders, files)
}