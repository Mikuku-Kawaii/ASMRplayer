use crate::domain::{AppError, AppResult, ErrorCode};
use serde_json::{Map, Number, Value};
use std::ffi::{c_char, c_int, c_ulong, c_void, CStr, CString};
use std::path::PathBuf;
use std::sync::OnceLock;

#[repr(C)]
pub struct mpv_handle {
    _private: [u8; 0],
}

pub const MPV_FORMAT_NONE: c_int = 0;
pub const MPV_FORMAT_STRING: c_int = 1;
pub const MPV_FORMAT_OSD_STRING: c_int = 2;
pub const MPV_FORMAT_FLAG: c_int = 3;
pub const MPV_FORMAT_INT64: c_int = 4;
pub const MPV_FORMAT_DOUBLE: c_int = 5;
pub const MPV_FORMAT_NODE: c_int = 6;
pub const MPV_FORMAT_NODE_ARRAY: c_int = 7;
pub const MPV_FORMAT_NODE_MAP: c_int = 8;

pub const MPV_EVENT_NONE: c_int = 0;
pub const MPV_EVENT_SHUTDOWN: c_int = 1;
pub const MPV_EVENT_START_FILE: c_int = 6;
pub const MPV_EVENT_END_FILE: c_int = 7;
pub const MPV_EVENT_FILE_LOADED: c_int = 8;
pub const MPV_EVENT_PROPERTY_CHANGE: c_int = 22;

pub const MPV_END_FILE_REASON_EOF: c_int = 0;
pub const MPV_END_FILE_REASON_STOP: c_int = 2;
pub const MPV_END_FILE_REASON_QUIT: c_int = 3;
pub const MPV_END_FILE_REASON_ERROR: c_int = 4;
pub const MPV_END_FILE_REASON_REDIRECT: c_int = 5;

#[repr(C)]
#[derive(Copy, Clone)]
pub union mpv_node_u {
    pub string: *mut c_char,
    pub flag: c_int,
    pub int64: i64,
    pub double_: f64,
    pub list: *mut mpv_node_list,
    pub ba: *mut c_void,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct mpv_node {
    pub u: mpv_node_u,
    pub format: c_int,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct mpv_node_list {
    pub num: c_int,
    pub values: *mut mpv_node,
    pub keys: *mut *mut c_char,
}

#[repr(C)]
pub struct mpv_event_property {
    pub name: *const c_char,
    pub format: c_int,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct mpv_event_start_file {
    pub playlist_entry_id: i64,
}

#[repr(C)]
pub struct mpv_event_end_file {
    pub reason: c_int,
    pub error: c_int,
    pub playlist_entry_id: i64,
    pub playlist_insert_id: i64,
    pub playlist_insert_num_entries: c_int,
}

#[repr(C)]
pub struct mpv_event {
    pub event_id: c_int,
    pub error: c_int,
    pub reply_userdata: u64,
    pub data: *mut c_void,
}

#[derive(Clone, Copy)]
pub struct LibMpvApi {
    pub mpv_client_api_version: unsafe extern "C" fn() -> c_ulong,
    pub mpv_error_string: unsafe extern "C" fn(c_int) -> *const c_char,
    pub mpv_free: unsafe extern "C" fn(*mut c_void),
    pub mpv_create: unsafe extern "C" fn() -> *mut mpv_handle,
    pub mpv_initialize: unsafe extern "C" fn(*mut mpv_handle) -> c_int,
    pub mpv_terminate_destroy: unsafe extern "C" fn(*mut mpv_handle),
    pub mpv_set_option_string:
        unsafe extern "C" fn(*mut mpv_handle, *const c_char, *const c_char) -> c_int,
    pub mpv_command: unsafe extern "C" fn(*mut mpv_handle, *const *const c_char) -> c_int,
    pub mpv_command_node:
        unsafe extern "C" fn(*mut mpv_handle, *mut mpv_node, *mut mpv_node) -> c_int,
    pub mpv_set_property:
        unsafe extern "C" fn(*mut mpv_handle, *const c_char, c_int, *const c_void) -> c_int,
    pub mpv_set_property_string:
        unsafe extern "C" fn(*mut mpv_handle, *const c_char, *const c_char) -> c_int,
    pub mpv_get_property:
        unsafe extern "C" fn(*mut mpv_handle, *const c_char, c_int, *mut c_void) -> c_int,
    pub mpv_observe_property:
        unsafe extern "C" fn(*mut mpv_handle, u64, *const c_char, c_int) -> c_int,
    pub mpv_wait_event: unsafe extern "C" fn(*mut mpv_handle, f64) -> *mut mpv_event,
    pub mpv_wakeup: unsafe extern "C" fn(*mut mpv_handle),
    pub mpv_free_node_contents: unsafe extern "C" fn(*mut mpv_node),
}

unsafe impl Send for LibMpvApi {}
unsafe impl Sync for LibMpvApi {}

/// Resolves the `mpv-2.dll` (`libmpv-2.dll`) dynamic library path and ensures companion `vulkan-1.dll` is staged.
pub fn resolve_libmpv_library() -> AppResult<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(custom) = std::env::var("ASMRPLAYER_LIBMPV_PATH") {
        candidates.push(PathBuf::from(custom));
    }
    if let Ok(custom) = std::env::var("ASMRPLAYER_MPV_PATH") {
        candidates.push(PathBuf::from(custom));
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("mpv-2.dll"));
            candidates.push(dir.join("libmpv-2.dll"));
            candidates.push(dir.join("binaries").join("mpv-2.dll"));
            candidates.push(dir.join("binaries").join("libmpv-2.dll"));
        }
    }

