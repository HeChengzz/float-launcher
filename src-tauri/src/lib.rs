mod backend;

use backend::item::Item;
use backend::{actions, icon::icon_data_url, lnk::resolve_lnk_target, scanner::*};

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::RwLock;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    utils::config::WindowEffectsConfig,
    window::Effect,
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow,
    WindowEvent,
};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_global_shortcut::{
    Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState,
};

const EDGE_MARGIN: i32 = 15;
const TOP_MARGIN: i32 = 15;
const BOTTOM_MARGIN: i32 = 15;
const ANIM_STEP_MS: u64 = 14;
const SHOW_DURATION: u64 = 240;
const HIDE_DURATION: u64 = 180;
const FULLSCREEN_POLL_MS: u64 = 500;

// ============================================================
//  内置白名单
// ============================================================
#[cfg(windows)]
const BUILTIN_WHITELIST: &[&str] = &[
    "chrome.exe", "msedge.exe", "firefox.exe", "brave.exe",
    "opera.exe", "vivaldi.exe", "iexplore.exe",
    "code.exe", "code - insiders.exe", "cursor.exe",
    "devenv.exe", "sublime_text.exe", "notepad++.exe",
    "pycharm64.exe", "pycharm.exe", "idea64.exe", "idea.exe",
    "rider64.exe", "goland64.exe", "webstorm64.exe",
    "clion64.exe", "datagrip64.exe", "rustrover64.exe",
    "phpstorm64.exe", "androidstudio64.exe",
    "windowsterminal.exe", "wt.exe", "conhost.exe",
    "powershell.exe", "pwsh.exe", "cmd.exe",
    "mintty.exe", "alacritty.exe", "wezterm-gui.exe",
    "explorer.exe",
    "winword.exe", "excel.exe", "powerpnt.exe", "onenote.exe",
    "wechat.exe", "weixin.exe", "qq.exe", "telegram.exe",
    "discord.exe", "slack.exe", "teams.exe", "zoom.exe",
    "ms-teams.exe",
    "vlc.exe", "mpc-hc64.exe", "mpc-hc.exe", "mpv.exe",
    "potplayer.exe", "potplayermini64.exe",
    "mstsc.exe", "vmware-vmx.exe", "virtualboxvm.exe",
];

// ============================================================
//  进程名 / 窗口标题
// ============================================================
#[cfg(windows)]
fn process_name_by_pid(pid: u32) -> String {
    use std::os::windows::ffi::OsStringExt;
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    if pid == 0 { return String::new(); }
    unsafe {
        let handle = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) if !h.is_invalid() => h,
            _ => return String::new(),
        };
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        )
        .is_ok();
        let _ = CloseHandle(handle);
        if !ok || size == 0 { return String::new(); }
        let os_str = std::ffi::OsString::from_wide(&buf[..size as usize]);
        std::path::PathBuf::from(os_str)
            .file_name()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    }
}

#[cfg(windows)]
unsafe fn window_title(hwnd: windows::Win32::Foundation::HWND) -> String {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowTextW;
    let mut buf = [0u16; 512];
    let len = GetWindowTextW(hwnd, &mut buf);
    if len <= 0 { return String::new(); }
    String::from_utf16_lossy(&buf[..len as usize])
}

fn matches_rule(process: &str, title: &str, rule: &str) -> bool {
    let rule_lower = rule.trim().to_lowercase();
    if rule_lower.is_empty() { return false; }
    if rule_lower.ends_with(".exe") {
        process.eq_ignore_ascii_case(&rule_lower)
    } else {
        title.to_lowercase().contains(&rule_lower)
    }
}

// ============================================================
//  窗口装饰
// ============================================================
#[cfg(windows)]
fn setup_window_decoration(w: &WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE,
        DWMWCP_ROUND,
    };

    let Ok(hwnd_tauri) = w.hwnd() else { return; };
    let raw = hwnd_tauri.0 as usize;
    let hwnd = HWND(raw as *mut core::ffi::c_void);

    unsafe {
        let pref = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &pref as *const _ as *const _,
            std::mem::size_of_val(&pref) as u32,
        );
        let none: u32 = 0xFFFF_FFFE;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &none as *const _ as *const _,
            std::mem::size_of_val(&none) as u32,
        );
    }
}

#[cfg(not(windows))]
fn setup_window_decoration(_w: &WebviewWindow) {}

