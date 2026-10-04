use std::path::{Path, PathBuf};

/// 解析 .lnk 指向的真实路径；失败时返回原路径
pub fn resolve_lnk_target(lnk_path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(p) = resolve_windows(lnk_path) {
            return p;
        }
    }
    lnk_path.to_path_buf()
}

#[cfg(windows)]
fn resolve_windows(lnk_path: &Path) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED, STGM_READ,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let link: IShellLinkW =
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let file: IPersistFile = link.cast().ok()?;

        let wide: Vec<u16> = lnk_path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        file.Load(PCWSTR(wide.as_ptr()), STGM_READ).ok()?;

        let mut buf = [0u16; 1024];
        let mut fd = WIN32_FIND_DATAW::default();
        // SLGP_RAWPATH = 0x00000004
        link.GetPath(&mut buf, &mut fd, 0x00000004).ok()?;

        let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
        if n == 0 {
            return None;
        }
        let s = String::from_utf16_lossy(&buf[..n]);
        let p = PathBuf::from(s);
        if p.exists() {
            Some(p)
        } else {
            None
        }
    }
}