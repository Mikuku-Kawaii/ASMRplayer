//! A private Python/CTranslate2 process per job; models stay loaded across chunks.
use super::speech_segments::RecognitionResult;
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
fn error(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::MediaDecodeFailed, message.into())
}
pub struct FasterWhisper {
    _worker: tempfile::NamedTempFile,
    child: Child,
    input: ChildStdin,
    output: mpsc::Receiver<String>,
    language: String,
    prompt: String,
    pub device: String,
}
impl FasterWhisper {
    pub fn new(
        directory: &Path,
        cuda: &Path,
        language: &str,
        prompt: &str,
        cancel: &AtomicBool,
    ) -> AppResult<Self> {
        // Use the worker shipped with this build even for already-installed models.
        // Never overwrite a runtime used by another task or running player.
        let mut worker = tempfile::Builder::new().suffix(".py").tempfile()?;
        worker.write_all(include_bytes!("../../native/faster-whisper/worker.py"))?;
        worker.flush()?;
        let mut command = Command::new(directory.join("python.exe"));
        command
            .arg("-I")
            .arg("-u")
            .arg(worker.path())
            .arg(cuda)
            .arg(directory)
            .current_dir(directory)
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
            .map_err(|_| error("无法启动 Faster-Whisper，请在设置中重新安装模型运行库"))?;
        let input = child.stdin.take().expect("piped input");
        let stdout = child.stdout.take().expect("piped output");
        let (tx, rx) = mpsc::sync_channel(2);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                if reader
                    .by_ref()
                    .take(2 * 1024 * 1024)
                    .read_line(&mut line)
                    .unwrap_or(0)
                    == 0
                    || !line.ends_with('\n')
                {
                    break;
                }
                if let Some(value) = line.strip_prefix("ASMR_CT2:") {
                    if tx.send(value.trim().to_owned()).is_err() {
                        break;
                    }
                }
            }
        });
        let mut worker = Self {
            _worker: worker,
            child,
            input,
            output: rx,
            language: language.into(),
            prompt: prompt.into(),
            device: String::new(),
        };
        let ready = worker.receive(cancel)?;
        worker.device = ready["device"]
            .as_str()
            .ok_or_else(|| error("识别运行库未报告就绪"))?
            .into();
        Ok(worker)
    }
    pub fn set_language(&mut self, language: &str) -> AppResult<()> {
        if !["auto", "ja", "zh", "en", "ko"].contains(&language) {
            return Err(error("Whisper 语言选项无效"));
        }
        self.language = language.into();
        Ok(())
    }
    fn receive(&mut self, cancel: &AtomicBool) -> AppResult<serde_json::Value> {
        let deadline = Instant::now() + Duration::from_secs(300);
        loop {
            if cancel.load(Ordering::SeqCst) {
                let _ = self.child.kill();
                return Err(error("已取消识别"));
            }
            match self.output.recv_timeout(Duration::from_millis(100)) {
                Ok(line) => {
                    let value: serde_json::Value =
                        serde_json::from_str(&line).map_err(|_| error("识别进程响应无效"))?;
                    if let Some(message) = value["error"].as_str() {
                        return Err(error(message));
                    }
                    return Ok(value);
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(error(
                        "Faster-Whisper 进程已退出，已完成字幕保留；请检查模型运行库",
                    ))
                }
                Err(_) if Instant::now() >= deadline => {
                    let _ = self.child.kill();
                    return Err(error("Faster-Whisper 识别超时，已完成字幕保留"));
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
        serde_json::from_value(self.request(samples, cancel)?)
            .map_err(|_| error("识别字幕格式无效"))
    }
    pub fn transcribe_comparison(
        &mut self,
        samples: &[f32],
        cancel: &AtomicBool,
        without_prompt: bool,
    ) -> AppResult<RecognitionResult> {
        let saved = if without_prompt {
            Some(std::mem::take(&mut self.prompt))
        } else {
            None
        };
        let result = self.request_with_activity(samples, cancel, true).and_then(|value|
            serde_json::from_value(value).map_err(|_| error("识别字幕格式无效")));
        if let Some(prompt) = saved {
            self.prompt = prompt;
        }
        result
    }
    pub fn transcribe_phrases(
        &mut self,
        samples: &[f32],
        start_ms: i64,
        cancel: &AtomicBool,
    ) -> AppResult<Vec<(i64, i64, String)>> {
        segment_phrases(
            self.request(samples, cancel)?,
            start_ms,
            samples.len() as i64 / 16,
        )
    }
    fn request(&mut self, samples: &[f32], cancel: &AtomicBool) -> AppResult<serde_json::Value> {
        self.request_with_activity(samples, cancel, false)
    }
    fn request_with_activity(&mut self, samples: &[f32], cancel: &AtomicBool, speech_activity: bool) -> AppResult<serde_json::Value> {
        if samples.is_empty() || samples.len() > 320000 || samples.iter().any(|v| !v.is_finite()) {
            return Err(error("识别音频片段无效"));
        }
        if cancel.load(Ordering::SeqCst) {
            return Err(error("已取消识别"));
        }
        let header = serde_json::json!({"count":samples.len(),"language":self.language,"prompt":self.prompt,"speech_activity":speech_activity});
        let mut bytes = format!("{header}\n").into_bytes();
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        self.input
            .write_all(&bytes)
            .and_then(|_| self.input.flush())
            .map_err(|_| error("识别输入管道已断开"))?;
        self.receive(cancel)
    }
}
fn segment_phrases(
    value: serde_json::Value,
    offset: i64,
    duration: i64,
) -> AppResult<Vec<(i64, i64, String)>> {
    #[derive(serde::Deserialize)]
    struct Segment {
        start: f64,
        end: f64,
        text: String,
    }
    let segments: Vec<Segment> = serde_json::from_value(value["segments"].clone())
        .map_err(|_| error("识别字幕时间格式无效"))?;
    let mut cues = Vec::new();
    let mut previous_end = 0;
    for segment in segments {
        if !segment.start.is_finite() || !segment.end.is_finite() || segment.end < segment.start {
            return Err(error("识别字幕时间无效"));
        }
        let start = ((segment.start * 1000.0).round() as i64).clamp(previous_end, duration);
        let end = ((segment.end * 1000.0).round() as i64).clamp(start, duration);
        if end > start {
            cues.extend(super::speech_engine::short_phrases(
                &segment.text,
                offset + start,
                offset + end,
            ));
            previous_end = end;
        }
    }
    Ok(cues)
}
impl Drop for FasterWhisper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t1513_ct2_segment_times_stay_in_owned_audio() {
        let value = serde_json::json!({"segments":[
            {"start":0.2,"end":1.4,"text":"こんにちは"},
            {"start":1.2,"end":3.0,"text":"こんばんは"}]});
        let cues = segment_phrases(value, 7000, 2000).unwrap();
        assert_eq!(
            cues,
            vec![
                (7200, 8400, "こんにちは".into()),
                (8400, 9000, "こんばんは".into())
            ]
        );
        assert!(segment_phrases(
            serde_json::json!({"segments":[{"start":2.0,"end":1.0,"text":"a"}]}),
            0,
            3000
        )
        .is_err());
    }
    #[test]
    #[ignore = "requires an explicitly prepared isolated model runtime"]
    fn t1513_ct2_real_worker_protocol_and_cancel() {
        let root = std::path::PathBuf::from(
            std::env::var_os("ASMR_CT2_TEST_RUNTIME").expect("test runtime"),
        );
        let cancel = AtomicBool::new(false);
        let mut engine =
            FasterWhisper::new(&root, &root.join("no-cuda"), "ja", "", &cancel).unwrap();
        assert_eq!(engine.device, "CPU · INT8");
        for _ in 0..2 {
            assert!(engine
                .transcribe(&vec![0.0; 16000], &cancel)
                .unwrap()
                .text
                .is_empty());
        }
        assert!(engine.set_language("yue").is_err());
        engine.set_language("ja").unwrap();
        let tone: Vec<f32> = (0..16000).map(|i| (i as f32 * 0.1).sin() * 0.002).collect();
        let started = Instant::now();
        let evidence = engine.transcribe(&tone, &cancel).unwrap();
        assert!(evidence.speech_activity.is_none());
        let diagnostic = engine.transcribe_comparison(&tone, &cancel, true).unwrap();
        let activity = diagnostic.speech_activity.expect("installed Silero diagnostic");
        assert_eq!(activity.model, "silero-vad");
        assert_eq!(activity.frames, 32);
        assert!((0.0..=1.0).contains(&activity.max_probability));
        assert!(activity.mean_probability <= activity.max_probability);
        assert_eq!(evidence.config["condition_on_previous_text"], false);
        assert_eq!(evidence.config["vad_filter"], false);
        assert_eq!(evidence.config["word_timestamps"], false);
        assert_eq!(evidence.config["log_prob_threshold"], -1.0);
        assert!(evidence.config["no_speech_threshold"].is_null());
        assert_eq!(
            evidence.config["temperature"],
            serde_json::json!([0.0, 0.2, 0.4])
        );
        assert!(evidence.segments.iter().all(|s| s.avg_logprob.is_some()
            && s.no_speech_prob.is_some()
            && s.compression_ratio.is_some()));
        eprintln!("ASR synthetic tone: {} decoder segments, inference {:?}; no speech accuracy measurement", evidence.segments.len(), started.elapsed());
        let cues = segment_phrases(serde_json::to_value(evidence).unwrap(), 7000, 1000).unwrap();
        assert!(cues
            .iter()
            .all(|(start, end, _)| *start >= 7000 && *end <= 8000 && end > start));
        cancel.store(true, Ordering::SeqCst);
        assert!(engine
            .transcribe(&vec![0.0; 16000], &cancel)
            .unwrap_err()
            .message
            .contains("取消"));
    }
}