// ============================================================
//  背景模糊
// ============================================================
fn apply_blur_effect(w: &WebviewWindow, blur: u32) {
    if blur == 0 {
        let cfg = WindowEffectsConfig { effects: vec![], ..Default::default() };
        let _ = w.set_effects(cfg);
        return;
    }
    let acrylic = WindowEffectsConfig {
        effects: vec![Effect::Acrylic],
        interactive: false,
        ..Default::default()
    };
    if w.set_effects(acrylic).is_ok() { return; }
    let blur_eff = WindowEffectsConfig {
        effects: vec![Effect::Blur],
        interactive: false,
        ..Default::default()
    };
    let _ = w.set_effects(blur_eff);
}

// ============================================================
//  全屏检测
// ============================================================
#[cfg(windows)]
fn is_d3d_fullscreen() -> bool {
    use windows::Win32::UI::Shell::{
        SHQueryUserNotificationState, QUNS_RUNNING_D3D_FULL_SCREEN,
    };
    unsafe {
        if let Ok(state) = SHQueryUserNotificationState() {
            if state == QUNS_RUNNING_D3D_FULL_SCREEN { return true; }
        }
    }
    false
}

#[cfg(windows)]
fn is_foreground_fullscreen_game(app: &AppHandle) -> bool {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetShellWindow, GetWindowRect,
        GetWindowThreadProcessId, IsIconic,
    };

    unsafe {
        let fg = GetForegroundWindow();
        if fg.0.is_null() { return false; }
        let shell = GetShellWindow();
        if fg == shell { return false; }

        for label in ["launcher", "files", "settings"] {
            if let Some(w) = app.get_webview_window(label) {
                if let Ok(h) = w.hwnd() {
                    if h.0 == fg.0 { return false; }
                }
            }
        }

        if IsIconic(fg).as_bool() { return false; }

        let mut rect = RECT::default();
        if GetWindowRect(fg, &mut rect).is_err() { return false; }

        let hmon = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(hmon, &mut mi).as_bool() { return false; }

        let m = mi.rcMonitor;
        let covers_monitor =
            rect.left <= m.left
            && rect.top <= m.top
            && rect.right >= m.right
            && rect.bottom >= m.bottom;
        if !covers_monitor { return false; }

        let mut pid: u32 = 0;
        GetWindowThreadProcessId(fg, Some(&mut pid));
        let process = process_name_by_pid(pid);
        let title = window_title(fg);

        for rule in BUILTIN_WHITELIST {
            if process.eq_ignore_ascii_case(rule) { return false; }
        }

        let cfg = app.state::<PanelState>().config_snapshot();
        for rule in &cfg.non_game_whitelist {
            if matches_rule(&process, &title, rule) { return false; }
        }

        true
    }
}

#[cfg(windows)]
fn is_fullscreen_app_running(app: &AppHandle) -> bool {
    if is_d3d_fullscreen() { return true; }
    if is_foreground_fullscreen_game(app) { return true; }
    false
}

#[cfg(not(windows))]
fn is_fullscreen_app_running(_app: &AppHandle) -> bool { false }

// ============================================================
//  配置
// ============================================================
#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct CustomFont {
    name: String,
    path: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct Config {
    #[serde(default = "default_icon_style")]
    icon_style: String,
    #[serde(default = "default_icon_size")]
    icon_size: u32,
    #[serde(default = "default_font_size")]
    font_size: u32,
    #[serde(default = "default_font_family")]
    font_family: String,
    #[serde(default)]
    custom_fonts: Vec<CustomFont>,
    #[serde(default = "default_pause_on_fullscreen")]
    pause_on_fullscreen: bool,
    #[serde(default = "default_launcher_width")]
    launcher_width: u32,
    #[serde(default = "default_files_width")]
    files_width: u32,
    #[serde(default = "default_opacity")]
    opacity: u32,
    #[serde(default = "default_blur")]
    blur: u32,
    #[serde(default)]
    non_game_whitelist: Vec<String>,
}

fn default_icon_style() -> String { "mask".into() }
fn default_icon_size() -> u32 { 48 }
fn default_font_size() -> u32 { 12 }
fn default_font_family() -> String { "segoe".into() }
fn default_pause_on_fullscreen() -> bool { true }
fn default_launcher_width() -> u32 { 480 }
fn default_files_width() -> u32 { 400 }
fn default_opacity() -> u32 { 100 }
fn default_blur() -> u32 { 0 }

