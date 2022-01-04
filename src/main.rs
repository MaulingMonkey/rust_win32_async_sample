#![windows_subsystem = "windows"]   // No console window
#![allow(non_snake_case)]           // WinAPI style vars
#![deny(unreachable_patterns)]      // probably a bad WM_* match

use futures::executor::{LocalPool, LocalSpawner};
use futures::task::LocalSpawnExt;

use winapi::shared::minwindef::*;
use winapi::shared::windef::*;
use winapi::um::libloaderapi::*;
use winapi::um::wingdi::*;
use winapi::um::winuser::*;

use wchar::wchz;

use std::cell::RefCell;
use std::mem::zeroed;
use std::ptr::{null, null_mut};
use std::time::Duration;

thread_local! {
    static UI_POOL : RefCell<LocalPool> = Default::default();
    static UI_SPAWNER : LocalSpawner = UI_POOL.with(|pool| pool.borrow().spawner());
}

fn main() {
    spawn_window();
    main_loop();
}

async fn on_mouse_down() {
    wait_for(Duration::from_secs(2)).await; // doesn't block the UI thread
    unsafe { MessageBoxW(null_mut(), wchz!("Time!").as_ptr(), wchz!("Time!").as_ptr(), MB_OK) }; // runs on (and blocks!) the UI thread
}

fn main_loop() {
    loop {
        let mut msg : MSG = unsafe { zeroed() };
        while unsafe { PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) } != 0 {
            unsafe { TranslateMessage(&msg) };
            unsafe { DispatchMessageW(&msg) };
            match msg.message {
                WM_QUIT => return,
                _       => {},
            }
        }

        UI_POOL.with(|pool| pool.borrow_mut().run_until_stalled());
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, uMsg: UINT, wParam: WPARAM, lParam: LPARAM) -> LRESULT {
    std::panic::catch_unwind(||{ // unwinding panics across FFI callback boundaries would be undefined behavior
        match uMsg {
            WM_DESTROY => unsafe {
                PostQuitMessage(0);
                0
            },
            WM_LBUTTONDOWN => {
                UI_SPAWNER.with(|s| s.spawn_local(on_mouse_down())).unwrap();
                0
            },
            WM_PAINT => unsafe {
                let mut ps : PAINTSTRUCT = zeroed();
                let hdc = BeginPaint(hwnd, &mut ps);
                let brush = CreateSolidBrush(RGB(0x33, 0x66, 0x99));
                FillRect(hdc, &ps.rcPaint, brush);
                EndPaint(hwnd, &ps);
                DeleteObject(brush as HGDIOBJ);
                0
            },
            _ => unsafe {
                DefWindowProcW(hwnd, uMsg, wParam, lParam)
            },
        }
    }).unwrap_or_else(|panic|{
        eprintln!("window_proc paniced: {:?}", panic);
        std::process::abort();
    })
}

fn spawn_window() {
    let hInstance = unsafe { GetModuleHandleW(null()) };
    assert!(!hInstance.is_null());

    let hCursor = unsafe { LoadCursorW(null_mut(), IDC_ARROW) };
    assert!(!hCursor.is_null());

    let wc = WNDCLASSW { lpfnWndProc: Some(window_proc), hInstance, hCursor, lpszClassName: wchz!("SampleWndClass").as_ptr(), .. unsafe { zeroed() } };
    assert!(unsafe { RegisterClassW(&wc) } != 0);

    let hwnd = unsafe { CreateWindowExW(
        0, wchz!("SampleWndClass").as_ptr(), wchz!("Title").as_ptr(), WS_OVERLAPPEDWINDOW,  // exstyle, class, title, style
        CW_USEDEFAULT, CW_USEDEFAULT, 400, 300,                                             // x, y, w, h
        null_mut(), null_mut(), hInstance, null_mut()                                       // parent, menu, hInstance, lpParam
    )};
    assert!(!hwnd.is_null());

    assert!(unsafe { ShowWindow(hwnd, SW_SHOW) } == 0);
}

async fn wait_for(d: Duration) {
    let (sender, receiver) = futures::channel::oneshot::channel::<()>();
    std::thread::spawn(move ||{
        std::thread::sleep(d);
        let _ = sender.send(());
    });
    receiver.await.unwrap();
}
