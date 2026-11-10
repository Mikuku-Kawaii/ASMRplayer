//! T1513: bounded analysis audio, isolated from the audible playback handle/DSP.
use super::speech_segments::{RecognitionResult, CHUNK_MS};
use crate::{
    domain::{AppError, AppResult, ErrorCode},
    player::libmpv_ffi::*,
};
use std::{
    ffi::{c_char, c_void, CStr, CString},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

fn error(message: &str) -> AppError {
    AppError::new(ErrorCode::MediaDecodeFailed, message)
}

// The PCM cache includes both configurable margins. Individual ASR requests
// remain bounded to 20 seconds by asr_quality and dialogue_review.
pub(super) const ANALYSIS_PADDING_MS: i64 = 2_000;
pub(super) const MAX_ANALYSIS_PADDING_MS: i64 = 8_000;
const MAX_ANALYSIS_MS: i64 = CHUNK_MS + 2 * MAX_ANALYSIS_PADDING_MS;

fn validate_decode_range(start_ms: i64, length_ms: i64) -> AppResult<()> {
    if start_ms < 0
        || length_ms <= 0
        || length_ms > MAX_ANALYSIS_MS
        || start_ms.checked_add(length_ms).is_none()
    {
        return Err(error("分析音频分段范围无效"));
    }
    Ok(())
}
fn cstring(value: &str) -> AppResult<CString> {
    CString::new(value).map_err(|_| error("音源或模型路径包含无效字符"))
}

#[cfg(windows)]
extern "C" {
    fn asmr_sv_create(
        dll: *const u16,
        model: *const c_char,
        tokens: *const c_char,
        language: *const c_char,
        provider: *const c_char,
    ) -> *mut c_void;
    fn asmr_sv_decode(engine: *mut c_void, samples: *const f32, count: i32) -> *mut c_char;
    fn asmr_sv_free_text(text: *mut c_char);
    fn asmr_sv_destroy(engine: *mut c_void);
}
pub struct SenseVoice {
    handle: *mut c_void,
}
impl SenseVoice {
    pub fn new(runtime: &Path, model: &Path, language: &str) -> AppResult<Self> {
        Self::with_provider(runtime, model, language, "model.int8.onnx", "cpu")
    }
    pub(super) fn with_provider(
        runtime: &Path,
        model: &Path,
        language: &str,
        filename: &str,
        provider: &str,
    ) -> AppResult<Self> {
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            let dll: Vec<u16> = runtime
                .join("sherpa-onnx-c-api.dll")
                .as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect();
            let model_file = cstring(&model.join(filename).to_string_lossy())?;
            let tokens = cstring(&model.join("tokens.txt").to_string_lossy())?;
            let language = cstring(language)?;
            let provider = cstring(provider)?;
            let handle = unsafe {
                asmr_sv_create(
                    dll.as_ptr(),
                    model_file.as_ptr(),
                    tokens.as_ptr(),
                    language.as_ptr(),
                    provider.as_ptr(),
                )
            };
            if handle.is_null() {
                return Err(error(
                    "SenseVoice 初始化失败，请在识别与翻译设置中检查模型及 1.12.20 运行库",
                ));
            }
            Ok(Self { handle })
        }
        #[cfg(not(windows))]
        {
            let _ = (runtime, model, language, filename, provider);
            Err(error("当前识别运行库仅支持 Windows x64"))
        }
    }
    pub fn transcribe(&mut self, samples: &[f32]) -> AppResult<RecognitionResult> {
        if samples.is_empty() || samples.len() > 20 * 16000 {
            return Err(error("识别音频片段长度无效"));
        }
        #[cfg(windows)]
        unsafe {
            let result = asmr_sv_decode(self.handle, samples.as_ptr(), samples.len() as i32);
            if result.is_null() {
                return Err(error("SenseVoice 未返回识别结果"));
            }
            let text = CStr::from_ptr(result).to_string_lossy().into_owned();
            asmr_sv_free_text(result);
            serde_json::from_str(&text).map_err(|_| error("SenseVoice 返回的识别结果格式无效"))
        }
        #[cfg(not(windows))]
        {
            Err(error("当前识别运行库仅支持 Windows x64"))
        }
    }
}
impl Drop for SenseVoice {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            asmr_sv_destroy(self.handle);
        }
    }
}
pub fn clean_text(text: &str) -> String {
    let mut remaining = text;
    let mut result = String::new();
    while let Some(start) = remaining.find("<|") {
        result.push_str(&remaining[..start]);
        if let Some(end) = remaining[start..].find("|>") {
            remaining = &remaining[start + end + 2..];
        } else {
            remaining = &remaining[start..];
            break;
        }
    }
    result.push_str(remaining);
    result.trim().to_owned()
}