impl Default for Config {
    fn default() -> Self {
        Self {
            icon_style: default_icon_style(),
            icon_size: default_icon_size(),
            font_size: default_font_size(),
            font_family: default_font_family(),
            custom_fonts: Vec::new(),
            pause_on_fullscreen: default_pause_on_fullscreen(),
            launcher_width: default_launcher_width(),
            files_width: default_files_width(),
            opacity: default_opacity(),
            blur: default_blur(),
            non_game_whitelist: Vec::new(),
        }
    }
}

fn config_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("float-launcher").join("config.json")
}

fn fonts_dir() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("float-launcher").join("fonts")
}

fn load_config() -> Config {
    let p = config_path();
    if let Ok(text) = std::fs::read_to_string(&p) {
        if let Ok(cfg) = serde_json::from_str::<Config>(&text) {
            return cfg;
        }
    }
    Config::default()
}

fn save_config(cfg: &Config) {
    let p = config_path();
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(&p, text);
    }
}

// ============================================================
//  面板状态
// ============================================================
struct PanelState {
    launcher_token: AtomicI32,
    launcher_target: AtomicBool,
    files_token: AtomicI32,
    files_target: AtomicBool,
    shortcuts_suspended: AtomicBool,
    config: RwLock<Config>,
}

impl PanelState {
    fn new(config: Config) -> Self {
        Self {
            launcher_token: AtomicI32::new(0),
            launcher_target: AtomicBool::new(false),
            files_token: AtomicI32::new(0),
            files_target: AtomicBool::new(false),
            shortcuts_suspended: AtomicBool::new(false),
            config: RwLock::new(config),
        }
    }
    fn token(&self, label: &str) -> &AtomicI32 {
        if label == "launcher" { &self.launcher_token } else { &self.files_token }
    }
    fn target(&self, label: &str) -> &AtomicBool {
        if label == "launcher" { &self.launcher_target } else { &self.files_target }
    }
    fn bump_token(&self, label: &str) -> i32 {
        self.token(label).fetch_add(1, Ordering::SeqCst) + 1
    }
    fn is_current(&self, label: &str, token: i32) -> bool {
        self.token(label).load(Ordering::SeqCst) == token
    }
    fn set_target(&self, label: &str, visible: bool) {
        self.target(label).store(visible, Ordering::SeqCst);
    }
    fn get_target(&self, label: &str) -> bool {
        self.target(label).load(Ordering::SeqCst)
    }
    fn config_snapshot(&self) -> Config {
        self.config.read().unwrap().clone()
    }
    fn pause_on_fullscreen(&self) -> bool {
        self.config.read().unwrap().pause_on_fullscreen
    }
}

// ============================================================
//  Settings payload
// ============================================================
#[derive(serde::Serialize)]
struct SettingsPayload {
    icon_style: String,
    icon_size: u32,
    font_size: u32,
    font_family: String,
    custom_fonts: Vec<CustomFont>,
    pause_on_fullscreen: bool,
    autostart: bool,
    version: String,
    launcher_width: u32,
    files_width: u32,
    opacity: u32,
    blur: u32,
    non_game_whitelist: Vec<String>,
}

// ============================================================
//  Commands
// ============================================================
#[tauri::command]
fn scan_desktop() -> Vec<Item> { scan_desktop_shortcuts() }

#[tauri::command]
fn scan_start() -> Vec<Item> { scan_start_apps() }

#[tauri::command]
fn scan_uwp() -> Vec<Item> { scan_uwp_apps() }

#[tauri::command]
fn scan_files() -> (Vec<Item>, Vec<Item>) { scan_desktop_contents() }

#[tauri::command]
fn list_dir(path: String) -> Result<(Vec<Item>, Vec<Item>), String> {
    let p = PathBuf::from(&path);
    if !p.is_dir() {
        return Err("not a directory".into());
    }
    let mut folders = Vec::new();
    let mut files = Vec::new();
    let rd = std::fs::read_dir(&p).map_err(|e| e.to_string())?;
    for e in rd.flatten() {
        let ep = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        let lower = name.to_lowercase();
        if lower == "desktop.ini" || lower == "thumbs.db" || lower == ".ds_store" {
            continue;
        }
        let is_dir = ep.is_dir();
        let it = Item {
            name,
            path: ep.to_string_lossy().into_owned(),
            is_dir,
            kind: if is_dir { "folder".into() } else { "file".into() },
        };
        if is_dir { folders.push(it); } else { files.push(it); }
    }
    folders.sort_by_key(|i| i.name.to_lowercase());
    files.sort_by_key(|i| i.name.to_lowercase());
    Ok((folders, files))
}

