// App icons for history. When a dictation lands in an app we have not seen
// before, its icon is pulled out of the executable once and cached as a PNG
// under history/icons/<key>.png. The window asks for them by key.

use std::path::PathBuf;

use tauri::{AppHandle, Manager};

fn dir(app: &AppHandle) -> Option<PathBuf> {
    let d = app.path().app_data_dir().ok()?.join("history").join("icons");
    let _ = std::fs::create_dir_all(&d);
    Some(d)
}

fn path(app: &AppHandle, key: &str) -> Option<PathBuf> {
    if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    dir(app).map(|d| d.join(format!("{key}.png")))
}

/// Extracts and caches the icon for `key` from `exe`, unless the cached copy
/// is at least as new as the executable. Apps change their icons when they
/// update, so a cache that never refreshes ends up showing the old one.
pub fn ensure(app: &AppHandle, key: &str, exe: &str) {
    let Some(p) = path(app, key) else { return };
    if exe.is_empty() || key == "neptune" || key == "blurt" {
        return;
    }
    let modified = |f: &std::path::Path| std::fs::metadata(f).and_then(|m| m.modified()).ok();
    if let (Some(png), Some(bin)) = (modified(&p), modified(std::path::Path::new(exe))) {
        if png >= bin {
            return;
        }
    }
    if let Some((w, h, rgba)) = extract(exe, 64) {
        let _ = write_png(&p, w, h, &rgba);
    }
}

/// The cached icon as a data URL, if we have one. neptune's own row always
/// shows the icon it was built with.
pub fn data_url(app: &AppHandle, key: &str) -> Option<String> {
    if key == "neptune" || key == "blurt" {
        return Some(format!("data:image/png;base64,{}", base64(include_bytes!("../icons/128x128.png"))));
    }
    let p = path(app, key)?;
    let bytes = std::fs::read(p).ok()?;
    Some(format!("data:image/png;base64,{}", base64(&bytes)))
}

fn write_png(p: &std::path::Path, w: u32, h: u32, rgba: &[u8]) -> anyhow::Result<()> {
    let f = std::fs::File::create(p)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header()?;
    wr.write_image_data(rgba)?;
    Ok(())
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// The executable's icon as RGBA at roughly `size` pixels.
#[cfg(windows)]
fn extract(exe: &str, size: u32) -> Option<(u32, u32, Vec<u8>)> {
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP,
    };
    use windows::Win32::UI::Shell::SHDefExtractIconW;
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

    let wide: Vec<u16> = exe.encode_utf16().chain(std::iter::once(0)).collect();
    let mut large = HICON::default();
    let mut small = HICON::default();
    unsafe {
        let hr = SHDefExtractIconW(PCWSTR(wide.as_ptr()), 0, 0, Some(&mut large), Some(&mut small), size);
        if hr.is_err() || large.is_invalid() {
            return None;
        }
        let mut info = ICONINFO::default();
        let ok = GetIconInfo(large, &mut info).is_ok();
        let result = if ok { bitmap_rgba(info.hbmColor, info.hbmMask) } else { None };
        if !info.hbmColor.is_invalid() {
            let _ = DeleteObject(info.hbmColor);
        }
        if !info.hbmMask.is_invalid() {
            let _ = DeleteObject(info.hbmMask);
        }
        let _ = DestroyIcon(large);
        if !small.is_invalid() {
            let _ = DestroyIcon(small);
        }
        return result;

        /// Reads a colour bitmap (and its 1-bit mask, for icons without an
        /// alpha channel) into top-down RGBA.
        unsafe fn bitmap_rgba(color: HBITMAP, mask: HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
            let dc = CreateCompatibleDC(None);
            let mut bi = BITMAPINFO::default();
            bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            // First call fills in the dimensions.
            if GetDIBits(dc, color, 0, 0, None, &mut bi, DIB_RGB_COLORS) == 0 {
                let _ = DeleteDC(dc);
                return None;
            }
            let w = bi.bmiHeader.biWidth.unsigned_abs();
            let h = bi.bmiHeader.biHeight.unsigned_abs();
            if w == 0 || h == 0 || w > 512 || h > 512 {
                let _ = DeleteDC(dc);
                return None;
            }
            let read = |bm: HBITMAP| -> Option<Vec<u8>> {
                let mut bi = BITMAPINFO::default();
                bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
                bi.bmiHeader.biWidth = w as i32;
                bi.bmiHeader.biHeight = -(h as i32); // top-down
                bi.bmiHeader.biPlanes = 1;
                bi.bmiHeader.biBitCount = 32;
                bi.bmiHeader.biCompression = BI_RGB.0;
                let mut buf = vec![0u8; (w * h * 4) as usize];
                let n = GetDIBits(dc, bm, 0, h, Some(buf.as_mut_ptr() as *mut _), &mut bi, DIB_RGB_COLORS);
                (n != 0).then_some(buf)
            };
            let mut px = read(color)?;
            let has_alpha = px.chunks(4).any(|p| p[3] != 0);
            if !has_alpha && !mask.is_invalid() {
                if let Some(m) = read(mask) {
                    for (p, mp) in px.chunks_mut(4).zip(m.chunks(4)) {
                        p[3] = if mp[0] == 0 { 255 } else { 0 };
                    }
                }
            } else if !has_alpha {
                for p in px.chunks_mut(4) {
                    p[3] = 255;
                }
            }
            let _ = DeleteDC(dc);
            // BGRA -> RGBA
            for p in px.chunks_mut(4) {
                p.swap(0, 2);
            }
            Some((w, h, px))
        }
    }
}

#[cfg(not(windows))]
fn extract(_exe: &str, _size: u32) -> Option<(u32, u32, Vec<u8>)> {
    None
}
