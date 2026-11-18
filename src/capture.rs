// capture.rs — screenshots: PrintWindow(PW_RENDERFULLCONTENT) for background/occluded windows (Qoder WGC-occluded analog + ZCode background observation)
use anyhow::{bail, Result};
use image::RgbaImage;
use std::os::windows::ffi::OsStrExt as _;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    GetObjectW, GetWindowDC, ReleaseDC, SelectObject, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, SRCCOPY,
};
use windows::Win32::Graphics::GdiPlus::{
    EncoderParameter, EncoderParameters, EncoderQuality, GdiplusShutdown, GdiplusStartup,
    GdiplusStartupInput, GdipCreateBitmapFromScan0, GdipDisposeImage, GdipGetImageEncoders,
    GdipGetImageEncodersSize, GdipSaveImageToFile, GpBitmap, GpImage, ImageCodecInfo, Status,
};
use windows::Win32::Storage::Xps::{PW_CLIENTONLY, PrintWindow, PRINT_WINDOW_FLAGS};
use windows::Win32::UI::WindowsAndMessaging::{
    CURSORINFO, CURSOR_SHOWING, DI_NORMAL, DrawIconEx, GetCursorInfo, GetIconInfo, ICONINFO,
    PW_RENDERFULLCONTENT,
};
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

// ---------------- recorder fast path (`cua rec capture`) ----------------
//
// The recorder's hot loop cannot use shot_screen/save_jpeg: shot_screen
// allocates a 13 MB buffer and rebuilds GDI objects per frame, and the image
// crate's pure-Rust JPEG encoder needs 200-400 ms per 2K frame — together
// that capped a full-screen recording at ~1.6 fps. This path keeps the GDI
// objects and buffer alive across frames, feeds the BGRA DIB scan straight
// into GDI+'s (C-speed) JPEG encoder with zero conversion passes, and bakes
// the mouse cursor into the frame — BitBlt alone does NOT include the
// pointer, so a recording without it could not be used to judge the pointer
// preview against the real cursor.

/// BGRA, top-down, 32bpp — the layout of GetDIBits and of GDI+'
/// PixelFormat32bppARGB (little-endian ARGB dword), so the raw DIB scan can
/// be handed to the JPEG encoder unchanged.
pub struct ScreenGrabber {
    hdc: HDC,
    mem: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    w: i32,
    h: i32,
    vx: i32,
    vy: i32,
    buf: Vec<u8>,
}