#[tauri::command]
fn icon(path: String) -> Option<String> {
    let real = if path.to_lowercase().ends_with(".lnk") {
        resolve_lnk_target(Path::new(&path))
            .to_string_lossy()
            .into_owned()
    } else {
        path.clone()
    };
    icon_data_url(&real).or_else(|| icon_data_url(&path))
}

#[tauri::command]
fn resolve_lnk(path: String) -> String {
    resolve_lnk_target(Path::new(&path))
        .to_string_lossy()
        .into_owned()
}

#[tauri::command]
fn open(path: String) -> Result<(), String> { actions::open_path(&path) }

#[tauri::command]
fn mk_folder(name: String, parent: Option<String>) -> Result<String, String> {
    actions::create_folder(&name, parent.as_deref())
}

#[tauri::command]
fn mk_file(name: String, parent: Option<String>) -> Result<String, String> {
    actions::create_file(&name, parent.as_deref())
}

#[tauri::command]
fn delete_to_trash(path: String) -> Result<(), String> {
    actions::delete_to_trash(&path)
}

#[tauri::command(rename_all = "snake_case")]
fn rename_item(path: String, new_name: String) -> Result<String, String> {
    actions::rename_path(&path, &new_name)
}

#[tauri::command(rename_all = "snake_case")]
fn move_item(src: String, dst_dir: String) -> Result<String, String> {
    actions::move_item(&src, &dst_dir)
}

#[tauri::command]
fn hide_panel_cmd(app: AppHandle, label: String) {
    let state = app.state::<PanelState>();
    state.set_target(&label, false);
    do_hide(&app, &label);
}

#[tauri::command]
fn get_settings(app: AppHandle) -> SettingsPayload {
    let cfg = app.state::<PanelState>().config_snapshot();
    let autostart = app.autolaunch().is_enabled().unwrap_or(false);
    SettingsPayload {
        icon_style: cfg.icon_style,
        icon_size: cfg.icon_size,
        font_size: cfg.font_size,
        font_family: cfg.font_family,
        custom_fonts: cfg.custom_fonts,
        pause_on_fullscreen: cfg.pause_on_fullscreen,
        autostart,
        version: env!("CARGO_PKG_VERSION").to_string(),
        launcher_width: cfg.launcher_width,
        files_width: cfg.files_width,
        opacity: cfg.opacity,
        blur: cfg.blur,
        non_game_whitelist: cfg.non_game_whitelist,
    }
}

#[tauri::command]
fn set_setting(
    app: AppHandle,
    key: String,
    value: serde_json::Value,
) -> Result<(), String> {
    let state = app.state::<PanelState>();

    let snapshot = {
        let mut cfg = state.config.write().unwrap();
        match key.as_str() {
            "icon_style" => {
                let v = value.as_str().unwrap_or("mask");
                let v = match v {
                    "real" | "mask" | "redraw" => v,
                    _ => return Err("invalid icon_style".into()),
                };
                cfg.icon_style = v.to_string();
            }
            "icon_size" => {
                cfg.icon_size = value.as_u64().unwrap_or(48).clamp(32, 96) as u32;
            }
            "font_size" => {
                cfg.font_size = value.as_u64().unwrap_or(12).clamp(10, 18) as u32;
            }
            "font_family" => {
                let v = value.as_str().unwrap_or("segoe");
                let ok = matches!(v, "segoe" | "yahei" | "mono" | "system")
                    || v.starts_with("custom:");
                if !ok {
                    return Err("invalid font_family".into());
                }
                cfg.font_family = v.to_string();
            }
            "pause_on_fullscreen" => {
                cfg.pause_on_fullscreen = value.as_bool().unwrap_or(true);
            }
            "launcher_width" => {
                cfg.launcher_width = value.as_u64().unwrap_or(480).clamp(320, 800) as u32;
            }
            "files_width" => {
                cfg.files_width = value.as_u64().unwrap_or(400).clamp(280, 800) as u32;
            }
            "opacity" => {
                cfg.opacity = value.as_u64().unwrap_or(100).clamp(0, 100) as u32;
            }
            "blur" => {
                cfg.blur = value.as_u64().unwrap_or(0).clamp(0, 100) as u32;
            }
            "non_game_whitelist" => {
                if let Some(arr) = value.as_array() {
                    let mut out: Vec<String> = Vec::new();
                    for v in arr {
                        if let Some(s) = v.as_str() {
                            let s = s.trim();
                            if !s.is_empty() {
                                out.push(s.to_string());
                            }
                        }
                    }
                    cfg.non_game_whitelist = out;
                } else {
                    return Err("non_game_whitelist must be an array".into());
                }
            }
            _ => return Err(format!("unknown key: {key}")),
        }
        cfg.clone()
    };

    save_config(&snapshot);

    if key == "launcher_width" || key == "files_width" {
        let label = if key == "launcher_width" { "launcher" } else { "files" };
        if let Some(w) = app.get_webview_window(label) {
            if w.is_visible().unwrap_or(false) {
                let (_, end_x, y, panel_w, panel_h) = panel_geometry(&app, label, &w);
                let _ = w.set_size(PhysicalSize::new(panel_w as u32, panel_h as u32));
                let _ = w.set_position(PhysicalPosition::new(end_x, y));
            }
        }
    }

    if key == "blur" {
        for lbl in ["launcher", "files"] {
            if let Some(w) = app.get_webview_window(lbl) {
                apply_blur_effect(&w, snapshot.blur);
            }
        }
    }

    let _ = app.emit("settings-changed", &snapshot);
    Ok(())
}

