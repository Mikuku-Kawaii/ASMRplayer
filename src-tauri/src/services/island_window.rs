//! The island has no native non-client frame, even while its region changes.
//! Tao retains WS_CAPTION for undecorated top-level windows and suppresses it
//! through WM_NCCALCSIZE. Region changes can still expose the classic frame.
use windows::{
    core::Result,
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        UI::{
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::*,
        },
    },
};

const SUBCLASS_ID: usize = 0x41534D49;

fn client_only_style(style: u32) -> u32 {
    (style & !(WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX).0)
        | WS_POPUP.0
}

fn client_only_ex_style(style: u32) -> u32 {
    style & !(WS_EX_WINDOWEDGE | WS_EX_CLIENTEDGE | WS_EX_DLGMODALFRAME | WS_EX_STATICEDGE).0
}

// Installed and removed on the owning UI thread. Windows supplies STYLESTRUCT
// only for WM_STYLECHANGING; no borrowed Rust state is retained by the callback.
unsafe extern "system" fn island_proc(
    hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM, id: usize, _data: usize,
) -> LRESULT {
    match message {
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
        WM_NCDESTROY => { let _ = RemoveWindowSubclass(hwnd, Some(island_proc), id); }
        _ => {}
    }
    DefSubclassProc(hwnd, message, wparam, lparam)
}

/// Call before showing the window or attaching the island's composition tree.
pub fn prepare(hwnd: HWND) -> Result<()> {
    unsafe {
        SetWindowSubclass(hwnd, Some(island_proc), SUBCLASS_ID, 0).ok()?;
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

    #[test]
    fn frame_stays_absent_when_runtime_reapplies_styles_and_regions() {
        unsafe {
            // Hidden synthetic HWND; no user window or profile is touched.
            let hwnd = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_WINDOWEDGE,
                w!("STATIC"), w!("Island frame regression"), WS_OVERLAPPEDWINDOW,
                0, 0, 280, 196, None, None, None, None).unwrap();
            let _window = TestWindow(hwnd);
            prepare(hwnd).unwrap();
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
