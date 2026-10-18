//! CUDA is isolated from the player's CPU ONNX DLLs and native process failures.
use super::{speech_engine::SenseVoice, speech_segments::RecognitionResult};
use crate::domain::{AppError, AppResult, ErrorCode};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const PREFIX: &str = "ASMR_GPU:";
fn error(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::MediaDecodeFailed, message.into())
}

pub struct GpuRecognizer {
    child: Child,
    input: ChildStdin,
    output: mpsc::Receiver<String>,
    pub device: String,
}
impl GpuRecognizer {
    pub fn new(directory: &Path, language: &str, cancel: &AtomicBool) -> AppResult<Self> {
        Self::spawn(&std::env::current_exe().map_err(|_| error("无法定位 GPU 识别程序"))?, directory, language, cancel)
    }
    #[cfg(test)]
    pub(crate) fn for_local_test(executable: &Path, directory: &Path, language: &str, cancel: &AtomicBool) -> AppResult<Self> {
        Self::spawn(executable, directory, language, cancel)
    }
    fn spawn(executable: &Path, directory: &Path, language: &str, cancel: &AtomicBool) -> AppResult<Self> {
        let mut command = Command::new(executable);
        command
            .arg("--sensevoice-gpu-worker")
            .arg(directory)
            .arg(language)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command
            .spawn()
            .map_err(|_| error("无法启动 GPU 识别进程"))?;
        let input = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let (tx, rx) = mpsc::sync_channel(2);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                // A malformed worker cannot grow a line without bound.
                if reader
                    .by_ref()
                    .take(2 * 1024 * 1024)
                    .read_line(&mut line)
                    .unwrap_or(0)
                    == 0
                {
                    break;
                }
                if !line.ends_with('\n') {
                    break;
                }
                if let Some(message) = line.strip_prefix(PREFIX) {
                    if tx.send(message.trim().to_owned()).is_err() {
                        break;
                    }
                }
            }
        });
        let mut engine = Self {
            child,
            input,
            output: rx,
            device: String::new(),
        };
        let ready = engine.receive(cancel)?;
        engine.device = ready["device"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| error("GPU 识别进程未报告 CUDA 设备"))?
            .to_owned();
        Ok(engine)
    }
    fn receive(&mut self, cancel: &AtomicBool) -> AppResult<serde_json::Value> {
        let deadline = Instant::now() + Duration::from_secs(180);
        loop {
            if cancel.load(Ordering::SeqCst) {
                let _ = self.child.kill();
                return Err(error("已取消识别"));
            }
            match self.output.recv_timeout(Duration::from_millis(100)) {
                Ok(line) => {
                    let value: serde_json::Value =
                        serde_json::from_str(&line).map_err(|_| error("GPU 识别响应格式无效"))?;
                    if let Some(message) = value["error"].as_str() {
                        return Err(error(message));
                    }
                    return Ok(value);
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(error(
                        "GPU 识别进程退出，请检测 GPU 或检查 NVIDIA 驱动；已完成字幕保留",
                    ))
                }
                Err(_) if Instant::now() >= deadline => {
                    let _ = self.child.kill();
                    return Err(error("GPU 识别超时，已完成字幕保留"));
                }
                Err(_) => {}
            }
        }
    }
    pub fn transcribe(
        &mut self,
        samples: &[f32],
        cancel: &AtomicBool,
    ) -> AppResult<RecognitionResult> {
        validate_audio(samples)?;
        if cancel.load(Ordering::SeqCst) {
            return Err(error("已取消识别"));
        }
        let mut bytes = Vec::with_capacity(4 + samples.len() * 4);
        bytes.extend_from_slice(&(samples.len() as u32).to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        self.input
            .write_all(&bytes)
            .and_then(|_| self.input.flush())
            .map_err(|_| error("GPU 识别进程已断开，已完成字幕保留"))?;
        serde_json::from_value(self.receive(cancel)?).map_err(|_| error("GPU 识别返回格式无效"))
    }
}
impl Drop for GpuRecognizer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn validate_audio(samples: &[f32]) -> AppResult<()> {
    if samples.is_empty() || samples.len() > 20 * 16000 || samples.iter().any(|v| !v.is_finite()) {
        Err(error("GPU 识别音频片段无效"))
    } else {
        Ok(())
    }
}
fn publish(value: serde_json::Value) -> AppResult<()> {
    let mut out = std::io::stdout().lock();
    writeln!(out, "{PREFIX}{value}")
        .and_then(|_| out.flush())
        .map_err(|_| error("识别响应管道已关闭"))
}
pub fn worker(directory: &Path, language: &str) -> AppResult<()> {
    if !["auto", "zh", "ja", "en", "ko", "yue"].contains(&language) {
        return Err(error("识别语言无效"));
    }
    let device = probe_cuda(directory)?;
    let mut engine =
        SenseVoice::with_provider(directory, directory, language, "model.fp16.onnx", "cuda")?;
    publish(serde_json::json!({"device":device}))?;
    let mut input = std::io::stdin().lock();
    loop {
        let mut count = [0u8; 4];
        match input.read_exact(&mut count) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(_) => return Err(error("识别输入管道已关闭")),
        }
        let count = u32::from_le_bytes(count) as usize;
        if count == 0 || count > 20 * 16000 {
            return Err(error("GPU 音频长度无效"));
        }
        let mut bytes = vec![0u8; count * 4];
        input
            .read_exact(&mut bytes)
            .map_err(|_| error("GPU 音频不完整"))?;
        let audio: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        validate_audio(&audio)?;
        let result = engine.transcribe(&audio)?;
        publish(
            serde_json::json!({"text":result.text,"event":result.event,"tokens":result.tokens,"timestamps":result.timestamps}),
        )?;
    }
}
pub fn worker_main(directory: &Path, language: &str) -> i32 {
    match worker(directory, language) {
        Ok(()) => 0,
        Err(e) => {
            let _ = publish(serde_json::json!({"error":e.message}));
            1
        }
    }
}
fn probe_cuda(directory: &Path) -> AppResult<String> {
    #[cfg(windows)]
    {
        use std::{
            ffi::{c_char, CStr},
            os::windows::ffi::OsStrExt,
        };
        extern "C" {
            fn asmr_sv_cuda_probe(
                runtime: *const u16,
                message: *mut c_char,
                capacity: usize,
            ) -> i32;
        }
        let directory: Vec<u16> = directory.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut message = [0 as c_char; 2048];
        let result =
            unsafe { asmr_sv_cuda_probe(directory.as_ptr(), message.as_mut_ptr(), message.len()) };
        let detail = unsafe { CStr::from_ptr(message.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        if result != 0 {
            return Err(error(format!(
                "CUDA 初始化失败，请检查 NVIDIA 驱动和 GPU 运行库：{detail}"
            )));
        }
        Ok(detail)
    }
    #[cfg(not(windows))]
    {
        let _ = directory;
        Err(error("GPU 识别仅支持 Windows NVIDIA CUDA"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t1513_gpu_pcm_bounds_reject_nan_and_oversized_requests() {
        assert!(validate_audio(&[]).is_err());
        assert!(validate_audio(&[f32::NAN]).is_err());
        assert!(validate_audio(&vec![0.0; 320001]).is_err());
        assert!(validate_audio(&vec![0.0; 16000]).is_ok());
        assert!(worker(Path::new("unused"), "invalid").is_err());
    }
}