#[tauri::command]
fn set_autostart(app: AppHandle, value: bool) -> Result<(), String> {
    let mgr = app.autolaunch();
    let r = if value { mgr.enable() } else { mgr.disable() };
    r.map_err(|e| e.to_string())
}

#[tauri::command]
async fn upload_font(app: AppHandle) -> Result<Option<CustomFont>, String> {
    let app2 = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app2.dialog()
            .file()
            .add_filter("字体文件", &["ttf", "otf", "woff", "woff2"])
            .blocking_pick_file()
    })
    .await
    .map_err(|e| e.to_string())?;

    let Some(file_path) = picked else { return Ok(None); };

    let src = file_path.into_path().map_err(|e| e.to_string())?;
    let file_name = src.file_name().ok_or("invalid file name")?
        .to_string_lossy().into_owned();
    let name = Path::new(&file_name).file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| file_name.clone());

    let dir = fonts_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dst = dir.join(&file_name);
    std::fs::copy(&src, &dst).map_err(|e| e.to_string())?;

    let custom = CustomFont {
        name: name.clone(),
        path: dst.to_string_lossy().into_owned(),
    };

    let state = app.state::<PanelState>();
    let snapshot = {
        let mut cfg = state.config.write().unwrap();
        cfg.custom_fonts.retain(|f| f.name != name);
        cfg.custom_fonts.push(custom.clone());
        cfg.font_family = format!("custom:{}", name);
        cfg.clone()
    };
    save_config(&snapshot);
    let _ = app.emit("settings-changed", &snapshot);

    Ok(Some(custom))
}

#[tauri::command]
fn remove_font(app: AppHandle, name: String) -> Result<(), String> {
    let state = app.state::<PanelState>();
    let snapshot = {
        let mut cfg = state.config.write().unwrap();
        if let Some(f) = cfg.custom_fonts.iter().find(|f| f.name == name) {
            let _ = std::fs::remove_file(&f.path);
        }
        cfg.custom_fonts.retain(|f| f.name != name);
        if cfg.font_family == format!("custom:{name}") {
            cfg.font_family = "segoe".into();
        }
        cfg.clone()
    };
    save_config(&snapshot);
    let _ = app.emit("settings-changed", &snapshot);
    Ok(())
}

#[tauri::command]
fn show_settings(app: AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        setup_window_decoration(&w);
        let _ = w.center();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[tauri::command]
fn hide_settings(app: AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.hide();
    }
}

// ============================================================
//  窗口枚举
// ============================================================
#[derive(serde::Serialize)]
struct WindowInfo {
    title: String,
    process: String,
}

#[cfg(windows)]
unsafe extern "system" fn enum_windows_cb(
    hwnd: windows::Win32::Foundation::HWND,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::BOOL {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowTextLengthW, IsWindowVisible, GetWindowThreadProcessId,
    };

    let list = &mut *(lparam.0 as *mut Vec<WindowInfo>);

    if !IsWindowVisible(hwnd).as_bool() { return windows::Win32::Foundation::BOOL(1); }
    if GetWindowTextLengthW(hwnd) == 0 { return windows::Win32::Foundation::BOOL(1); }

    let title = window_title(hwnd);
    if title.trim().is_empty() { return windows::Win32::Foundation::BOOL(1); }

    let mut pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    let process = process_name_by_pid(pid);

    list.push(WindowInfo { title, process });
    windows::Win32::Foundation::BOOL(1)
}

