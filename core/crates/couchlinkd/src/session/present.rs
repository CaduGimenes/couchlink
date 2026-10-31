//! Shows a virtual screen in a player's part of a real one: a borderless window on top, filled every frame with what
//! the virtual monitor shows (DXGI desktop duplication into a flip swap chain, one copy on the GPU, no scaling since
//! the virtual monitor has the part's exact size). One thread per player; dropping the [`Presenter`] closes it.
//!
//! The window never takes the focus or the keyboard: games get their input from the hook, and clicks on it reach no
//! game. Host control finds the game under the pointer through `rawkbm::set_redirects`.

use couchlink_core::session::Rect;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

/// 0: shown on top, 1: shown, not on top, 2: hidden (the window shortcuts).
const SHOW_ON_TOP: u8 = 0;
const SHOW: u8 = 1;
const HIDE: u8 = 2;

pub struct Presenter {
    stop: Arc<AtomicBool>,
    show: Arc<AtomicU8>,
    thread: Option<JoinHandle<()>>,
}

impl Presenter {
    /// Shows the monitor `source` (`\\.\DISPLAY9`) at `dest` on the desktop.
    pub fn start(source: String, dest: Rect) -> Presenter {
        let stop = Arc::new(AtomicBool::new(false));
        let show = Arc::new(AtomicU8::new(SHOW_ON_TOP));
        let thread = {
            let (stop, show) = (stop.clone(), show.clone());
            std::thread::Builder::new()
                .name("present".into())
                .spawn(move || {
                    #[cfg(windows)]
                    win::run(&source, dest, &stop, &show);
                    #[cfg(not(windows))]
                    let _ = (source, dest, stop, show);
                })
                .ok()
        };
        Presenter { stop, show, thread }
    }

    /// The window shortcuts: front (on top or not) or hidden.
    pub fn set_shown(&self, shown: bool, on_top: bool) {
        self.show.store(if !shown { HIDE } else if on_top { SHOW_ON_TOP } else { SHOW }, Ordering::Relaxed);
    }
}

