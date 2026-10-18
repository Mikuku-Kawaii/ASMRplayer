//! The island has no native non-client frame, even while its region changes.
//! Tao retains WS_CAPTION for undecorated top-level windows and suppresses it
//! through WM_NCCALCSIZE. Region changes can still expose the classic frame.
use windows::{
    core::Result,
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::SystemServices::{
            APPCOMMAND_ID, APPCOMMAND_MEDIA_FAST_FORWARD, APPCOMMAND_MEDIA_NEXTTRACK,
            APPCOMMAND_MEDIA_PAUSE, APPCOMMAND_MEDIA_PLAY, APPCOMMAND_MEDIA_PLAY_PAUSE,
            APPCOMMAND_MEDIA_PREVIOUSTRACK, APPCOMMAND_MEDIA_REWIND, APPCOMMAND_MEDIA_STOP,
        },
        UI::{
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::*,
        },
    },
};

const SUBCLASS_ID: usize = 0x41534D49;
type MediaHandler = Box<dyn Fn(&'static str) + Send + Sync>;

fn media_action(lparam: LPARAM) -> Option<&'static str> {
    // GET_APPCOMMAND_LPARAM masks off the keyboard/mouse source flags.
    match APPCOMMAND_ID((lparam.0 as u32 >> 16) & !FAPPCOMMAND_MASK) {
        APPCOMMAND_MEDIA_PLAY_PAUSE => Some("toggle_pause"),
        APPCOMMAND_MEDIA_PLAY => Some("play"),
        APPCOMMAND_MEDIA_PAUSE => Some("pause"),
        APPCOMMAND_MEDIA_STOP => Some("stop"),
        APPCOMMAND_MEDIA_NEXTTRACK => Some("next"),
        APPCOMMAND_MEDIA_PREVIOUSTRACK => Some("previous"),
        APPCOMMAND_MEDIA_FAST_FORWARD => Some("fast_forward"),
        APPCOMMAND_MEDIA_REWIND => Some("rewind"),
        _ => None,
    }
}

fn client_only_style(style: u32) -> u32 {
    (style & !(WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX).0)
        | WS_POPUP.0
}

fn client_only_ex_style(style: u32) -> u32 {
    style & !(WS_EX_WINDOWEDGE | WS_EX_CLIENTEDGE | WS_EX_DLGMODALFRAME | WS_EX_STATICEDGE).0
}

// Installed and removed on the owning UI thread. Windows supplies STYLESTRUCT
// only for WM_STYLECHANGING. The boxed media callback is owned until NCDESTROY.
unsafe extern "system" fn island_proc(
    hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM, id: usize, data: usize,
) -> LRESULT {
    match message {
        WM_APPCOMMAND => {
            if let Some(action) = media_action(lparam) {
                if data != 0 { (&*(data as *const MediaHandler))(action); }
                // DefWindowProc forwards unhandled top-level media commands to
                // the shell, which can deliver them to another music player.
                return LRESULT(1);
            }
        }
        WM_STYLECHANGING if lparam.0 != 0 => {
            let styles = &mut *(lparam.0 as *mut STYLESTRUCT);
            if wparam.0 as i32 == GWL_STYLE.0 {
                styles.styleNew = client_only_style(styles.styleNew);
            } else if wparam.0 as i32 == GWL_EXSTYLE.0 {
                styles.styleNew = client_only_ex_style(styles.styleNew);
            }
        }
        // Suppress both NCCALCSIZE variants, including the simple RECT variant
        // used when SetWindowRgn removes/restores the region during a morph.
        WM_NCCALCSIZE | WM_NCPAINT => return LRESULT(0),
        WM_NCACTIVATE | WM_ERASEBKGND => return LRESULT(1),
        WM_NCDESTROY => {
            let _ = RemoveWindowSubclass(hwnd, Some(island_proc), id);
            if data != 0 { drop(Box::from_raw(data as *mut MediaHandler)); }
        }
        _ => {}
    }
    DefSubclassProc(hwnd, message, wparam, lparam)
}

