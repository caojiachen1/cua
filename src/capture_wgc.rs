// capture_wgc.rs — Windows Graphics Capture path for window capture.
//
// Why this exists: PrintWindow(PW_RENDERFULLCONTENT) returns a blank frame for
// many GPU-composited apps (Chromium/Electron, some games) and for windows that
// don't answer WM_PRINT. WGC composites the real window surface, so occluded and
// uncooperative windows still produce pixels.
//
// Constraints (documented, not bugs):
//   - minimized windows cannot be captured by WGC either -> caller must fall back
//     or report WINDOW_MINIMIZED
//   - requires Win10 1803+; on failure the caller falls back to PrintWindow -> BitBlt
//   - needs a D3D11 device; on GPU-less/RDP sessions device creation may fail

use anyhow::{anyhow, bail, Result};
use windows::core::{factory, Interface};
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;

pub struct WgcFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

struct Device {
    d3d: ID3D11Device,
    winrt: IDirect3DDevice,
}

fn create_device() -> Result<Device> {
    // WinRT must be initialized before RoGetActivationFactory (used by
    // windows::core::factory). Ignore RPC_E_CHANGED_MODE when the thread is
    // already in an STA.
    unsafe {
        use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }
    unsafe {
        let mut d3d: Option<ID3D11Device> = None;
        let levels = [D3D_FEATURE_LEVEL_11_0];
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            None,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&levels),
            D3D11_SDK_VERSION,
            Some(&mut d3d),
            None,
            None,
        )
        .map_err(|e| anyhow!("D3D11CreateDevice: {e}"))?;
        let d3d = d3d.ok_or_else(|| anyhow!("D3D11CreateDevice returned no device"))?;
        let dxgi: IDXGIDevice = d3d.cast().map_err(|e| anyhow!("cast IDXGIDevice: {e}"))?;
        let inspectable =
            CreateDirect3D11DeviceFromDXGIDevice(&dxgi).map_err(|e| anyhow!("CreateDirect3D11DeviceFromDXGIDevice: {e}"))?;
        let winrt: IDirect3DDevice = inspectable
            .cast()
            .map_err(|e| anyhow!("cast IDirect3DDevice: {e}"))?;
        Ok(Device { d3d, winrt })
    }
}

/// Capture one frame of `hwnd` via WGC. Returns Err with a human-readable reason;
/// callers are expected to fall back.
pub fn capture_window(hwnd: HWND, timeout_ms: u64) -> Result<WgcFrame> {
    if hwnd.0.is_null() {
        bail!("null hwnd");
    }

    unsafe {
        let is_min = windows::Win32::UI::WindowsAndMessaging::IsIconic(hwnd).as_bool();
        if is_min {
            bail!("window is minimized");
        }
    }

    let dev = create_device()?;

    let interop: IGraphicsCaptureItemInterop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
        .map_err(|e| anyhow!("GraphicsCaptureItem factory: {e}"))?;
    let item: GraphicsCaptureItem = unsafe {
        interop
            .CreateForWindow(hwnd)
            .map_err(|e| anyhow!("CreateForWindow: {e}"))?
    };

    let size = item.Size().map_err(|e| anyhow!("item.Size: {e}"))?;
    let (w, h) = (size.Width, size.Height);
    if w <= 0 || h <= 0 {
        bail!("window has empty capture size ({w}x{h})");
    }

    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &dev.winrt,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        2,
        size,
    )
    .map_err(|e| anyhow!("CreateFreeThreaded: {e}"))?;

    let session = pool
        .CreateCaptureSession(&item)
        .map_err(|e| anyhow!("CreateCaptureSession: {e}"))?;
    // No cursor: the cursor is composited separately by the caller.
    let _ = session.SetIsCursorCaptureEnabled(false);
    session.StartCapture().map_err(|e| anyhow!("StartCapture: {e}"))?;

    let frame = loop_deadline(&pool, timeout_ms);
    let result = match frame {
        Ok(f) => read_frame(&dev.d3d, &f, w as u32, h as u32),
        Err(e) => Err(e),
    };

    let _ = session.Close();
    let _ = pool.Close();
    result
}

fn loop_deadline(pool: &Direct3D11CaptureFramePool, timeout_ms: u64) -> Result<windows::Graphics::Capture::Direct3D11CaptureFrame> {
    let start = std::time::Instant::now();
    loop {
        match pool.TryGetNextFrame() {
            Ok(frame) => return Ok(frame),
            Err(_) => {
                if start.elapsed().as_millis() as u64 >= timeout_ms {
                    bail!("no frame within {timeout_ms} ms");
                }
                std::thread::sleep(std::time::Duration::from_millis(8));
            }
        }
    }
}

fn read_frame(
    d3d: &ID3D11Device,
    frame: &windows::Graphics::Capture::Direct3D11CaptureFrame,
    w: u32,
    h: u32,
) -> Result<WgcFrame> {
    unsafe {
        let surface = frame.Surface().map_err(|e| anyhow!("frame.Surface: {e}"))?;
        // A WinRT IDirect3DSurface does not QI directly to IDXGISurface; the
        // supported route is IDirect3DDxgiInterfaceAccess.
        let access: IDirect3DDxgiInterfaceAccess = surface
            .cast()
            .map_err(|e| anyhow!("cast IDirect3DDxgiInterfaceAccess: {e}"))?;
        let texture: ID3D11Texture2D = access
            .GetInterface()
            .map_err(|e| anyhow!("DxgiInterfaceAccess.GetInterface: {e}"))?;

        let mut src_desc = D3D11_TEXTURE2D_DESC::default();
        texture.GetDesc(&mut src_desc);

        let staging_desc = D3D11_TEXTURE2D_DESC {
            Width: src_desc.Width,
            Height: src_desc.Height,
            MipLevels: 1,
            ArraySize: 1,
            Format: src_desc.Format,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };

        let mut staging: Option<ID3D11Texture2D> = None;
        d3d.CreateTexture2D(&staging_desc, None, Some(&mut staging))
            .map_err(|e| anyhow!("CreateTexture2D(staging): {e}"))?;
        let staging = staging.ok_or_else(|| anyhow!("staging texture not created"))?;

        let ctx = d3d.GetImmediateContext().map_err(|e| anyhow!("GetImmediateContext: {e}"))?;
        ctx.CopyResource(&staging, &texture);
        ctx.Flush();

        let mut mapped = Default::default();
        ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .map_err(|e| anyhow!("Map: {e}"))?;

        let row_bytes = (src_desc.Width * 4) as usize;
        let mut rgba = vec![0u8; row_bytes * src_desc.Height as usize];
        for y in 0..src_desc.Height as usize {
            let src = (mapped.pData as *const u8).add(y * mapped.RowPitch as usize);
            let dst = rgba.as_mut_ptr().add(y * row_bytes);
            std::ptr::copy_nonoverlapping(src, dst, row_bytes);
        }
        ctx.Unmap(&staging, 0);

        // BGRA -> RGBA. WGC leaves alpha undefined for opaque windows (often 0),
        // which makes the PNG composite onto black and washes colours out.
        for px in rgba.chunks_exact_mut(4) {
            px.swap(0, 2);
            px[3] = 255;
        }

        Ok(WgcFrame {
            width: src_desc.Width.min(w),
            height: src_desc.Height.min(h),
            rgba,
        })
    }
}