impl Drop for Presenter {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(windows)]
mod win {
    use super::*;
    use std::time::Duration;
    use windows::core::{w, Interface};
    use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_BOX, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
    };
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1, IDXGIFactory2, IDXGIOutput, IDXGIOutput1, IDXGIOutput5, IDXGIOutputDuplication,
        IDXGIResource, IDXGISwapChain1, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO, DXGI_PRESENT, DXGI_SCALING_STRETCH,
        DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT,
    };
    use windows::Win32::Graphics::Gdi::{GetStockObject, BLACK_BRUSH, HBRUSH};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW, RegisterClassW, SetWindowPos, ShowWindow, HWND_NOTOPMOST,
        HWND_TOPMOST, MA_NOACTIVATE, MSG, PM_REMOVE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, WM_MOUSEACTIVATE, WNDCLASSW,
        WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
    };

    unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        if msg == WM_MOUSEACTIVATE {
            return LRESULT(MA_NOACTIVATE as isize);
        }
        // SAFETY: forwards the message unchanged.
        unsafe { DefWindowProcW(hwnd, msg, w, l) }
    }

    fn window(dest: Rect) -> Option<HWND> {
        // SAFETY: plain Win32 calls; the class name is a static string.
        unsafe {
            let inst = GetModuleHandleW(None).ok()?;
            let class = w!("CouchlinkVirtualScreen");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wnd_proc),
                hInstance: inst.into(),
                lpszClassName: class,
                hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
                ..Default::default()
            };
            RegisterClassW(&wc); // already registered by an earlier presenter is fine
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
                class,
                w!("Couchlink virtual screen"),
                WS_POPUP,
                dest.x,
                dest.y,
                dest.w,
                dest.h,
                None,
                None,
                Some(inst.into()),
                None,
            )
            .ok()?;
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            Some(hwnd)
        }
    }

    /// Capture from the monitor and the swap chain that shows it in the window.
    struct Pipe {
        ctx: ID3D11DeviceContext,
        dupl: IDXGIOutputDuplication,
        swap: IDXGISwapChain1,
        back: ID3D11Texture2D,
    }

    fn find_output(source: &str) -> Option<(IDXGIAdapter1, IDXGIOutput)> {
        // SAFETY: DXGI enumeration; every object is reference counted by the wrapper types.
        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
            for a in 0.. {
                let Ok(adapter) = factory.EnumAdapters1(a) else { break };
                for o in 0.. {
                    let Ok(output) = adapter.EnumOutputs(o) else { break };
                    let Ok(desc) = output.GetDesc() else { continue };
                    let n = desc.DeviceName.iter().position(|&c| c == 0).unwrap_or(desc.DeviceName.len());
                    if String::from_utf16_lossy(&desc.DeviceName[..n]).eq_ignore_ascii_case(source) {
                        return Some((adapter, output));
                    }
                }
            }
            None
        }
    }

    fn open(source: &str, hwnd: HWND, dest: Rect) -> Result<Pipe, String> {
        let (adapter, output) = find_output(source).ok_or_else(|| format!("{source} is not there"))?;
        // SAFETY: device, duplication and swap chain creation with valid arguments; out parameters are locals.
        unsafe {
            let mut device: Option<ID3D11Device> = None;
            let mut ctx: Option<ID3D11DeviceContext> = None;
            D3D11CreateDevice(&adapter, D3D_DRIVER_TYPE_UNKNOWN, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut device), None, Some(&mut ctx))
                .map_err(|e| format!("no Direct3D device: {e}"))?;
            let (device, ctx) = (device.ok_or("no device")?, ctx.ok_or("no context")?);
            // Ask for 8-bit BGRA even on an HDR monitor, so the copy into the swap chain always matches.
            // DuplicateOutput1 only works in a per-monitor DPI aware process; the older call works everywhere.
            let dupl = match output.cast::<IDXGIOutput5>().and_then(|o5| o5.DuplicateOutput1(&device, 0, &[DXGI_FORMAT_B8G8R8A8_UNORM])) {
                Ok(d) => d,
                Err(_) => output.cast::<IDXGIOutput1>().and_then(|o1| o1.DuplicateOutput(&device)).map_err(|e| format!("cannot capture {source}: {e}"))?,
            };
            let factory: IDXGIFactory2 = adapter.GetParent().map_err(|e| e.to_string())?;
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: dest.w as u32,
                Height: dest.h as u32,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                ..Default::default()
            };
            let swap = factory.CreateSwapChainForHwnd(&device, hwnd, &desc, None, None).map_err(|e| format!("cannot draw the window: {e}"))?;
            let back: ID3D11Texture2D = swap.GetBuffer(0).map_err(|e| e.to_string())?;
            Ok(Pipe { ctx, dupl, swap, back })
        }
    }

    /// Copies the next frame in (when there is one) and shows it. Err when the capture must be opened again.
    fn frame(p: &Pipe, dest: Rect) -> Result<(), ()> {
        // SAFETY: the duplication hands out a frame that is released before the next one is asked for.
        unsafe {
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut res: Option<IDXGIResource> = None;
            match p.dupl.AcquireNextFrame(16, &mut info, &mut res) {
                Ok(()) => {}
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(()),
                Err(_) => return Err(()),
            }
            if let Some(tex) = res.and_then(|r| r.cast::<ID3D11Texture2D>().ok()) {
                let b = D3D11_BOX { left: 0, top: 0, front: 0, right: dest.w as u32, bottom: dest.h as u32, back: 1 };
                p.ctx.CopySubresourceRegion(&p.back, 0, 0, 0, 0, &tex, 0, Some(&b));
            }
            let _ = p.dupl.ReleaseFrame();
            let _ = p.swap.Present(0, DXGI_PRESENT(0));
        }
        Ok(())
    }

    pub fn run(source: &str, dest: Rect, stop: &AtomicBool, show: &AtomicU8) {
        let Some(hwnd) = window(dest) else {
            eprintln!("couchlinkd: cannot open the window for virtual screen {source}");
            return;
        };
        let mut pipe: Option<Pipe> = None;
        let mut shown = SHOW_ON_TOP;
        let mut last_error = String::new();
        while !stop.load(Ordering::Relaxed) {
            // SAFETY: this thread's own message queue.
            unsafe {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    DispatchMessageW(&msg);
                }
            }
            let want = show.load(Ordering::Relaxed);
            if want != shown {
                // SAFETY: a window this thread owns.
                unsafe {
                    if want == HIDE {
                        let _ = ShowWindow(hwnd, SW_HIDE);
                    } else {
                        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                        let z = if want == SHOW_ON_TOP { HWND_TOPMOST } else { HWND_NOTOPMOST };
                        let _ = SetWindowPos(hwnd, Some(z), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                    }
                }
                shown = want;
            }
            if pipe.is_none() {
                match open(source, hwnd, dest) {
                    Ok(p) => pipe = Some(p),
                    Err(e) => {
                        if e != last_error {
                            eprintln!("couchlinkd: virtual screen {source}: {e}");
                            last_error = e;
                        }
                        std::thread::sleep(Duration::from_millis(250));
                        continue;
                    }
                }
            }
            if let Some(p) = &pipe {
                if frame(p, dest).is_err() {
                    // The monitor changed mode, a full screen game took it, or the desktop switched: open again.
                    pipe = None;
                }
            }
        }
        drop(pipe);
        // SAFETY: the window belongs to this thread.
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}
