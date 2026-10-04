mod backend;

use backend::item::Item;
use backend::{actions, icon::icon_data_url, lnk::resolve_lnk_target, scanner::*};

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Mutex, RwLock};
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

// ============================================================
//  系统检测
// ============================================================
#[cfg(windows)]
fn windows_build() -> u32 {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    if let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
    {
        if let Ok(build) = key.get_value::<String, _>("CurrentBuildNumber") {
            if let Ok(n) = build.parse::<u32>() {
                return n;
            }
        }
    }
    0
}

#[cfg(not(windows))]
fn windows_build() -> u32 { 0 }

#[cfg(windows)]
fn accent_color() -> String {
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::Graphics::Dwm::DwmGetColorizationColor;

    unsafe {
        let mut color: u32 = 0;
        let mut opaque = BOOL::default();
        if DwmGetColorizationColor(&mut color, &mut opaque).is_ok() {
            let r = (color >> 16) & 0xFF;
            let g = (color >> 8) & 0xFF;
            let b = color & 0xFF;
            return format!("#{:02X}{:02X}{:02X}", r, g, b);
        }
    }
    "#0078D4".to_string()
}

#[cfg(not(windows))]
fn accent_color() -> String { "#0078D4".to_string() }

// ============================================================
//  材质
// ============================================================
static ACTUAL_EFFECT: Mutex<Option<String>> = Mutex::new(None);

fn apply_effects(w: &WebviewWindow) {
    let app = w.app_handle();

    let mica = WindowEffectsConfig {
        effects: vec![Effect::Mica],
        interactive: false,
        ..Default::default()
    };
    if w.set_effects(mica).is_ok() {
        *ACTUAL_EFFECT.lock().unwrap() = Some("mica".into());
        eprintln!("[effects] Mica 已应用: {}", w.label());
        let _ = app.emit("system-effect-changed", "mica");
        return;
    }

    let acrylic = WindowEffectsConfig {
        effects: vec![Effect::Acrylic],
        interactive: false,
        ..Default::default()
    };
    if w.set_effects(acrylic).is_ok() {
        *ACTUAL_EFFECT.lock().unwrap() = Some("acrylic".into());
        eprintln!("[effects] Acrylic 已应用: {}", w.label());
        let _ = app.emit("system-effect-changed", "acrylic");
        return;
    }

    *ACTUAL_EFFECT.lock().unwrap() = Some("none".into());
    eprintln!("[effects] 系统不支持材质: {}", w.label());
    let _ = app.emit("system-effect-changed", "none");
}

fn apply_effects_deferred(w: WebviewWindow, delay_ms: u64) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        let w2 = w.clone();
        let app = w.app_handle().clone();
        let _ = app.run_on_main_thread(move || {
            apply_effects(&w2);
        });
    });
}

// ============================================================
//  独占全屏
// ============================================================
#[cfg(windows)]
fn is_fullscreen_app_running() -> bool {
    use windows::Win32::UI::Shell::{
        SHQueryUserNotificationState, QUNS_RUNNING_D3D_FULL_SCREEN,
    };
    unsafe {
        if let Ok(state) = SHQueryUserNotificationState() {
            if state == QUNS_RUNNING_D3D_FULL_SCREEN {
                return true;
            }
        }
    }
    false
}

#[cfg(not(windows))]
fn is_fullscreen_app_running() -> bool { false }

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
    #[serde(default = "default_theme")]
    theme: String,
    #[serde(default = "default_icon_style")]
    icon_style: String,
    #[serde(default = "default_icon_size")]
    icon_size: u32,
    #[serde(default = "default_font_size")]
    font_size: u32,
    #[serde(default = "default_font_family")]
    font_family: String,
    #[serde(default = "default_mica_strength")]
    mica_strength: u32,
    /// 面板整体不透明度 0~100
    #[serde(default = "default_opacity")]
    opacity: u32,
    #[serde(default)]
    custom_fonts: Vec<CustomFont>,
    #[serde(default = "default_pause_on_fullscreen")]
    pause_on_fullscreen: bool,
}

fn default_theme() -> String { "system".into() }
fn default_icon_style() -> String { "mask".into() }
fn default_icon_size() -> u32 { 48 }
fn default_font_size() -> u32 { 12 }
fn default_font_family() -> String { "segoe".into() }
fn default_mica_strength() -> u32 { 30 }
fn default_opacity() -> u32 { 100 }
fn default_pause_on_fullscreen() -> bool { true }

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            icon_style: default_icon_style(),
            icon_size: default_icon_size(),
            font_size: default_font_size(),
            font_family: default_font_family(),
            mica_strength: default_mica_strength(),
            opacity: default_opacity(),
            custom_fonts: Vec::new(),
            pause_on_fullscreen: default_pause_on_fullscreen(),
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
    config: RwLock<Config>,
}

