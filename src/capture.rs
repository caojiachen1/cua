// capture.rs — screenshots: PrintWindow(PW_RENDERFULLCONTENT) for background/occluded windows (Qoder WGC-occluded analog + ZCode background observation)
use anyhow::{bail, Result};
use image::RgbaImage;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    GetWindowDC, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    SRCCOPY,
};
use windows::Win32::Storage::Xps::{PW_CLIENTONLY, PrintWindow, PRINT_WINDOW_FLAGS};
use windows::Win32::UI::WindowsAndMessaging::PW_RENDERFULLCONTENT;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXSCREEN, SM_CXVIRTUALSCREEN, SM_CYSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

pub struct Shot {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub rendered: bool, // compat: false => nothing usable (e.g. DRM/minimized)
    /// graded capture result per the coordinate/capture contract:
    /// "captured" | "suspected_black" | "no_pixels" | "capture_failed".
    /// A legitimately black frame stays "captured" only if PrintWindow reported
    /// success; an all-black frame from a failed print is "suspected_black".
    pub status: &'static str,
}

pub fn shot_window(hwnd: HWND, client_only: bool) -> Result<Shot> {
    let (w, h) = window_pixel_size(hwnd)?;
    if w <= 0 || h <= 0 || w > 20000 || h > 20000 {
        bail!("bad window size {w}x{h}");
    }
    let shot = unsafe { print_window(hwnd, w, h, client_only) }?;
    Ok(shot)
}

pub fn shot_screen(all_monitors: bool) -> Result<Shot> {
    unsafe {
        let (x, y, w, h) = if all_monitors {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        } else {
            (0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))
        };
        if w <= 0 || h <= 0 {
            bail!("bad screen metrics");
        }
        let hdc = GetDC(windows::Win32::Foundation::HWND::default());
        let mem = CreateCompatibleDC(hdc);
        let bmp = CreateCompatibleBitmap(hdc, w, h);
        let obj: windows::Win32::Graphics::Gdi::HGDIOBJ = bmp.into();
        let old = SelectObject(mem, obj);
        BitBlt(mem, 0, 0, w, h, hdc, x, y, SRCCOPY).map_err(|e| anyhow::anyhow!("BitBlt screen: {e}"))?;
        let _ = SelectObject(mem, old);
        let mut shot = blit_to_rgba(mem, bmp, w, h);
        let _ = DeleteObject(obj);
        let _ = DeleteDC(mem);
        let _ = ReleaseDC(windows::Win32::Foundation::HWND::default(), hdc);
        shot.rendered = true;
        shot.status = "captured";
        Ok(shot)
    }
}

pub fn window_pixel_size(hwnd: HWND) -> Result<(i32, i32)> {
    let r = crate::win::window_rect(hwnd)?;
    Ok((r[2], r[3]))
}

unsafe fn print_window(hwnd: HWND, w: i32, h: i32, client_only: bool) -> Result<Shot> {
    let hdc = GetWindowDC(hwnd);
    if hdc.is_invalid() {
        bail!("GetWindowDC failed");
    }
    let mem = CreateCompatibleDC(hdc);
    let bmp = CreateCompatibleBitmap(hdc, w, h);
    let obj: windows::Win32::Graphics::Gdi::HGDIOBJ = bmp.into();
    let old = SelectObject(mem, obj);
    // PW_RENDERFULLCONTENT (0x2): includes DWM/DX content, works for occluded/background windows
    let mut flags = PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT);
    if client_only {
        flags = PRINT_WINDOW_FLAGS(flags.0 | PW_CLIENTONLY.0);
    }
    let ok = PrintWindow(hwnd, mem, flags).as_bool();
    // deselect before GetDIBits (documented requirement)
    let _ = SelectObject(mem, old);
    let mut shot = blit_to_rgba(mem, bmp, w, h);
    let _ = DeleteObject(obj);
    let _ = DeleteDC(mem);
    let _ = ReleaseDC(hwnd, hdc);
    // QQ Music's TXGuiFoundation returns FALSE from PrintWindow yet still fills the bitmap,
    // so the BOOL cannot decide on its own; the pixels do (minimized/DRM render nothing).
    let content = has_content(&shot.rgba);
    shot.rendered = content;
    shot.status = match (ok, content) {
        (true, true) => "captured",
        // black frame with a successful print may be legitimately dark content
        (true, false) => "suspected_black",
        // FALSE + no pixels: minimized/DRM window rendered nothing
        (false, false) => "no_pixels",
        // FALSE but pixels present: provider quirk (QQ Music), trust the pixels
        (false, true) => "captured",
    };
    Ok(shot)
}

/// Strided sample so a black letterbox bar at the top cannot mask a rendered frame.
fn has_content(rgba: &[u8]) -> bool {
    let px = rgba.len() / 4;
    if px == 0 {
        return false;
    }
    let stride = (px / 4096).max(1);
    (0..px).step_by(stride).any(|i| {
        let p = &rgba[i * 4..i * 4 + 4];
        p[0] as u32 + p[1] as u32 + p[2] as u32 > 0
    })
}

unsafe fn blit_to_rgba(
    mem: windows::Win32::Graphics::Gdi::HDC,
    bmp: windows::Win32::Graphics::Gdi::HBITMAP,
    w: i32,
    h: i32,
) -> Shot {
    let mut bi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        ..Default::default()
    };
    let mut buf = vec![0u8; (w as usize * h as usize * 4).max(4)];
    let _ = GetDIBits(
        mem,
        bmp,
        0,
        h as u32,
        Some(buf.as_mut_ptr() as *mut _),
        &mut bi,
        DIB_RGB_COLORS,
    );
    // BGRA -> RGBA
    for px in buf.chunks_exact_mut(4) {
        px.swap(0, 2);
        px[3] = 255;
    }
    Shot {
        width: w as u32,
        height: h as u32,
        rgba: buf,
        rendered: true,
        status: "captured",
    }
}

pub fn save_png(shot: &Shot, path: &std::path::Path) -> Result<()> {
    let img = RgbaImage::from_raw(shot.width, shot.height, shot.rgba.clone())
        .ok_or_else(|| anyhow::anyhow!("bad image buffer"))?;
    img.save_with_format(path, image::ImageFormat::Png)?;
    Ok(())
}

/// JPEG is what the screen recorder writes: an 8-bit RGB encode is ~10x smaller than PNG
/// and fast enough to keep up with a few frames per second.
pub fn save_jpeg(shot: &Shot, path: &std::path::Path, quality: u8) -> Result<()> {
    let img = RgbaImage::from_raw(shot.width, shot.height, shot.rgba.clone())
        .ok_or_else(|| anyhow::anyhow!("bad image buffer"))?;
    let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut f, quality).encode_image(&rgb)?;
    Ok(())
}