/// Call before showing the window or attaching the island's composition tree.
pub fn prepare(hwnd: HWND, on_media: impl Fn(&'static str) + Send + Sync + 'static) -> Result<()> {
    unsafe {
        let handler: Box<MediaHandler> = Box::new(Box::new(on_media));
        let data = Box::into_raw(handler);
        if let Err(error) = SetWindowSubclass(hwnd, Some(island_proc), SUBCLASS_ID, data as usize).ok() {
            drop(Box::from_raw(data));
            return Err(error);
        }
        SetWindowLongPtrW(hwnd, GWL_STYLE, client_only_style(GetWindowLongPtrW(hwnd, GWL_STYLE) as u32) as isize);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, client_only_ex_style(GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32) as isize);
        SetWindowPos(hwnd, None, 0, 0, 0, 0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::{core::w, Win32::Foundation::RECT};
    use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn};

    struct TestWindow(HWND);
    impl Drop for TestWindow {
        fn drop(&mut self) { unsafe { let _ = DestroyWindow(self.0); } }
    }

    unsafe extern "system" fn fallback_proc(
        hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, data: usize,
    ) -> LRESULT {
        if message == WM_APPCOMMAND {
            (*(data as *const std::sync::atomic::AtomicUsize)).fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return LRESULT(0);
        }
        DefSubclassProc(hwnd, message, wparam, lparam)
    }

    #[test]
    fn media_commands_stay_in_the_island_and_never_reach_shell_fallback() {
        use std::sync::{Arc, Mutex, atomic::{AtomicUsize, Ordering}};
        unsafe {
            let fallback = AtomicUsize::new(0);
            let hwnd = CreateWindowExW(WS_EX_TOOLWINDOW, w!("STATIC"), w!("Island media regression"),
                WS_POPUP, 0, 0, 280, 196, None, None, None, None).unwrap();
            let window = TestWindow(hwnd);
            SetWindowSubclass(hwnd, Some(fallback_proc), 1, &fallback as *const _ as usize).ok().unwrap();
            let actions = Arc::new(Mutex::new(Vec::new()));
            let received = actions.clone();
            prepare(hwnd, move |action| received.lock().unwrap().push(action)).unwrap();
            for flags in [FAPPCOMMAND_KEY, FAPPCOMMAND_MOUSE, FAPPCOMMAND_OEM] {
                for (command, action) in [
                    (APPCOMMAND_MEDIA_PLAY_PAUSE, "toggle_pause"),
                    (APPCOMMAND_MEDIA_PLAY, "play"), (APPCOMMAND_MEDIA_PAUSE, "pause"),
                    (APPCOMMAND_MEDIA_STOP, "stop"), (APPCOMMAND_MEDIA_NEXTTRACK, "next"),
                    (APPCOMMAND_MEDIA_PREVIOUSTRACK, "previous"),
                    (APPCOMMAND_MEDIA_FAST_FORWARD, "fast_forward"), (APPCOMMAND_MEDIA_REWIND, "rewind"),
                ] {
                    let before = actions.lock().unwrap().len();
                    assert_eq!(SendMessageW(hwnd, WM_APPCOMMAND, Some(WPARAM(hwnd.0 as usize)),
                        Some(LPARAM(((command.0 | flags) << 16) as isize))).0, 1);
                    let received = actions.lock().unwrap();
                    assert_eq!(received.len(), before + 1);
                    assert_eq!(received.last(), Some(&action));
                }
            }
            assert_eq!(fallback.load(Ordering::SeqCst), 0);
            // Unrelated app commands still reach the normal window chain.
            SendMessageW(hwnd, WM_APPCOMMAND, None, Some(LPARAM(10 << 16)));
            assert_eq!(fallback.load(Ordering::SeqCst), 1);
            drop(window);
            assert_eq!(Arc::strong_count(&actions), 1, "destroy releases the callback");
        }
    }

    #[test]
    fn frame_stays_absent_when_runtime_reapplies_styles_and_regions() {
        unsafe {
            // Hidden synthetic HWND; no user window or profile is touched.
            let hwnd = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_WINDOWEDGE,
                w!("STATIC"), w!("Island frame regression"), WS_OVERLAPPEDWINDOW,
                0, 0, 280, 196, None, None, None, None).unwrap();
            let _window = TestWindow(hwnd);
            prepare(hwnd, |_| {}).unwrap();
            // TOPMOST is managed by SetWindowPos, not SetWindowLongPtrW.
            // Verify our filter preserves it independently of OS Z-order policy.
            assert_eq!(client_only_ex_style((WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_WINDOWEDGE).0),
                (WS_EX_TOOLWINDOW | WS_EX_TOPMOST).0);
            for _ in 0..3 {
                SetWindowLongPtrW(hwnd, GWL_STYLE, (WS_OVERLAPPEDWINDOW | WS_CLIPSIBLINGS).0 as isize);
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, (WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_WINDOWEDGE).0 as isize);
                let region = CreateRoundRectRgn(0, 0, 201, 43, 42, 42);
                assert_ne!(SetWindowRgn(hwnd, Some(region), false), 0);
                assert_ne!(SetWindowRgn(hwnd, None, false), 0);
                assert_eq!(GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & WS_OVERLAPPEDWINDOW.0, 0);
                let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
                assert_eq!(ex & WS_EX_WINDOWEDGE.0, 0);
                assert_eq!(ex & WS_EX_TOOLWINDOW.0, WS_EX_TOOLWINDOW.0);
                assert_ne!(GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & WS_CLIPSIBLINGS.0, 0);
                let mut rect = RECT { left: 0, top: 0, right: 280, bottom: 196 };
                SendMessageW(hwnd, WM_NCCALCSIZE, Some(WPARAM(0)), Some(LPARAM((&mut rect as *mut RECT) as isize)));
                assert_eq!((rect.left, rect.top, rect.right, rect.bottom), (0, 0, 280, 196));
                assert_eq!(SendMessageW(hwnd, WM_ERASEBKGND, None, None).0, 1);
            }
        }
    }
}
