//! T1417: serialize early startup until the single-instance plugin's message
//! window exists. Without this gate, simultaneous launches can see its mutex
//! before its HWND is created and both proceed with application setup.
#[cfg(windows)]
pub struct StartupGate(isize);

#[cfg(windows)]
impl StartupGate {
    pub fn acquire(identifier: &str) -> Result<Self, String> {
        use windows::{core::PCWSTR, Win32::{Foundation::{CloseHandle, WAIT_ABANDONED, WAIT_OBJECT_0}, System::Threading::{CreateMutexW, WaitForSingleObject}}};
        let name: Vec<u16> = format!("Local\\{identifier}-startup\0").encode_utf16().collect();
        unsafe {
            let handle = CreateMutexW(None, false, PCWSTR(name.as_ptr())).map_err(|e| e.to_string())?;
            // A secondary process exits inside the plugin, bypassing Rust Drop.
            // Win32 abandons its mutex so a third waiting launch can take over.
            let result = WaitForSingleObject(handle, 30_000);
            if result != WAIT_OBJECT_0 && result != WAIT_ABANDONED {
                let _ = CloseHandle(handle);
                return Err("应用仍在启动，请稍后重试".into());
            }
            Ok(Self(handle.0 as isize))
        }
    }
}

#[cfg(windows)]
impl Drop for StartupGate {
    fn drop(&mut self) {
        use windows::Win32::{Foundation::{CloseHandle, HANDLE}, System::Threading::ReleaseMutex};
        unsafe {
            let handle = HANDLE(self.0 as *mut _);
            let _ = ReleaseMutex(handle);
            let _ = CloseHandle(handle);
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn concurrent_startup_waits_until_first_instance_is_ready() {
        let id = uuid::Uuid::new_v4().to_string();
        let first = StartupGate::acquire(&id).unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _second = StartupGate::acquire(&id).unwrap();
            send.send(()).unwrap();
        });
        assert!(receive.recv_timeout(std::time::Duration::from_millis(100)).is_err());
        drop(first);
        receive.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        worker.join().unwrap();
    }
}