impl PanelState {
    fn new(config: Config) -> Self {
        Self {
            launcher_token: AtomicI32::new(0),
            launcher_target: AtomicBool::new(false),
            files_token: AtomicI32::new(0),
            files_target: AtomicBool::new(false),
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
//  Payloads
// ============================================================
#[derive(serde::Serialize)]
struct SettingsPayload {
    theme: String,
    icon_style: String,
    icon_size: u32,
    font_size: u32,
    font_family: String,
    mica_strength: u32,
    opacity: u32,
    custom_fonts: Vec<CustomFont>,
    pause_on_fullscreen: bool,
    autostart: bool,
    version: String,
}

#[derive(serde::Serialize)]
struct SystemInfo {
    build: u32,
    effect: String,
    accent: String,
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
fn get_system_info() -> SystemInfo {
    let effect = ACTUAL_EFFECT
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_else(|| "none".to_string());
    SystemInfo {
        build: windows_build(),
        effect,
        accent: accent_color(),
    }
}

#[tauri::command]
fn get_settings(app: AppHandle) -> SettingsPayload {
    let cfg = app.state::<PanelState>().config_snapshot();
    let autostart = app.autolaunch().is_enabled().unwrap_or(false);
    SettingsPayload {
        theme: cfg.theme,
        icon_style: cfg.icon_style,
        icon_size: cfg.icon_size,
        font_size: cfg.font_size,
        font_family: cfg.font_family,
        mica_strength: cfg.mica_strength,
        opacity: cfg.opacity,
        custom_fonts: cfg.custom_fonts,
        pause_on_fullscreen: cfg.pause_on_fullscreen,
        autostart,
        version: env!("CARGO_PKG_VERSION").to_string(),
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
            "theme" => {
                let v = value.as_str().unwrap_or("system");
                let v = match v {
                    "light" | "dark" | "system" => v,
                    _ => return Err("invalid theme".into()),
                };
                cfg.theme = v.to_string();
            }
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
            "mica_strength" => {
                cfg.mica_strength = value.as_u64().unwrap_or(30).clamp(0, 100) as u32;
            }
            "opacity" => {
                cfg.opacity = value.as_u64().unwrap_or(100).clamp(0, 100) as u32;
            }
            "pause_on_fullscreen" => {
                cfg.pause_on_fullscreen = value.as_bool().unwrap_or(true);
            }
            _ => return Err(format!("unknown key: {key}")),
        }
        cfg.clone()
    };

    save_config(&snapshot);
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
        let _ = w.center();
        let _ = w.show();
        let _ = w.set_focus();
        apply_effects_deferred(w.clone(), 100);
    }
}

#[tauri::command]
fn hide_settings(app: AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.hide();
    }
}

// ============================================================
//  工作区
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

fn panel_geometry(label: &str, w: &WebviewWindow) -> (i32, i32, i32, i32, i32) {
    let left = label == "launcher";
    let (wx, wy, ww, wh) = work_area(w);
    let panel_w = w.outer_size().map(|s| s.width as i32)
        .unwrap_or(if left { 480 } else { 400 });
    let panel_h = wh - TOP_MARGIN - BOTTOM_MARGIN;
    let y = wy + TOP_MARGIN;
    let (start_x, end_x) = if left {
        (wx - panel_w - EDGE_MARGIN, wx + EDGE_MARGIN)
    } else {
        (wx + ww + EDGE_MARGIN, wx + ww - panel_w - EDGE_MARGIN)
    };
    (start_x, end_x, y, panel_w, panel_h)
}

fn do_show(app: &AppHandle, label: &str) {
    let Some(w) = app.get_webview_window(label) else { return; };
    let (start_x, end_x, y, panel_w, panel_h) = panel_geometry(label, &w);
    let _ = w.set_size(PhysicalSize::new(panel_w as u32, panel_h as u32));

    let cur_x = if w.is_visible().unwrap_or(false) {
        w.outer_position().map(|p| p.x).unwrap_or(start_x)
    } else {
        let _ = w.set_position(PhysicalPosition::new(start_x, y));
        start_x
    };
    let _ = w.show();
    let _ = w.set_focus();
    apply_effects_deferred(w.clone(), 100);
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
    let panel_w = w.outer_size().map(|s| s.width as i32).unwrap_or(480);
    let cur_x = w.outer_position().map(|p| p.x).unwrap_or(wx);
    let y = w.outer_position().map(|p| p.y).unwrap_or(TOP_MARGIN);
    let end_x = if left {
        wx - panel_w - EDGE_MARGIN
    } else {
        wx + ww + EDGE_MARGIN
    };

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
    is_fullscreen_app_running()
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
            icon,
            resolve_lnk,
            open,
            mk_folder,
            mk_file,
            delete_to_trash,
            rename_item,
            move_item,
            hide_panel_cmd,
            get_system_info,
            get_settings,
            set_setting,
            set_autostart,
            upload_font,
            remove_font,
            show_settings,
            hide_settings,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            let alt1 = Shortcut::new(Some(Modifiers::ALT), Code::Digit1);
            let alt2 = Shortcut::new(Some(Modifiers::ALT), Code::Digit2);

            let h1 = handle.clone();
            app.global_shortcut().on_shortcut(alt1, move |_app, _sc, ev| {
                if ev.state == ShortcutState::Pressed {
                    toggle_panel(&h1, "launcher");
                }
            })?;

            let h2 = handle.clone();
            app.global_shortcut().on_shortcut(alt2, move |_app, _sc, ev| {
                if ev.state == ShortcutState::Pressed {
                    toggle_panel(&h2, "files");
                }
            })?;

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
                    &sep1,
                    &refresh_item,
                    &sep2,
                    &real_icons_item, &settings_item,
                    &sep3,
                    &quit,
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
                    "refresh" => {
                        let _ = app.emit("refresh-requested", ());
                        eprintln!("[tray] 已请求刷新");
                    }
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