/// Short breaths/interjections are poor language samples. Use a longer audio
/// transcript to recognize Japanese context, never classify shared Han characters
/// as Chinese or rewrite the recognized text into another language.
pub fn contextual_language<'a>(requested: &'a str, context: &str) -> &'a str {
    if requested != "auto" {
        return requested;
    }
    let text = clean_text(context);
    let letters = text.chars().filter(|c| c.is_alphabetic()).count();
    let kana = text
        .chars()
        .filter(|c| matches!(c, '\u{3041}'..='\u{3096}' | '\u{30a1}'..='\u{30fa}'))
        .count();
    if kana >= 6 && kana * 4 >= letters {
        "ja"
    } else {
        "auto"
    }
}

pub fn contextual_language_with_title<'a>(
    requested: &'a str,
    context: &str,
    title: &str,
) -> &'a str {
    let language = contextual_language(requested, context);
    if language != "auto" {
        return language;
    }
    // A substantial audio transcript outranks a translated or multilingual title.
    if clean_text(context)
        .chars()
        .filter(|c| c.is_alphabetic())
        .count()
        >= 6
    {
        return "auto";
    }
    let kana = title
        .chars()
        .filter(|c| matches!(c, '\u{3041}'..='\u{3096}' | '\u{30a1}'..='\u{30fa}'))
        .count();
    let hangul = title
        .chars()
        .filter(|c| matches!(c, '\u{ac00}'..='\u{d7af}'))
        .count();
    if kana >= 3 && hangul == 0 {
        "ja"
    } else if hangul >= 3 && kana == 0 {
        "ko"
    } else {
        "auto"
    } // Han-only titles cannot distinguish Chinese from Japanese.
}

/// Without independent timing evidence the complete text retains its audio interval.
pub fn short_phrases(text: &str, start_ms: i64, end_ms: i64) -> Vec<(i64, i64, String)> {
    let text = clean_text(text);
    if end_ms <= start_ms || text.trim().is_empty() {
        Vec::new()
    } else {
        vec![(start_ms, end_ms, text)]
    }
}

pub(super) fn is_punctuation(c: char) -> bool {
    c.is_ascii_punctuation()
        || "。．！？，、；：「」『』【】〔〕（）［］｛｝〈〉《》“”‘’…‥—–〜～・﹏＂＇／＼"
            .contains(c)
}

struct AnalysisHandle {
    api: &'static LibMpvApi,
    handle: *mut mpv_handle,
}