#[tauri::command]
fn list_visible_windows() -> Vec<WindowInfo> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::LPARAM;
        use windows::Win32::UI::WindowsAndMessaging::EnumWindows;
        let mut list: Vec<WindowInfo> = Vec::new();
        unsafe {
            let _ = EnumWindows(
                Some(enum_windows_cb),
                LPARAM(&mut list as *mut _ as isize),
            );
        }
        let mut seen = std::collections::HashSet::new();
        list.retain(|w| seen.insert((w.process.clone(), w.title.clone())));
        list.sort_by(|a, b| a.process.cmp(&b.process).then(a.title.cmp(&b.title)));
        list
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

// ============================================================
//  ★ 快捷键注册 / 暂停
// ============================================================
fn shortcut_alt1() -> Shortcut {
    Shortcut::new(Some(Modifiers::ALT), Code::Digit1)
}
fn shortcut_alt2() -> Shortcut {
    Shortcut::new(Some(Modifiers::ALT), Code::Digit2)
}

/// 注册两个面板快捷键（幂等）
///
/// ★ 关键：必须用 `on_shortcut()` 而不是 `register()`。
///   - `register()` 只把热键交给系统，不会绑定回调
///   - `on_shortcut()` = 内部 register + 设置 handler，二者缺一不可
fn register_panel_shortcuts(app: &AppHandle) {
    let gs = app.global_shortcut();

    // 先清理（避免"已注册"错误导致 on_shortcut 提前返回、handler 不更新）
    let _ = gs.unregister(shortcut_alt1());
    let _ = gs.unregister(shortcut_alt2());

    let h1 = app.clone();
    if let Err(e) = gs.on_shortcut(shortcut_alt1(), move |_app, _sc, ev| {
        if ev.state == ShortcutState::Pressed {
            toggle_panel(&h1, "launcher");
        }
    }) {
        eprintln!("[shortcut] Alt+1 注册失败: {e}");
    }

    let h2 = app.clone();
    if let Err(e) = gs.on_shortcut(shortcut_alt2(), move |_app, _sc, ev| {
        if ev.state == ShortcutState::Pressed {
            toggle_panel(&h2, "files");
        }
    }) {
        eprintln!("[shortcut] Alt+2 注册失败: {e}");
    }

    eprintln!("[shortcut] 已注册 Alt+1 / Alt+2");
}

/// 注销两个面板快捷键（把键交给前台程序，比如游戏）
fn unregister_panel_shortcuts(app: &AppHandle) {
    let gs = app.global_shortcut();
    let _ = gs.unregister(shortcut_alt1());
    let _ = gs.unregister(shortcut_alt2());
    eprintln!("[shortcut] 已暂停 Alt+1 / Alt+2（全屏游戏占用）");
}

/// 后台线程：全屏状态切换时暂停 / 恢复快捷键
fn spawn_fullscreen_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        let mut suspended = false;
        loop {
            let should_suspend = {
                let state = app.state::<PanelState>();
                state.pause_on_fullscreen() && is_fullscreen_app_running(&app)
            };

            if should_suspend != suspended {
                suspended = should_suspend;
                let app2 = app.clone();
                let _ = app.run_on_main_thread(move || {
                    let state = app2.state::<PanelState>();
                    state.shortcuts_suspended.store(suspended, Ordering::SeqCst);
                    if suspended {
                        unregister_panel_shortcuts(&app2);
                    } else {
                        // ★ 用 on_shortcut 恢复（重新绑定 handler）
                        register_panel_shortcuts(&app2);
                    }
                });
            }

            std::thread::sleep(std::time::Duration::from_millis(FULLSCREEN_POLL_MS));
        }
    });
}

// ============================================================
//  工作区 / 面板几何
// ============================================================
#[cfg(windows)]
fn work_area(w: &WebviewWindow) -> (i32, i32, i32, i32) {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };

    let m = w.current_monitor().ok().flatten()
        .or_else(|| w.primary_monitor().ok().flatten());
    let Some(m) = m else { return (0, 0, 1920, 1080); };
    let p = m.position();
    let s = m.size();
    let center = POINT {
        x: p.x + (s.width as i32) / 2,
        y: p.y + (s.height as i32) / 2,
    };
    unsafe {
        let hmon = MonitorFromPoint(center, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(hmon, &mut mi).as_bool() {
            let r = mi.rcWork;
            return (r.left, r.top, r.right - r.left, r.bottom - r.top);
        }
    }
    (p.x, p.y, s.width as i32, s.height as i32)
}