    let clean_env = std::env::var("ASMRPLAYER_CLEAN_ENV")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if !clean_env {
        candidates.push(manifest_dir.join("binaries").join("mpv-2.dll"));
        candidates.push(manifest_dir.join("binaries").join("libmpv-2.dll"));
    }

    for candidate in candidates {
        if candidate.is_file() {
            if let Some(parent) = candidate.parent() {
                let target_dll = parent.join("vulkan-1.dll");
                if !target_dll.exists() {
                    let packaged_dll = parent.join("binaries").join("vulkan-1.dll");
                    if packaged_dll.is_file() {
                        let _ = std::fs::copy(&packaged_dll, &target_dll);
                    } else if !clean_env {
                        let dev_dll = manifest_dir.join("binaries").join("vulkan-1.dll");
                        if dev_dll.is_file() {
                            let _ = std::fs::copy(&dev_dll, &target_dll);
                        }
                    }
                }
            }
            return Ok(candidate);
        }
    }

    Err(AppError::new(
        ErrorCode::MpvMissing,
        "未找到 libmpv 动态库文件 (mpv-2.dll / libmpv-2.dll)",
    ))
}

static LIBMPV_API: OnceLock<Result<LibMpvApi, String>> = OnceLock::new();

pub fn get_libmpv_api() -> AppResult<&'static LibMpvApi> {
    let res = LIBMPV_API.get_or_init(|| unsafe { load_libmpv_api_once() });
    match res {
        Ok(api) => Ok(api),
        Err(msg) => Err(AppError::new(ErrorCode::MpvMissing, msg.clone())),
    }
}

#[cfg(target_os = "windows")]
unsafe fn load_libmpv_api_once() -> Result<LibMpvApi, String> {
    use windows::core::{HSTRING, PCSTR, PCWSTR};
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::System::LibraryLoader::{
        GetProcAddress, LoadLibraryExW, LOAD_WITH_ALTERED_SEARCH_PATH,
    };

    let dll_path = resolve_libmpv_library().map_err(|e| e.message)?;
    let canon = std::fs::canonicalize(&dll_path).unwrap_or(dll_path);
    let clean_path = canon
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('/', "\\");
    let wide = HSTRING::from(&clean_path);

    let module: HMODULE = LoadLibraryExW(
        PCWSTR(wide.as_ptr()),
        None,
        LOAD_WITH_ALTERED_SEARCH_PATH,
    )
    .map_err(|e| format!("加载 libmpv 动态库失败 ({}): {}", clean_path, e))?;

    unsafe fn sym<T>(module: HMODULE, name: &'static [u8]) -> Result<T, String> {
        let proc = GetProcAddress(module, PCSTR(name.as_ptr())).ok_or_else(|| {
            let s = String::from_utf8_lossy(&name[..name.len().saturating_sub(1)]);
            format!("libmpv 缺少导出符号: {}", s)
        })?;
        Ok(std::mem::transmute_copy(&proc))
    }

    Ok(LibMpvApi {
        mpv_client_api_version: sym(module, b"mpv_client_api_version\0")?,
        mpv_error_string: sym(module, b"mpv_error_string\0")?,
        mpv_free: sym(module, b"mpv_free\0")?,
        mpv_create: sym(module, b"mpv_create\0")?,
        mpv_initialize: sym(module, b"mpv_initialize\0")?,
        mpv_terminate_destroy: sym(module, b"mpv_terminate_destroy\0")?,
        mpv_set_option_string: sym(module, b"mpv_set_option_string\0")?,
        mpv_command: sym(module, b"mpv_command\0")?,
        mpv_command_node: sym(module, b"mpv_command_node\0")?,
        mpv_set_property: sym(module, b"mpv_set_property\0")?,
        mpv_set_property_string: sym(module, b"mpv_set_property_string\0")?,
        mpv_get_property: sym(module, b"mpv_get_property\0")?,
        mpv_observe_property: sym(module, b"mpv_observe_property\0")?,
        mpv_wait_event: sym(module, b"mpv_wait_event\0")?,
        mpv_wakeup: sym(module, b"mpv_wakeup\0")?,
        mpv_free_node_contents: sym(module, b"mpv_free_node_contents\0")?,
    })
}