/// Read duration for a background track without changing or opening audible playback.
pub fn probe_duration(
    uri: &str,
    headers: &[(String, String)],
    cancel: &AtomicBool,
) -> AppResult<i64> {
    let api = get_libmpv_api()?;
    let handle = unsafe { (api.mpv_create)() };
    if handle.is_null() {
        return Err(error("无法读取音轨时长"));
    }
    let _instance = AnalysisHandle { api, handle };
    for (name, value) in [
        ("config", "no"),
        ("load-scripts", "no"),
        ("terminal", "no"),
        ("msg-level", "all=no"),
        ("video", "no"),
        ("audio-display", "no"),
        ("ao", "null"),
        ("pause", "yes"),
        ("idle", "yes"),
        ("network-timeout", "20"),
    ] {
        if unsafe {
            (api.mpv_set_option_string)(handle, cstring(name)?.as_ptr(), cstring(value)?.as_ptr())
        } < 0
        {
            return Err(error("音轨分析参数不可用"));
        }
    }
    if !headers.is_empty() {
        let value = headers
            .iter()
            .map(|(k, v)| format!("{}: {}", k, v).replace(',', "\\,"))
            .collect::<Vec<_>>()
            .join(",");
        if unsafe {
            (api.mpv_set_option_string)(
                handle,
                cstring("http-header-fields")?.as_ptr(),
                cstring(&value)?.as_ptr(),
            )
        } < 0
        {
            return Err(error("音源访问参数不可用"));
        }
    }
    if unsafe { (api.mpv_initialize)(handle) } < 0 {
        return Err(error("音轨分析初始化失败"));
    }
    let args = [cstring("loadfile")?, cstring(uri)?];
    let pointers = [args[0].as_ptr(), args[1].as_ptr(), std::ptr::null()];
    if unsafe { (api.mpv_command)(handle, pointers.as_ptr()) } < 0 {
        return Err(error("无法打开识别音源"));
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if cancel.load(Ordering::SeqCst) {
            return Err(error("已取消识别"));
        }
        let event = unsafe { &*(api.mpv_wait_event)(handle, 0.05) };
        let mut seconds = 0.0f64;
        let status = unsafe {
            (api.mpv_get_property)(
                handle,
                cstring("duration")?.as_ptr(),
                MPV_FORMAT_DOUBLE,
                &mut seconds as *mut f64 as *mut c_void,
            )
        };
        if status >= 0 && seconds.is_finite() && seconds > 0.0 {
            return Ok((seconds * 1000.0).round() as i64);
        }
        if event.event_id == MPV_EVENT_END_FILE {
            break;
        }
    }
    Err(error("无法读取音轨时长，请检查音源或下载离线文件后重试"))
}
impl Drop for AnalysisHandle {
    fn drop(&mut self) {
        unsafe {
            (self.api.mpv_terminate_destroy)(self.handle);
        }
    }
}