#[cfg(not(windows))]
fn work_area(w: &WebviewWindow) -> (i32, i32, i32, i32) {
    let m = w.current_monitor().ok().flatten()
        .or_else(|| w.primary_monitor().ok().flatten());
    match m {
        Some(m) => {
            let p = m.position();
            let s = m.size();
            (p.x, p.y, s.width as i32, s.height as i32)
        }
        None => (0, 0, 1920, 1080),
    }
}

fn panel_logical_width(app: &AppHandle, label: &str) -> f64 {
    let cfg = app.state::<PanelState>().config_snapshot();
    if label == "launcher" { cfg.launcher_width as f64 } else { cfg.files_width as f64 }
}

fn panel_geometry(
    app: &AppHandle,
    label: &str,
    w: &WebviewWindow,
) -> (i32, i32, i32, i32, i32) {
    let left = label == "launcher";
    let (wx, wy, ww, wh) = work_area(w);

    let scale = w.scale_factor().unwrap_or(1.0);
    let edge = (EDGE_MARGIN as f64 * scale).round() as i32;
    let top = (TOP_MARGIN as f64 * scale).round() as i32;
    let bottom = (BOTTOM_MARGIN as f64 * scale).round() as i32;

    let panel_w = (panel_logical_width(app, label) * scale).round() as i32;
    let panel_h = wh - top - bottom;
    let y = wy + top;

    let (start_x, end_x) = if left {
        (wx - panel_w - edge, wx + edge)
    } else {
        (wx + ww + edge, wx + ww - panel_w - edge)
    };
    (start_x, end_x, y, panel_w, panel_h)
}

fn do_show(app: &AppHandle, label: &str) {
    let Some(w) = app.get_webview_window(label) else { return; };
    let (start_x, end_x, y, panel_w, panel_h) = panel_geometry(app, label, &w);
    setup_window_decoration(&w);
    let _ = w.set_size(PhysicalSize::new(panel_w as u32, panel_h as u32));

    let blur = app.state::<PanelState>().config_snapshot().blur;
    apply_blur_effect(&w, blur);

    let cur_x = if w.is_visible().unwrap_or(false) {
        w.outer_position().map(|p| p.x).unwrap_or(start_x)
    } else {
        let _ = w.set_position(PhysicalPosition::new(start_x, y));
        start_x
    };
    let _ = w.show();
    let _ = w.set_focus();
    animate_window_then(
        app.clone(), w, label.to_string(),
        cur_x, end_x, y, SHOW_DURATION, || {},
    );
}

fn do_hide(app: &AppHandle, label: &str) {
    let Some(w) = app.get_webview_window(label) else { return; };
    if !w.is_visible().unwrap_or(false) { return; }

    let left = label == "launcher";
    let (wx, _wy, ww, _wh) = work_area(&w);
    let scale = w.scale_factor().unwrap_or(1.0);
    let edge = (EDGE_MARGIN as f64 * scale).round() as i32;
    let panel_w = (panel_logical_width(app, label) * scale).round() as i32;

    let cur_x = w.outer_position().map(|p| p.x).unwrap_or(wx);
    let y = w.outer_position().map(|p| p.y).unwrap_or(0);
    let end_x = if left { wx - panel_w - edge } else { wx + ww + edge };

    let w2 = w.clone();
    animate_window_then(
        app.clone(), w, label.to_string(),
        cur_x, end_x, y, HIDE_DURATION,
        move || { let _ = w2.hide(); },
    );
}

fn should_skip_due_to_fullscreen(app: &AppHandle) -> bool {
    let state = app.state::<PanelState>();
    if !state.pause_on_fullscreen() { return false; }
    if state.shortcuts_suspended.load(Ordering::SeqCst) { return true; }
    is_fullscreen_app_running(app)
}

fn toggle_panel(app: &AppHandle, label: &str) {
    if should_skip_due_to_fullscreen(app) { return; }
    let state = app.state::<PanelState>();
    let want = !state.get_target(label);
    state.set_target(label, want);
    if want { do_show(app, label); } else { do_hide(app, label); }
}

fn show_panel(app: &AppHandle, label: &str) {
    if should_skip_due_to_fullscreen(app) { return; }
    let state = app.state::<PanelState>();
    state.set_target(label, true);
    do_show(app, label);
}

fn auto_hide_panel(app: &AppHandle, label: &str) {
    let state = app.state::<PanelState>();
    state.set_target(label, false);
    do_hide(app, label);
}