#[cfg(not(target_os = "windows"))]
unsafe fn load_libmpv_api_once() -> Result<LibMpvApi, String> {
    Err("libmpv dynamic loader currently configured for Windows target".into())
}

impl LibMpvApi {
    pub fn error_str(&self, code: c_int) -> String {
        unsafe {
            let ptr = (self.mpv_error_string)(code);
            if ptr.is_null() {
                format!("mpv error {}", code)
            } else {
                CStr::from_ptr(ptr).to_string_lossy().into_owned()
            }
        }
    }

    pub unsafe fn set_option_str(
        &self,
        ctx: *mut mpv_handle,
        name: &str,
        value: &str,
    ) -> AppResult<()> {
        let c_name = CString::new(name).unwrap_or_default();
        let c_val = CString::new(value).unwrap_or_default();
        let rc = (self.mpv_set_option_string)(ctx, c_name.as_ptr(), c_val.as_ptr());
        if rc < 0 {
            return Err(AppError::new(
                ErrorCode::MpvIpcError,
                format!("mpv_set_option_string({}, {}) 失败: {}", name, value, self.error_str(rc)),
            ));
        }
        Ok(())
    }

    pub unsafe fn set_property_str(
        &self,
        ctx: *mut mpv_handle,
        name: &str,
        value: &str,
    ) -> AppResult<()> {
        let c_name = CString::new(name).unwrap_or_default();
        let c_val = CString::new(value).unwrap_or_default();
        let rc = (self.mpv_set_property_string)(ctx, c_name.as_ptr(), c_val.as_ptr());
        if rc < 0 {
            return Err(AppError::new(
                ErrorCode::MpvIpcError,
                format!(
                    "mpv_set_property_string({}, {}) 失败: {}",
                    name,
                    value,
                    self.error_str(rc)
                ),
            ));
        }
        Ok(())
    }

    pub unsafe fn set_property_flag(
        &self,
        ctx: *mut mpv_handle,
        name: &str,
        flag: bool,
    ) -> AppResult<()> {
        let c_name = CString::new(name).unwrap_or_default();
        let val: c_int = if flag { 1 } else { 0 };
        let rc = (self.mpv_set_property)(
            ctx,
            c_name.as_ptr(),
            MPV_FORMAT_FLAG,
            (&val as *const c_int).cast::<c_void>(),
        );
        if rc < 0 {
            return Err(AppError::new(
                ErrorCode::MpvIpcError,
                format!("mpv_set_property({}, {}) 失败: {}", name, flag, self.error_str(rc)),
            ));
        }
        Ok(())
    }

    pub unsafe fn set_property_f64(
        &self,
        ctx: *mut mpv_handle,
        name: &str,
        val: f64,
    ) -> AppResult<()> {
        let c_name = CString::new(name).unwrap_or_default();
        let rc = (self.mpv_set_property)(
            ctx,
            c_name.as_ptr(),
            MPV_FORMAT_DOUBLE,
            (&val as *const f64).cast::<c_void>(),
        );
        if rc < 0 {
            return Err(AppError::new(
                ErrorCode::MpvIpcError,
                format!("mpv_set_property({}, {}) 失败: {}", name, val, self.error_str(rc)),
            ));
        }
        Ok(())
    }

