//! 图标获取：把文件 / 快捷方式的系统图标抓成 `data:image/png;base64,...`
//!
//! - `shell:` 开头的（UWP 命名空间）没有文件图标，返回 None，由前端生成字母图标
//! - 非 Windows 平台统一返回 None

/// 抓文件 / 快捷方式图标，返回 `data:image/png;base64,...`
pub fn icon_data_url(path: &str) -> Option<String> {
    if path.starts_with("shell:") {
        return None;
    }

    #[cfg(windows)]
    {
        if let Some(s) = windows_impl(path) {
            return Some(s);
        }
    }
    None
}

// ============================================================
//  Windows 实现
// ============================================================
#[cfg(windows)]
fn windows_impl(path: &str) -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{
        SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON,
    };
    use windows::Win32::UI::WindowsAndMessaging::DestroyIcon;

    let wide = to_wide(path);

    unsafe {
        let mut info = SHFILEINFOW::default();
        let r = SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            Default::default(),
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if r == 0 || info.hIcon.is_invalid() {
            return None;
        }

        let hicon = info.hIcon;
        let png = hicon_to_png(hicon);
        let _ = DestroyIcon(hicon);
        png
    }
}

/// 字符串 -> 以 0 结尾的 UTF-16 宽字符
#[cfg(windows)]
fn to_wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    let mut v: Vec<u16> = std::ffi::OsStr::new(s).encode_wide().collect();
    v.push(0);
    v
}

/// HICON -> PNG data URL
#[cfg(windows)]
unsafe fn hicon_to_png(
    hicon: windows::Win32::UI::WindowsAndMessaging::HICON,
) -> Option<String> {
    use windows::Win32::Graphics::Gdi::{
        DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP,
        BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetIconInfo, ICONINFO};

    let mut ii = ICONINFO::default();
    GetIconInfo(hicon, &mut ii).ok()?;

    let mut bm = BITMAP::default();
    let got = GetObjectW(
        ii.hbmColor, // HBITMAP 本身实现 Param<HGDIOBJ>，不要 .into()
        std::mem::size_of::<BITMAP>() as i32,
        Some(&mut bm as *mut _ as *mut _),
    );
    if got == 0 {
        let _ = DeleteObject(ii.hbmColor);
        let _ = DeleteObject(ii.hbmMask);
        return None;
    }

    let w = bm.bmWidth;
    let h = bm.bmHeight;

    let mut bmi = BITMAPINFO::default();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = w;
    bmi.bmiHeader.biHeight = -h; // 负数 = top-down
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB.0;

    let mut px = vec![0u8; (w * h * 4) as usize];

    // GetDC 返回 HDC，不要标成 HWND
    let hdc = GetDC(None);
    let ok = GetDIBits(
        hdc,
        ii.hbmColor,
        0,
        h as u32,
        Some(px.as_mut_ptr() as *mut _),
        &mut bmi,
        DIB_RGB_COLORS,
    );
    ReleaseDC(None, hdc);

    let _ = DeleteObject(ii.hbmColor);
    let _ = DeleteObject(ii.hbmMask);
    if ok == 0 {
        return None;
    }

    // 系统图标是 BGRA，image crate 要 RGBA
    for c in px.chunks_exact_mut(4) {
        c.swap(0, 2);
    }

    let img = image::RgbaImage::from_raw(w as u32, h as u32, px)?;
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).ok()?;

    use base64::{engine::general_purpose::STANDARD, Engine};
    Some(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(buf.into_inner())
    ))
}