/// Decode a bounded cache block plus the recognition context on each side.
pub fn decode_chunk(
    uri: &str,
    headers: &[(String, String)],
    start_ms: i64,
    length_ms: i64,
    output: &Path,
    cancel: &AtomicBool,
) -> AppResult<Vec<f32>> {
    if cancel.load(Ordering::SeqCst) {
        return Err(error("已取消识别"));
    }
    validate_decode_range(start_ms, length_ms)?;
    let api = get_libmpv_api()?;
    let handle = unsafe { (api.mpv_create)() };
    if handle.is_null() {
        return Err(error("无法创建独立音频分析实例"));
    }
    let instance = AnalysisHandle { api, handle };
    let options = [
        ("config", "no".into()),
        ("load-scripts", "no".into()),
        ("terminal", "no".into()),
        ("msg-level", "all=no".into()),
        ("video", "no".into()),
        ("audio-display", "no".into()),
        ("ao", "pcm".into()),
        ("ao-pcm-waveheader", "no".into()),
        ("ao-pcm-file", output.to_string_lossy().into_owned()),
        ("audio-format", "float".into()),
        ("audio-samplerate", "16000".into()),
        ("audio-channels", "mono".into()),
        ("volume", "100".into()),
        ("replaygain", "no".into()),
        ("idle", "yes".into()),
        ("start", format!("{:.3}", start_ms as f64 / 1000.0)),
        ("length", format!("{:.3}", length_ms as f64 / 1000.0)),
        ("network-timeout", "20".into()),
    ];
    for (name, value) in options {
        let name = cstring(name)?;
        let value = cstring(&value)?;
        if unsafe { (api.mpv_set_option_string)(handle, name.as_ptr(), value.as_ptr()) } < 0 {
            return Err(error(&format!(
                "音频分析参数不受当前 libmpv 支持：{}",
                name.to_string_lossy()
            )));
        }
    }
    if !headers.is_empty() {
        let value = headers
            .iter()
            .map(|(k, v)| format!("{}: {}", k, v).replace(',', "\\,"))
            .collect::<Vec<_>>()
            .join(",");
        if unsafe {
            (api.mpv_set_option_string)(
                handle,
                cstring("http-header-fields")?.as_ptr(),
                cstring(&value)?.as_ptr(),
            )
        } < 0
        {
            return Err(error("无法设置音源访问参数"));
        }
    }
    if unsafe { (api.mpv_initialize)(handle) } < 0 {
        return Err(error("音频分析实例初始化失败"));
    }
    let command = [cstring("loadfile")?, cstring(uri)?];
    let args = [command[0].as_ptr(), command[1].as_ptr(), std::ptr::null()];
    if unsafe { (api.mpv_command)(handle, args.as_ptr()) } < 0 {
        return Err(error("无法打开识别音源"));
    }
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(error("已取消识别"));
        }
        if Instant::now() > deadline {
            return Err(error("读取音源超时，可下载为离线文件后重试"));
        }
        let event = unsafe { &*(api.mpv_wait_event)(handle, 0.05) };
        if event.event_id == MPV_EVENT_END_FILE {
            let end = unsafe { &*(event.data as *const mpv_event_end_file) };
            if end.reason != MPV_END_FILE_REASON_EOF {
                return Err(error("读取音源失败，可下载为离线文件后重试"));
            }
            break;
        }
    }
    drop(instance); // flush PCM before reading it
    let bytes = std::fs::read(output).map_err(|_| error("无法读取分析音频"))?;
    // Allow up to one second of decoder frame padding, then trim to the request.
    if bytes.len() % 4 != 0 || bytes.len() > ((MAX_ANALYSIS_MS + 1000) * 16 * 4) as usize {
        return Err(error("分析音频格式或长度无效"));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|s| f32::from_le_bytes(s.try_into().unwrap()))
        .take((length_ms * 16) as usize)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t1513_language_context_requires_japanese_evidence_and_honors_explicit_choice() {
        let japanese = "<|ja|>うん そうすると 思ったより おやすみなさい";
        assert_eq!(contextual_language("auto", japanese), "ja");
        for context in [
            "嗯",
            "呼",
            "君",
            "日本語",
            "おや",
            "你好 今天一起听音乐",
            "hello and good night",
        ] {
            assert_eq!(contextual_language("auto", context), "auto");
        }
        for explicit in ["ja", "zh", "en", "ko", "yue"] {
            assert_eq!(contextual_language(explicit, japanese), explicit);
        }
        assert_eq!(clean_text("嗯"), "嗯"); // No text replacement or deletion.
    }
    #[test]
    fn t1513_title_language_is_only_a_weak_fallback() {
        assert_eq!(
            contextual_language_with_title("auto", "嗯", "おやすみ耳かき"),
            "ja"
        );
        assert_eq!(
            contextual_language_with_title("auto", "", "좋은 밤 소리"),
            "ko"
        );
        assert_eq!(
            contextual_language_with_title("auto", "嗯", "中文标题"),
            "auto"
        );
        assert_eq!(
            contextual_language_with_title(
                "auto",
                "hello this is an English recording",
                "おやすみ耳かき"
            ),
            "auto"
        );
        assert_eq!(
            contextual_language_with_title("zh", "嗯", "おやすみ耳かき"),
            "zh"
        );
    }
    #[test]
    fn t1513_cancelled_decode_does_not_open_output_or_load_engine() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("audio.pcm");
        assert!(decode_chunk("unused", &[], 0, 1000, &out, &AtomicBool::new(true)).is_err());
        assert!(!out.exists());
    }
    #[test]
    fn t1513_analysis_range_accepts_standard_and_extended_padding_but_stays_bounded() {
        for context in [ANALYSIS_PADDING_MS, 2500, MAX_ANALYSIS_PADDING_MS] {
            // Ordinary chunks and the 12-second Chinese-reference path.
            for core in [CHUNK_MS, 12_000] {
                assert!(validate_decode_range(0, core + context).is_ok());
                assert!(validate_decode_range(40_000 - context, core + 2 * context).is_ok());
                assert!(validate_decode_range(60_000 - context, 500 + context).is_ok());
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("invalid.pcm");
        for (start, length) in [
            (-1, 1000),
            (0, 0),
            (0, -1),
            (0, MAX_ANALYSIS_MS + 1),
            (i64::MAX, 1),
        ] {
            assert!(
                decode_chunk("unused", &[], start, length, &out, &AtomicBool::new(false)).is_err()
            );
            assert!(!out.exists(), "invalid range must not start a decoder");
        }
    }
    #[test]
    #[ignore = "requires the packaged libmpv runtime; synthetic fixture only"]
    fn t1513_native_analysis_decodes_resamples_and_seeks_without_speaker_output() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("音频 analysis.wav");
        let rate = 44100u32;
        let data: Vec<u8> = (0..rate * 64)
            .flat_map(|i| {
                let amplitude = if i < rate { 0.0 } else { 12000.0 };
                ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * amplitude)
                    .round()
                    .to_le_bytes()
            })
            .collect();
        // IEEE float WAV, 44.1 kHz mono. The second second is a 440 Hz tone.
        let mut wav = b"RIFF".to_vec();
        wav.extend((36 + data.len() as u32).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(3u16.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(rate.to_le_bytes());
        wav.extend((rate * 4).to_le_bytes());
        wav.extend(4u16.to_le_bytes());
        wav.extend(32u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend((data.len() as u32).to_le_bytes());
        // Scale to the floating point WAV range before writing.
        wav.extend(
            data.chunks_exact(4)
                .flat_map(|v| (f32::from_le_bytes(v.try_into().unwrap()) / 32768.0).to_le_bytes()),
        );
        std::fs::write(&input, wav).unwrap();
        let duration =
            probe_duration(input.to_str().unwrap(), &[], &AtomicBool::new(false)).unwrap();
        assert!((63990..=64010).contains(&duration), "duration {duration}");
        let samples = decode_chunk(
            input.to_str().unwrap(),
            &[],
            1000,
            1000,
            &dir.path().join("out.pcm"),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(
            (15900..=16000).contains(&samples.len()),
            "{} samples",
            samples.len()
        );
        assert!(samples.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
        let rms = (samples.iter().map(|v| v * v).sum::<f32>() / samples.len() as f32).sqrt();
        assert!((0.2..0.3).contains(&rms), "RMS {rms}");
        for padding in [ANALYSIS_PADDING_MS, 2500, MAX_ANALYSIS_PADDING_MS] {
            // First chunk, middle chunk, and EOF tail, with the caller's real budget.
            for start in [0, CHUNK_MS, 3 * CHUNK_MS] {
                let end = (start + CHUNK_MS).min(duration);
                let decode_start = (start - padding).max(0);
                let length = (end + padding).min(duration) - decode_start;
                let context = decode_chunk(
                    input.to_str().unwrap(),
                    &[],
                    decode_start,
                    length,
                    &dir.path().join("context.pcm"),
                    &AtomicBool::new(false),
                )
                .unwrap();
                let expected = (length * 16) as usize;
                assert!(
                    (expected - 100..=expected).contains(&context.len()),
                    "padding={padding}, start={start}, length={length}, samples={}",
                    context.len()
                );
                assert!(context.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
            }
        }
        assert!(decode_chunk(
            input.to_str().unwrap(),
            &[],
            0,
            MAX_ANALYSIS_MS + 1,
            &dir.path().join("invalid.pcm"),
            &AtomicBool::new(false)
        )
        .is_err());
    }
}