    pub unsafe fn set_property_str_array(
        &self,
        ctx: *mut mpv_handle,
        name: &str,
        items: &[String],
    ) -> AppResult<()> {
        let c_name = CString::new(name).unwrap_or_default();
        let c_strings: Vec<CString> = items
            .iter()
            .map(|s| CString::new(s.as_str()).unwrap_or_default())
            .collect();
        let mut nodes: Vec<mpv_node> = c_strings
            .iter()
            .map(|cs| mpv_node {
                u: mpv_node_u {
                    string: cs.as_ptr() as *mut c_char,
                },
                format: MPV_FORMAT_STRING,
            })
            .collect();
        let mut list = mpv_node_list {
            num: nodes.len() as c_int,
            values: if nodes.is_empty() {
                std::ptr::null_mut()
            } else {
                nodes.as_mut_ptr()
            },
            keys: std::ptr::null_mut(),
        };
        let root_node = mpv_node {
            u: mpv_node_u {
                list: &mut list as *mut mpv_node_list,
            },
            format: MPV_FORMAT_NODE_ARRAY,
        };
        let rc = (self.mpv_set_property)(
            ctx,
            c_name.as_ptr(),
            MPV_FORMAT_NODE,
            (&root_node as *const mpv_node).cast::<c_void>(),
        );
        if rc < 0 {
            return Err(AppError::new(
                ErrorCode::MpvIpcError,
                format!("mpv_set_property({}, array) 失败: {}", name, self.error_str(rc)),
            ));
        }
        Ok(())
    }

    pub unsafe fn observe_property(
        &self,
        ctx: *mut mpv_handle,
        reply_userdata: u64,
        name: &str,
        format: c_int,
    ) -> AppResult<()> {
        let c_name = CString::new(name).unwrap_or_default();
        let rc = (self.mpv_observe_property)(ctx, reply_userdata, c_name.as_ptr(), format);
        if rc < 0 {
            return Err(AppError::new(
                ErrorCode::MpvIpcError,
                format!("mpv_observe_property({}) 失败: {}", name, self.error_str(rc)),
            ));
        }
        Ok(())
    }

    pub unsafe fn command_strs(&self, ctx: *mut mpv_handle, args: &[&str]) -> AppResult<()> {
        let c_args: Vec<CString> = args
            .iter()
            .map(|s| CString::new(*s).unwrap_or_default())
            .collect();
        let mut ptrs: Vec<*const c_char> = c_args.iter().map(|cs| cs.as_ptr()).collect();
        ptrs.push(std::ptr::null());
        let rc = (self.mpv_command)(ctx, ptrs.as_ptr());
        if rc < 0 {
            return Err(AppError::new(
                ErrorCode::MpvIpcError,
                format!("mpv_command({:?}) 失败: {}", args, self.error_str(rc)),
            ));
        }
        Ok(())
    }

    pub unsafe fn get_property_json(&self, ctx: *mut mpv_handle, name: &str) -> AppResult<Value> {
        let c_name = CString::new(name).unwrap_or_default();
        let mut out_node: mpv_node = std::mem::zeroed();
        let rc = (self.mpv_get_property)(
            ctx,
            c_name.as_ptr(),
            MPV_FORMAT_NODE,
            (&mut out_node as *mut mpv_node).cast::<c_void>(),
        );
        if rc < 0 {
            return Err(AppError::new(
                ErrorCode::MpvIpcError,
                format!("mpv_get_property({}) 失败: {}", name, self.error_str(rc)),
            ));
        }
        let val = mpv_node_to_json(&out_node);
        (self.mpv_free_node_contents)(&mut out_node);
        Ok(val)
    }

