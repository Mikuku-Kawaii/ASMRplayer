//! Local Whisper with explicit glossary context. One native context per task.
use super::speech_segments::RecognitionResult;
use crate::domain::{AppError, AppResult, ErrorCode};
use std::{
    ffi::{c_char, c_void, CStr, CString},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

fn error(message: &str) -> AppError {
    AppError::new(ErrorCode::MediaDecodeFailed, message)
}
#[cfg(windows)]
extern "C" {
    fn asmr_whisper_create(dll: *const u16, model: *const u16) -> *mut c_void;
    fn asmr_whisper_destroy(handle: *mut c_void);
    fn asmr_whisper_decode(
        handle: *mut c_void,
        samples: *const f32,
        count: i32,
        language: *const c_char,
        prompt: *const c_char,
        cancel: extern "C" fn(*mut c_void) -> bool,
        cancel_data: *mut c_void,
    ) -> i32;
    fn asmr_whisper_segments(handle: *mut c_void) -> i32;
    fn asmr_whisper_text(handle: *mut c_void, index: i32) -> *const c_char;
    fn asmr_whisper_t0(handle: *mut c_void, index: i32) -> i64;
    fn asmr_whisper_t1(handle: *mut c_void, index: i32) -> i64;
    fn asmr_whisper_no_speech(handle: *mut c_void, index: i32) -> f32;
    fn asmr_whisper_prompt(handle: *mut c_void) -> *const c_char;
}
extern "C" fn cancelled(data: *mut c_void) -> bool {
    // The caller keeps the AtomicBool alive throughout the synchronous native call.
    unsafe { (&*(data as *const AtomicBool)).load(Ordering::SeqCst) }
}
pub struct Whisper {
    handle: *mut c_void,
    language: CString,
    prompt: CString,
}
impl Whisper {
    pub fn set_language(&mut self, language: &str) -> AppResult<()> {
        self.language = CString::new(language).map_err(|_| error("识别语言无效"))?;
        Ok(())
    }
    pub fn new(directory: &Path, language: &str, prompt: &str) -> AppResult<Self> {
        if language == "yue" {
            return Err(error(
                "Whisper small 不支持独立粤语选项，请选择自动判断或使用 SenseVoice",
            ));
        }
        let language = CString::new(language).map_err(|_| error("识别语言无效"))?;
        let prompt = CString::new(prompt).map_err(|_| error("识别词库含有无效字符"))?;
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            let wide = |name: &str| {
                directory
                    .join(name)
                    .as_os_str()
                    .encode_wide()
                    .chain(Some(0))
                    .collect::<Vec<_>>()
            };
            let handle = unsafe {
                asmr_whisper_create(
                    wide("whisper.dll").as_ptr(),
                    wide("ggml-small-q5_1.bin").as_ptr(),
                )
            };
            if handle.is_null() {
                return Err(error(
                    "Whisper 初始化失败，请在识别与翻译设置中检查 v1.8.2 运行库与 small Q5_1 模型",
                ));
            }
            Ok(Self {
                handle,
                language,
                prompt,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = (directory, language, prompt);
            Err(error("Whisper 运行库仅支持 Windows x64"))
        }
    }
    pub fn transcribe(
        &mut self,
        samples: &[f32],
        cancel: &AtomicBool,
    ) -> AppResult<RecognitionResult> {
        self.transcribe_comparison(samples, cancel, false)
    }
    pub fn transcribe_comparison(
        &mut self,
        samples: &[f32],
        cancel: &AtomicBool,
        without_prompt: bool,
    ) -> AppResult<RecognitionResult> {
        if cancel.load(Ordering::SeqCst) {
            return Err(error("已取消识别"));
        }
        if samples.is_empty()
            || samples.len() > 20 * 16000
            || samples.iter().any(|v| !v.is_finite())
        {
            return Err(error("识别音频片段无效"));
        }
        #[cfg(windows)]
        unsafe {
            let status = asmr_whisper_decode(
                self.handle,
                samples.as_ptr(),
                samples.len() as i32,
                self.language.as_ptr(),
                if without_prompt {
                    c"".as_ptr()
                } else {
                    self.prompt.as_ptr()
                },
                cancelled,
                cancel as *const AtomicBool as *mut c_void,
            );
            if cancel.load(Ordering::SeqCst) {
                return Err(error("已取消识别"));
            }
            if status != 0 {
                return Err(error("Whisper 识别失败，已完成片段保留"));
            }
            let mut text = String::new();
            let mut segments = Vec::new();
            for index in 0..asmr_whisper_segments(self.handle) {
                let segment = asmr_whisper_text(self.handle, index);
                if segment.is_null() {
                    return Err(error("Whisper 返回的识别结果无效"));
                }
                let value = CStr::from_ptr(segment).to_string_lossy();
                text.push_str(&value);
                segments.push(crate::domain::asr::SegmentEvidence {
                    start: asmr_whisper_t0(self.handle, index) as f64 / 100.,
                    end: asmr_whisper_t1(self.handle, index) as f64 / 100.,
                    text: value.into_owned(),
                    no_speech_prob: Some(asmr_whisper_no_speech(self.handle, index) as f64),
                    ..Default::default()
                });
            }
            Ok(RecognitionResult {
                text,
                segments,
                config: serde_json::json!({"backend":"whisper.cpp-1.8.2","no_context":true,"temperature":0,"temperature_inc":0,"beam_size":5,"no_speech_thold":1.0,"suppress_nst":false,"word_timestamps":false,"n_threads":2,"n_max_text_ctx":224,"translate":false,"initial_prompt":CStr::from_ptr(asmr_whisper_prompt(self.handle)).to_string_lossy()}),
                ..Default::default()
            })
        }
        #[cfg(not(windows))]
        {
            Err(error("Whisper 运行库仅支持 Windows x64"))
        }
    }
}
impl Drop for Whisper {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            asmr_whisper_destroy(self.handle);
        }
    }
}