fn animate_window_then<F>(
    app: AppHandle,
    w: WebviewWindow,
    label: String,
    x0: i32,
    x1: i32,
    y: i32,
    duration_ms: u64,
    on_done: F,
) where
    F: FnOnce() + Send + 'static,
{
    let token = {
        let state = app.state::<PanelState>();
        state.bump_token(&label)
    };

    std::thread::spawn(move || {
        let steps = (duration_ms / ANIM_STEP_MS).max(1);
        let sleep_ms = duration_ms / steps;

        for i in 1..=steps {
            {
                let state = app.state::<PanelState>();
                if !state.is_current(&label, token) { return; }
            }
            let t = i as f32 / steps as f32;
            let e = 1.0 - (1.0 - t).powi(3);
            let x = x0 + ((x1 - x0) as f32 * e) as i32;
            let _ = w.set_position(PhysicalPosition::new(x, y));
            std::thread::sleep(std::time::Duration::from_millis(sleep_ms));
        }
        {
            let state = app.state::<PanelState>();
            if !state.is_current(&label, token) { return; }
        }
        on_done();
    });
}

// ============================================================
//  Entry
// ============================================================
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let cfg = load_config();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .manage(PanelState::new(cfg))
        .invoke_handler(tauri::generate_handler![
            scan_desktop,
            scan_start,
            scan_uwp,
            scan_files,
            list_dir,
            icon,
            resolve_lnk,
            open,
            mk_folder,
            mk_file,
            delete_to_trash,
            rename_item,
            move_item,
            hide_panel_cmd,
            get_settings,
            set_setting,
            set_autostart,
            upload_font,
            remove_font,
            show_settings,
            hide_settings,
            list_visible_windows,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            for label in ["launcher", "files", "settings"] {
                if let Some(w) = app.get_webview_window(label) {
                    setup_window_decoration(&w);
                }
            }

            // ★ 注册快捷键（on_shortcut = register + handler）
            register_panel_shortcuts(&handle);

            // ★ 启动全屏监视
            spawn_fullscreen_watcher(handle.clone());

            let initial_real = {
                let cfg = app.state::<PanelState>().config_snapshot();
                cfg.icon_style != "redraw"
            };

            let open_l = MenuItem::with_id(
                app, "open_l", "打开软件面板  (Alt+1)", true, None::<&str>,
            )?;
            let open_f = MenuItem::with_id(
                app, "open_f", "打开文件面板  (Alt+2)", true, None::<&str>,
            )?;
            let sep1 = PredefinedMenuItem::separator(app)?;
            let refresh_item =
                MenuItem::with_id(app, "refresh", "刷新", true, None::<&str>)?;
            let sep2 = PredefinedMenuItem::separator(app)?;
            let real_icons_item = CheckMenuItem::with_id(
                app, "real_icons", "开启真实图标",
                true, initial_real, None::<&str>,
            )?;
            let settings_item =
                MenuItem::with_id(app, "settings", "设置…", true, None::<&str>)?;
            let sep3 = PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;

            let menu = Menu::with_items(
                app,
                &[
                    &open_l, &open_f,
                    &sep1, &refresh_item,
                    &sep2, &real_icons_item, &settings_item,
                    &sep3, &quit,
                ],
            )?;

            let real_icons_for_menu = real_icons_item.clone();

            let _tray = TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Floating Launcher")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, ev| match ev.id().as_ref() {
                    "open_l" => show_panel(app, "launcher"),
                    "open_f" => show_panel(app, "files"),
                    "refresh" => { let _ = app.emit("refresh-requested", ()); }
                    "settings" => show_settings(app.clone()),
                    "real_icons" => {
                        let state = app.state::<PanelState>();
                        let current = state.config_snapshot().icon_style;
                        let next = if current == "redraw" { "mask" } else { "redraw" };
                        let snapshot = {
                            let mut cfg = state.config.write().unwrap();
                            cfg.icon_style = next.to_string();
                            cfg.clone()
                        };
                        save_config(&snapshot);
                        let _ = real_icons_for_menu.set_checked(next != "redraw");
                        let _ = app.emit("settings-changed", &snapshot);
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up, ..
                    } = event {
                        toggle_panel(tray.app_handle(), "launcher");
                    }
                })
                .build(app)?;

            for label in ["launcher", "files"] {
                if let Some(w) = app.get_webview_window(label) {
                    let lbl = label.to_string();
                    let h = handle.clone();
                    w.on_window_event(move |e| {
                        if let WindowEvent::Focused(false) = e {
                            auto_hide_panel(&h, &lbl);
                        }
                    });
                }
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}