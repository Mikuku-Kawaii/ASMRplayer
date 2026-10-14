//! Windows deletion by an exclusive handle: never follow reparse points, and keep
//! every parent directory pinned until the handle is closed. No recursive deletion.
use std::{
    fs::{File, OpenOptions},
    io,
    path::Path,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity(pub Vec<u64>);

#[cfg(windows)]
mod platform {
    use super::*;
    use std::os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    };
    #[repr(C)]
    #[derive(Default)]
    struct Info {
        attributes: u32,
        created: [u32; 2],
        accessed: [u32; 2],
        modified: [u32; 2],
        volume: u32,
        size_high: u32,
        size_low: u32,
        links: u32,
        index_high: u32,
        index_low: u32,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetFileInformationByHandle(handle: *mut std::ffi::c_void, info: *mut Info) -> i32;
        fn SetFileInformationByHandle(
            handle: *mut std::ffi::c_void,
            class: i32,
            info: *const std::ffi::c_void,
            size: u32,
        ) -> i32;
    }
    pub struct LockedFile {
        file: File,
        _parents: Vec<File>,
    }
    impl LockedFile {
        pub fn open(path: &Path, exclusive: bool) -> io::Result<Self> {
            if !path.is_absolute()
                || path.components().any(|c| match c {
                    std::path::Component::ParentDir | std::path::Component::CurDir => true,
                    std::path::Component::Normal(name) => name.to_string_lossy().contains(':'),
                    _ => false,
                })
            {
                return Err(io::Error::other("拒绝非规范路径或备用数据流"));
            }
            let mut parents = Vec::new();
            let ancestors: Vec<_> = path.ancestors().skip(1).collect();
            for parent in ancestors.into_iter().rev() {
                let f = OpenOptions::new()
                    .access_mode(0x80)
                    .share_mode(1)
                    .custom_flags(0x02000000 | 0x00200000)
                    .open(parent)?;
                let meta = f.metadata()?;
                if !meta.is_dir() || meta.file_attributes() & 0x400 != 0 {
                    return Err(io::Error::other("跳过目录链接或重解析点"));
                }
                parents.push(f);
            }
            let file = OpenOptions::new()
                .access_mode(if exclusive { 0x10080 } else { 0x80 })
                .share_mode(if exclusive { 0 } else { 7 })
                .custom_flags(0x00200000)
                .open(path)?;
            let meta = file.metadata()?;
            if !meta.is_file() || meta.file_attributes() & 0x400 != 0 {
                return Err(io::Error::other("跳过非普通文件或重解析点"));
            }
            Ok(Self {
                file,
                _parents: parents,
            })
        }
        pub fn identity(&self) -> io::Result<Identity> {
            let mut info = Info::default();
            if unsafe { GetFileInformationByHandle(self.file.as_raw_handle(), &mut info) } == 0 {
                return Err(io::Error::last_os_error());
            }
            if info.links != 1 {
                return Err(io::Error::other("保留具有多个硬链接的文件"));
            }
            Ok(Identity(vec![
                info.volume as u64,
                info.index_high as u64,
                info.index_low as u64,
                info.created[0] as u64,
                info.created[1] as u64,
                info.modified[0] as u64,
                info.modified[1] as u64,
                ((info.size_high as u64) << 32) | info.size_low as u64,
            ]))
        }
        pub fn delete(&self) -> io::Result<()> {
            self.set_disposition(true)
        }
        pub fn retain(&self) -> io::Result<()> {
            self.set_disposition(false)
        }
        fn set_disposition(&self, delete: bool) -> io::Result<()> {
            let delete: u8 = u8::from(delete); // FILE_DISPOSITION_INFO.DeleteFile is a BOOLEAN.
            if unsafe {
                SetFileInformationByHandle(
                    self.file.as_raw_handle(),
                    4,
                    &delete as *const _ as *const _,
                    1,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }
}
#[cfg(windows)]
pub use platform::LockedFile;

// This product ships on Windows. Fail closed elsewhere, rather than weakening
// deletion guarantees with a path-based fallback.
#[cfg(not(windows))]
pub struct LockedFile;
#[cfg(not(windows))]
impl LockedFile {
    pub fn open(_: &Path, _: bool) -> io::Result<Self> {
        Err(io::Error::other("存储清理仅支持 Windows"))
    }
    pub fn identity(&self) -> io::Result<Identity> {
        Err(io::Error::other("不支持"))
    }
    pub fn delete(&self) -> io::Result<()> {
        Err(io::Error::other("不支持"))
    }
    pub fn retain(&self) -> io::Result<()> {
        Err(io::Error::other("不支持"))
    }
}