    /// Executes a JSON array command (compatible with our `MpvController::command` contract) directly via `libmpv` C-API.
    pub unsafe fn execute_json_command(
        &self,
        ctx: *mut mpv_handle,
        cmd: &Value,
    ) -> AppResult<Value> {
        let arr = cmd.as_array().ok_or_else(|| {
            AppError::new(ErrorCode::InvalidInput, "mpv 命令必须为数组格式")
        })?;
        if arr.is_empty() {
            return Ok(Value::Null);
        }
        let op = arr[0].as_str().unwrap_or("");
        match op {
            "set_property" if arr.len() >= 3 => {
                let prop = arr[1].as_str().unwrap_or("");
                let val = &arr[2];
                match val {
                    Value::Bool(b) => self.set_property_flag(ctx, prop, *b)?,
                    Value::Number(n) => {
                        if let Some(f) = n.as_f64() {
                            self.set_property_f64(ctx, prop, f)?;
                        }
                    }
                    Value::String(s) => self.set_property_str(ctx, prop, s)?,
                    Value::Array(items) => {
                        let strs: Vec<String> = items
                            .iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect();
                        self.set_property_str_array(ctx, prop, &strs)?;
                    }
                    _ => {}
                }
                Ok(Value::Null)
            }
            "get_property" if arr.len() >= 2 => {
                let prop = arr[1].as_str().unwrap_or("");
                self.get_property_json(ctx, prop)
            }
            "observe_property" if arr.len() >= 3 => {
                let id = arr[1].as_u64().unwrap_or(0);
                let prop = arr[2].as_str().unwrap_or("");
                let fmt = match prop {
                    "time-pos" | "duration" | "audio-bitrate" => MPV_FORMAT_DOUBLE,
                    "pause" | "paused-for-cache" => MPV_FORMAT_FLAG,
                    "audio-params/samplerate" => MPV_FORMAT_INT64,
                    "audio-codec-name" | "audio-params/format" | "audio-params/channels" => {
                        MPV_FORMAT_STRING
                    }
                    _ => MPV_FORMAT_NODE,
                };
                self.observe_property(ctx, id, prop, fmt)?;
                Ok(Value::Null)
            }
            "quit" => Ok(Value::Null),
            _ => {
                let str_args: Vec<String> = arr
                    .iter()
                    .map(|v| match v {
                        Value::String(s) => s.clone(),
                        Value::Number(n) => n.to_string(),
                        Value::Bool(b) => if *b { "yes" } else { "no" }.to_string(),
                        _ => v.to_string(),
                    })
                    .collect();
                let refs: Vec<&str> = str_args.iter().map(|s| s.as_str()).collect();
                self.command_strs(ctx, &refs)?;
                Ok(Value::Null)
            }
        }
    }
}

pub unsafe fn mpv_node_to_json(node: &mpv_node) -> Value {
    match node.format {
        MPV_FORMAT_NONE => Value::Null,
        MPV_FORMAT_STRING | MPV_FORMAT_OSD_STRING => {
            if node.u.string.is_null() {
                Value::Null
            } else {
                Value::String(CStr::from_ptr(node.u.string).to_string_lossy().into_owned())
            }
        }
        MPV_FORMAT_FLAG => Value::Bool(node.u.flag != 0),
        MPV_FORMAT_INT64 => Value::Number(Number::from(node.u.int64)),
        MPV_FORMAT_DOUBLE => Number::from_f64(node.u.double_)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        MPV_FORMAT_NODE_ARRAY => {
            if node.u.list.is_null() {
                return Value::Array(Vec::new());
            }
            let list = &*node.u.list;
            let count = list.num.max(0) as usize;
            let mut out = Vec::with_capacity(count);
            if !list.values.is_null() {
                for i in 0..count {
                    let elem = &*list.values.add(i);
                    out.push(mpv_node_to_json(elem));
                }
            }
            Value::Array(out)
        }
        MPV_FORMAT_NODE_MAP => {
            if node.u.list.is_null() {
                return Value::Object(Map::new());
            }
            let list = &*node.u.list;
            let count = list.num.max(0) as usize;
            let mut map = Map::with_capacity(count);
            if !list.values.is_null() && !list.keys.is_null() {
                for i in 0..count {
                    let k_ptr = *list.keys.add(i);
                    if !k_ptr.is_null() {
                        let key = CStr::from_ptr(k_ptr).to_string_lossy().into_owned();
                        let elem = &*list.values.add(i);
                        map.insert(key, mpv_node_to_json(elem));
                    }
                }
            }
            Value::Object(map)
        }
        _ => Value::Null,
    }
}

pub unsafe fn property_event_to_json(prop: &mpv_event_property) -> (String, Value) {
    let name = if prop.name.is_null() {
        String::new()
    } else {
        CStr::from_ptr(prop.name).to_string_lossy().into_owned()
    };
    if prop.data.is_null() {
        return (name, Value::Null);
    }
    let val = match prop.format {
        MPV_FORMAT_NONE => Value::Null,
        MPV_FORMAT_NODE => mpv_node_to_json(&*(prop.data as *const mpv_node)),
        MPV_FORMAT_DOUBLE => Number::from_f64(*(prop.data as *const f64))
            .map(Value::Number)
            .unwrap_or(Value::Null),
        MPV_FORMAT_FLAG => Value::Bool(*(prop.data as *const c_int) != 0),
        MPV_FORMAT_INT64 => Value::Number(Number::from(*(prop.data as *const i64))),
        MPV_FORMAT_STRING | MPV_FORMAT_OSD_STRING => {
            let str_ptr = *(prop.data as *const *const c_char);
            if str_ptr.is_null() {
                Value::Null
            } else {
                Value::String(CStr::from_ptr(str_ptr).to_string_lossy().into_owned())
            }
        }
        _ => Value::Null,
    };
    (name, val)
}

