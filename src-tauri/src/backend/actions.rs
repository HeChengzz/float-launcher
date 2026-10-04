use std::fs;
use std::path::{Path, PathBuf};

use crate::backend::paths::desktop_path;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// ============================================================
//  打开
// ============================================================
pub fn open_path(path: &str) -> Result<(), String> {
    if path.starts_with("shell:") {
        std::process::Command::new("explorer.exe")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("cmd")
            .args(["/C", "start", "", path])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(windows))]
    {
        opener::open(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ============================================================
//  新建
// ============================================================
fn unique_path(parent: &Path, name: &str) -> PathBuf {
    let (base, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    let mut candidate = parent.join(name);
    let mut n = 2;
    while candidate.exists() {
        candidate = parent.join(format!("{base} ({n}){ext}"));
        n += 1;
    }
    candidate
}

pub fn create_folder(name: &str, parent: Option<&str>) -> Result<String, String> {
    let parent = parent
        .map(PathBuf::from)
        .unwrap_or_else(desktop_path);
    let p = unique_path(&parent, name);
    fs::create_dir(&p).map_err(|e| e.to_string())?;
    Ok(p.to_string_lossy().into_owned())
}

pub fn create_file(name: &str, parent: Option<&str>) -> Result<String, String> {
    let parent = parent
        .map(PathBuf::from)
        .unwrap_or_else(desktop_path);
    let p = unique_path(&parent, name);
    fs::write(&p, b"").map_err(|e| e.to_string())?;
    Ok(p.to_string_lossy().into_owned())
}

// ============================================================
//  重命名
// ============================================================
pub fn rename_path(path: &str, new_name: &str) -> Result<String, String> {
    let p = PathBuf::from(path);
    let new_name = new_name.trim();

    if new_name.is_empty() {
        return Err("名称不能为空".into());
    }

    const INVALID: &[char] = &['\\', '/', ':', '*', '?', '"', '<', '>', '|'];
    if new_name.contains(INVALID) {
        return Err("名称包含非法字符 \\ / : * ? \" < > |".into());
    }

    let parent = p.parent().ok_or("无法获取父目录")?;

    let ext = p
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    let final_name = if !ext.is_empty() && !new_name.contains('.') {
        format!("{new_name}.{ext}")
    } else {
        new_name.to_string()
    };

    let new_path = parent.join(&final_name);

    if new_path == p {
        return Ok(p.to_string_lossy().into_owned());
    }

    if new_path.exists() {
        return Err(format!("「{final_name}」已存在"));
    }

    fs::rename(&p, &new_path).map_err(|e| e.to_string())?;
    Ok(new_path.to_string_lossy().into_owned())
}

// ============================================================
//  移动到指定文件夹
// ============================================================
pub fn move_item(src: &str, dst_dir: &str) -> Result<String, String> {
    let src = PathBuf::from(src);
    let dst_dir = PathBuf::from(dst_dir);

    if !src.exists() {
        return Err("源文件不存在".into());
    }
    if !dst_dir.is_dir() {
        return Err("目标不是文件夹".into());
    }

    let file_name = src
        .file_name()
        .ok_or("无效的源路径")?
        .to_owned();
    let target = dst_dir.join(&file_name);

    // 已经在目标文件夹里
    if src.parent() == Some(dst_dir.as_path()) {
        return Err("已经在目标文件夹里了".into());
    }

    // 目标已存在同名项
    if target.exists() {
        return Err(format!(
            "目标文件夹里已有「{}」",
            file_name.to_string_lossy()
        ));
    }

    // 防止把文件夹拖进自己的子目录
    if src.is_dir() {
        if let (Ok(src_canon), Ok(dst_canon)) =
            (src.canonicalize(), dst_dir.canonicalize())
        {
            if dst_canon.starts_with(&src_canon) {
                return Err("不能把文件夹移动到它自己里面".into());
            }
        }
    }

    // 先尝试 rename（同盘很快）
    match fs::rename(&src, &target) {
        Ok(_) => Ok(target.to_string_lossy().into_owned()),
        Err(_) => {
            // 跨盘 → copy + delete
            if src.is_dir() {
                copy_dir_recursive(&src, &target)?;
                fs::remove_dir_all(&src).map_err(|e| e.to_string())?;
            } else {
                fs::copy(&src, &target).map_err(|e| e.to_string())?;
                fs::remove_file(&src).map_err(|e| e.to_string())?;
            }
            Ok(target.to_string_lossy().into_owned())
        }
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

// ============================================================
//  移动到回收站
// ============================================================
#[cfg(windows)]
pub fn delete_to_trash(path: &str) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{SHFileOperationW, SHFILEOPSTRUCTW};

    const FO_DELETE: u32 = 3;
    const FLAGS: u16 = 0x0454;

    let abs = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(path)
    };

    let mut from: Vec<u16> = abs.as_os_str().encode_wide().collect();
    from.push(0);
    from.push(0);

    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        fFlags: FLAGS,
        ..Default::default()
    };

    let r = unsafe { SHFileOperationW(&mut op) };
    if r != 0 {
        return Err(format!("SHFileOperationW 失败，错误码 {r}"));
    }
    if op.fAnyOperationsAborted.as_bool() {
        return Err("操作已取消".into());
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn delete_to_trash(_path: &str) -> Result<(), String> {
    Err("当前平台不支持移动到回收站".into())
}