impl ScreenGrabber {
    pub fn new(all_monitors: bool) -> Result<Self> {
        unsafe {
            let (vx, vy, w, h) = if all_monitors {
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
            let hdc = GetDC(HWND::default());
            if hdc.is_invalid() {
                bail!("GetDC screen failed");
            }
            let mem = CreateCompatibleDC(hdc);
            let bmp = CreateCompatibleBitmap(hdc, w, h);
            if mem.is_invalid() || bmp.is_invalid() {
                ReleaseDC(HWND::default(), hdc);
                bail!("screen grabber GDI setup failed");
            }
            let obj: HGDIOBJ = bmp.into();
            let old = SelectObject(mem, obj);
            Ok(Self {
                hdc,
                mem,
                bmp,
                old,
                w,
                h,
                vx,
                vy,
                buf: vec![0u8; (w as usize * h as usize * 4).max(4)],
            })
        }
    }

    /// BitBlt the screen into the reused buffer (BGRA, top-down) and return
    /// the scan. The borrow ends at the next grab.
    pub fn grab(&mut self, with_cursor: bool) -> Result<&[u8]> {
        unsafe {
            BitBlt(self.mem, 0, 0, self.w, self.h, self.hdc, self.vx, self.vy, SRCCOPY)
                .map_err(|e| anyhow::anyhow!("BitBlt screen: {e}"))?;
            if with_cursor {
                self.draw_cursor();
            }
            let mut bi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: self.w,
                    biHeight: -self.h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            if GetDIBits(
                self.mem,
                self.bmp,
                0,
                self.h as u32,
                Some(self.buf.as_mut_ptr() as *mut _),
                &mut bi,
                DIB_RGB_COLORS,
            ) == 0
            {
                bail!("GetDIBits failed");
            }
            Ok(&self.buf)
        }
    }

    /// Bake the current mouse cursor into the frame at its true position —
    /// without it the recording cannot show where real input lands.
    /// Frame dimensions (physical px), fixed at construction.
    pub fn size(&self) -> (u32, u32) {
        (self.w as u32, self.h as u32)
    }

    unsafe fn draw_cursor(&mut self) {
        let mut ci = CURSORINFO { cbSize: std::mem::size_of::<CURSORINFO>() as u32, ..Default::default() };
        if GetCursorInfo(&mut ci).is_err() || (ci.flags.0 & CURSOR_SHOWING.0) == 0 || ci.hCursor.is_invalid() {
            return;
        }
        let mut ii = ICONINFO::default();
        if GetIconInfo(ci.hCursor, &mut ii).is_err() {
            return;
        }
        let (mut cw, mut ch) = (32i32, 32i32);
        if !ii.hbmColor.is_invalid() {
            // colour cursor: the colour bitmap IS the cursor at natural size
            let mut bm = BITMAP::default();
            if GetObjectW(
                ii.hbmColor,
                std::mem::size_of::<BITMAP>() as i32,
                Some(&mut bm as *mut _ as *mut core::ffi::c_void),
            ) != 0
                && bm.bmWidth > 0
                && bm.bmHeight > 0
            {
                cw = bm.bmWidth;
                ch = bm.bmHeight;
            }
            let _ = DeleteObject(HGDIOBJ(ii.hbmColor.0));
        }
        if !ii.hbmMask.is_invalid() {
            // monochrome cursor: the mask stacks the AND plane on the XOR
            // plane, so its height is twice the cursor's — halve it, but only
            // when there is no colour bitmap (halving a colour cursor's mask
            // squashes the arrow flat)
            let mut bm = BITMAP::default();
            if GetObjectW(
                ii.hbmMask,
                std::mem::size_of::<BITMAP>() as i32,
                Some(&mut bm as *mut _ as *mut core::ffi::c_void),
            ) != 0
                && bm.bmWidth > 0
                && bm.bmHeight >= 2
            {
                cw = bm.bmWidth;
                if ii.hbmColor.is_invalid() {
                    ch = bm.bmHeight / 2;
                }
            }
            let _ = DeleteObject(HGDIOBJ(ii.hbmMask.0));
        }
        let _ = DrawIconEx(
            self.mem,
            ci.ptScreenPos.x - self.vx - ii.xHotspot as i32,
            ci.ptScreenPos.y - self.vy - ii.yHotspot as i32,
            ci.hCursor,
            cw,
            ch,
            0,
            None,
            DI_NORMAL,
        );
    }
}

impl Drop for ScreenGrabber {
    fn drop(&mut self) {
        unsafe {
            let _ = SelectObject(self.mem, self.old);
            let _ = DeleteObject(HGDIOBJ(self.bmp.0));
            let _ = DeleteDC(self.mem);
            let _ = ReleaseDC(HWND::default(), self.hdc);
        }
    }
}

/// GDI+ JPEG encoder. `new` starts GDI+ once; the C encoder scales far better
/// than the image crate's pure-Rust one and takes the BGRA scan as-is, so the
/// recorder needs no RGBA conversion pass at all.
pub struct GdiJpeg {
    token: usize,
    jpeg_clsid: windows::core::GUID,
}

/// JPEG codec format identifier {B96B3CAE-0728-11D3-9D7B-0000F81EF32E}
/// (the windows crate does not export it as a constant).
const GUID_FORMAT_JPEG: windows::core::GUID =
    windows::core::GUID::from_u128(0xB96B3CAE_0728_11D3_9D7B_0000F81EF32E);

impl GdiJpeg {
    pub fn new() -> Result<Self> {
        unsafe {
            let si = GdiplusStartupInput {
                GdiplusVersion: 1,
                DebugEventCallback: 0,
                SuppressBackgroundThread: windows::Win32::Foundation::BOOL(0),
                SuppressExternalCodecs: windows::Win32::Foundation::BOOL(0),
            };
            let mut token = 0usize;
            if GdiplusStartup(&mut token, &si, std::ptr::null_mut()) != Status(0) {
                bail!("GdiplusStartup failed");
            }
            let (mut n, mut size) = (0u32, 0u32);
            if GdipGetImageEncodersSize(&mut n, &mut size) != Status(0) || size == 0 {
                bail!("GDI+ encoder enumeration failed");
            }
            let mut buf = vec![0u8; size as usize];
            if GdipGetImageEncoders(n, size, buf.as_mut_ptr() as *mut ImageCodecInfo) != Status(0) {
                bail!("GDI+ encoder list failed");
            }
            let codecs = buf.as_ptr() as *const ImageCodecInfo;
            let mut jpeg_clsid = None;
            for k in 0..n {
                let c = &*codecs.offset(k as isize);
                if c.FormatID == GUID_FORMAT_JPEG {
                    jpeg_clsid = Some(c.Clsid);
                    break;
                }
            }
            let Some(jpeg_clsid) = jpeg_clsid else {
                bail!("no GDI+ JPEG codec");
            };
            Ok(Self { token, jpeg_clsid })
        }
    }

    /// Encode a BGRA scan as JPEG (alpha is ignored) and write it to `path`.
    pub fn save(&self, bgra: &[u8], w: u32, h: u32, quality: u8, path: &std::path::Path) -> Result<()> {
        unsafe {
            let mut bmp: *mut GpBitmap = std::ptr::null_mut();
            if GdipCreateBitmapFromScan0(
                w as i32,
                h as i32,
                (w * 4) as i32,
                0x0026200A, // PixelFormat32bppARGB
                Some(bgra.as_ptr()),
                &mut bmp,
            ) != Status(0)
            {
                bail!("GdipCreateBitmapFromScan0 failed");
            }
            let q: u32 = quality as u32;
            let mut params = EncoderParameters {
                Count: 1,
                Parameter: [EncoderParameter {
                    Guid: EncoderQuality,
                    Type: 4, // ValueTagTypeULONG
                    NumberOfValues: 1,
                    Value: &q as *const u32 as *mut core::ffi::c_void,
                }],
            };
            let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
            wide.push(0);
            let status = GdipSaveImageToFile(
                bmp as *mut GpImage,
                PCWSTR::from_raw(wide.as_ptr()),
                &self.jpeg_clsid,
                &params,
            );
            GdipDisposeImage(bmp as *mut GpImage);
            if status != Status(0) {
                bail!("GDI+ JPEG encode failed: {status:?}");
            }
            Ok(())
        }
    }
}

impl Drop for GdiJpeg {
    fn drop(&mut self) {
        unsafe {
            GdiplusShutdown(self.token);
        }
    }
}
