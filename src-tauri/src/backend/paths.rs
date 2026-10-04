use std::path::PathBuf;

/// 真实桌面路径（dirs 内部走 SHGetKnownFolderPath，兼容 OneDrive 重定向）
pub fn desktop_path() -> PathBuf {
    if let Some(p) = dirs::desktop_dir() {
        return p;
    }
    std::env::var_os("USERPROFILE")
        .map(|p| PathBuf::from(p).join("Desktop"))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 两个开始菜单 Programs 目录
pub fn start_menu_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        v.push(
            PathBuf::from(appdata)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
        );
    }
    if let Some(pd) = std::env::var_os("ProgramData") {
        v.push(
            PathBuf::from(pd)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
        );
    }
    v
}