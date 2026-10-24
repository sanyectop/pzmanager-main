//! 临时验证宿主：一个持续换帧的 OpenGL 窗口，用来验证覆盖层 DLL 的加载界面与动画。

use std::time::{Duration, Instant};

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::GetDC;
use windows::Win32::Graphics::OpenGL::{
    ChoosePixelFormat, SetPixelFormat, SwapBuffers, PFD_DOUBLEBUFFER, PFD_DRAW_TO_WINDOW,
    PFD_SUPPORT_OPENGL, PFD_TYPE_RGBA, PIXELFORMATDESCRIPTOR,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW, RegisterClassW,
    SetWindowPos, ShowWindow, TranslateMessage, HWND_TOPMOST, MSG, PM_REMOVE, SWP_SHOWWINDOW,
    SW_SHOW, WNDCLASSW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

#[link(name = "opengl32")]
extern "system" {
    fn wglCreateContext(hdc: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    fn wglMakeCurrent(hdc: *mut std::ffi::c_void, hglrc: *mut std::ffi::c_void) -> i32;
    fn wglDeleteContext(hglrc: *mut std::ffi::c_void) -> i32;
    fn glClearColor(r: f32, g: f32, b: f32, a: f32);
    fn glClear(mask: u32);
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn main() {
    let seconds: u64 = std::env::args().nth(1).and_then(|v| v.parse().ok()).unwrap_or(30);
    unsafe {
        let instance = GetModuleHandleW(windows::core::PCWSTR::null()).unwrap();
        let class_name = w!("PzOverlayVerifyHost");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };
        assert_ne!(RegisterClassW(&wc), 0, "RegisterClassW 失败");
        let hwnd = CreateWindowExW(
            Default::default(),
            class_name,
            w!("PZ overlay verify host"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            120,
            120,
            900,
            640,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap();
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 120, 120, 900, 640, SWP_SHOWWINDOW);

        let hdc = GetDC(Some(hwnd));
        let pfd = PIXELFORMATDESCRIPTOR {
            nSize: size_of::<PIXELFORMATDESCRIPTOR>() as u16,
            nVersion: 1,
            dwFlags: PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER,
            iPixelType: PFD_TYPE_RGBA,
            cColorBits: 32,
            cDepthBits: 24,
            ..Default::default()
        };
        let format = ChoosePixelFormat(hdc, &pfd);
        assert_ne!(format, 0, "ChoosePixelFormat 失败");
        SetPixelFormat(hdc, format, &pfd).expect("SetPixelFormat 失败");
        let hglrc = wglCreateContext(hdc.0);
        assert!(!hglrc.is_null(), "wglCreateContext 失败");
        assert_ne!(wglMakeCurrent(hdc.0, hglrc), 0, "wglMakeCurrent 失败");

        use std::io::Write as _;
        println!("PID={} READY", std::process::id());
        let _ = std::io::stdout().flush();

        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(seconds) {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            glClearColor(0.05, 0.15, 0.4, 1.0);
            glClear(0x0000_4000);
            SwapBuffers(hdc).expect("SwapBuffers 失败");
            std::thread::sleep(Duration::from_millis(16));
        }

        let _ = wglMakeCurrent(std::ptr::null_mut(), std::ptr::null_mut());
        let _ = wglDeleteContext(hglrc);
        let _ = DestroyWindow(hwnd);
        println!("DONE");
    }
}
