//! T1513: opt-in model management. Construction and snapshots never access the network.
use crate::{
    db::Database,
    domain::{AppError, AppResult, ErrorCode},
};
use futures_util::StreamExt;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::watch,
};

const SETTINGS_KEY: &str = "recognition_translation_settings";
#[path = "whisper_assets.rs"]
mod whisper_assets;
#[path = "faster_whisper_assets.rs"]
mod faster_whisper_assets;
#[path = "model_download.rs"]
mod model_download;
#[path = "sensevoice_fp16_assets.rs"]
mod sensevoice_fp16_assets;
#[path = "ollama_gguf.rs"]
mod ollama_gguf;
#[path = "local_translation.rs"]
mod local_translation;
#[path = "deepseek_translation.rs"]
mod deepseek_translation;
#[path = "full_text_translation.rs"]
pub(super) mod full_text_translation;
#[path = "model_management.rs"]
mod model_management;
pub(super) use local_translation::{TranslationLine, LOCAL_BATCH_CHARS, LOCAL_BATCH_SIZE};
const MODEL_BASE: &str = "https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/2365baeacb507f821a0c8120fcee3d484dba7a07";
const FILES: [(&str, u64, &str); 2] = [
    (
        "model.int8.onnx",
        239233841,
        "c71f0ce00bec95b07744e116345e33d8cbbe08cef896382cf907bf4b51a2cd51",
    ),
    (
        "tokens.txt",
        315894,
        "f449eb28dc567533d7fa59be34e2abca8784f771850c78a47fb731a31429a1dc",
    ),
];

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AutoRecognitionMode {
    #[default]
    Off,
    Missing,
    All,
}
fn deserialize_auto_recognition<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<AutoRecognitionMode, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Stored { Mode(AutoRecognitionMode), Legacy(bool) }
    Ok(match Stored::deserialize(deserializer)? {
        Stored::Mode(mode) => mode,
        Stored::Legacy(true) => AutoRecognitionMode::Missing,
        Stored::Legacy(false) => AutoRecognitionMode::Off,
    })
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct RecognitionSettings {
    #[serde(deserialize_with = "deserialize_auto_recognition")]
    pub auto_recognition: AutoRecognitionMode,
    pub auto_translation: bool,
    pub pre_read_seconds: u32,
    pub translation_provider: String,
    pub ollama_url: String,
    pub ollama_model: String,
    pub deepseek_model: String,
    pub deepseek_prompt: String,
    pub full_text_review: bool,
    pub chinese_reference: bool,
    pub translation_options: TranslationOptions,
    pub preferences: super::subtitle_preferences::SubtitlePreferences,
}
impl Default for RecognitionSettings {
    fn default() -> Self {
        Self {
            auto_recognition: AutoRecognitionMode::Off,
            auto_translation: false,
            pre_read_seconds: 15,
            translation_provider: "ollama".into(),
            ollama_url: "http://127.0.0.1:11434".into(),
            ollama_model: "qwen3:4b".into(),
            deepseek_model: String::new(),
            deepseek_prompt: String::new(),
            full_text_review: false,
            chinese_reference: false,
            translation_options: Default::default(),
            preferences: Default::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct TranslationOptions {
    pub ollama_thinking: String,
    pub deepseek_thinking: String,
    pub temperature: f64,
    pub max_tokens: u32,
}
impl Default for TranslationOptions {
    fn default() -> Self { Self { ollama_thinking: "off".into(), deepseek_thinking: "default".into(), temperature: 0.2, max_tokens: 2048 } }
}
impl TranslationOptions {
    pub fn ollama_think(&self) -> serde_json::Value {
        match self.ollama_thinking.as_str() {
            "off" => serde_json::json!(false), "on" => serde_json::json!(true),
            level => serde_json::json!(level),
        }
    }
    fn deepseek_body(&self, model: &str, messages: serde_json::Value) -> serde_json::Value {
        let mut body = serde_json::json!({"model":model,"messages":messages,"stream":false,"response_format":{"type":"json_object"},"max_tokens":self.max_tokens});
        match self.deepseek_thinking.as_str() {
            "on" => body["thinking"] = serde_json::json!({"type":"enabled"}),
            "off" => { body["thinking"] = serde_json::json!({"type":"disabled"}); body["temperature"] = serde_json::json!(self.temperature); }
            _ => {} // Provider/model default may think; don't send ineffective sampling controls.
        }
        body
    }
}

#[derive(Clone, Default, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelJob {
    pub state: String,
    pub label: String,
    pub completed: u64,
    pub total: Option<u64>,
    pub error: Option<String>,
    pub alternative_available: bool,
    pub installed_model: Option<String>,
}
#[derive(Default)]
struct JobSlot {
    job: ModelJob,
    cancel: Option<watch::Sender<bool>>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecognitionSnapshot {
    pub model_directories: std::collections::BTreeMap<String, String>,
    pub ollama_directory_hint: String,
    pub sensevoice_fp16_installed: bool,
    pub sensevoice_fp16_bytes: u64,
    pub whisper_installed: bool,
    pub whisper_bytes: u64,
    pub faster_whisper_installed: bool,
    pub faster_whisper_bytes: u64,
    pub kotoba_whisper_installed: bool,
    pub kotoba_whisper_bytes: u64,
    pub runtime_installed: bool,
    pub runtime_bytes: u64,
    pub settings: RecognitionSettings,
    pub has_deepseek_key: bool,
    pub recognition_installed: bool,
    pub recognition_bytes: u64,
    pub recognition_directory: String,
    pub recognition_job: ModelJob,
    pub translation_job: ModelJob,
    pub ollama_install_job: ModelJob,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AvailableModel {
    pub name: String,
    pub size: Option<u64>,
}

#[derive(Clone)]
pub struct RecognitionService {
    pub(super) model_access: Arc<tokio::sync::RwLock<()>>,
    db: Database,
    directory: PathBuf,
    credential_service: String,
    recognition: Arc<Mutex<JobSlot>>,
    translation: Arc<Mutex<JobSlot>>,
    pub ollama_installer: super::ollama_install::OllamaInstaller,
}

fn invalid(message: &str) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message)
}
fn io_error(_: std::io::Error) -> AppError {
    AppError::new(
        ErrorCode::DiskOffline,
        "无法读写模型文件，请检查目录权限和剩余空间",
    )
}
fn network_error(_: reqwest::Error) -> AppError {
    AppError::new(
        ErrorCode::NetworkUnavailable,
        "模型服务连接失败或超时，请检查网络；使用本地翻译时请先启动 Ollama",
    )
}
fn ollama_network_error(error: reqwest::Error) -> AppError {
    AppError::new(
        ErrorCode::NetworkUnavailable,
        if error.is_connect() {
            "无法连接本机 Ollama。请点击「安装 Ollama」自动安装并启动服务；已安装时请检查 Ollama 地址和端口。"
        } else if error.is_timeout() {
            "Ollama 响应超时，请检查本地服务及其下载网络后重试；已下载的模型分层会保留。"
        } else {
            "Ollama 连接中断或响应无效，请检测本地服务后重试；已下载的模型分层会保留。"
        },
    )
}
fn ollama_download_error(detail: &str) -> AppError {
    // Classify server errors without exposing upstream URLs, credentials or local paths.
    let detail = detail.to_ascii_lowercase();
    let message = if detail.contains("redirect target not allowed")
        && detail.contains("non-public")
        && (detail.contains("198.18.") || detail.contains("198.19."))
    {
        "Ollama 下载被代理 DNS 的 Fake-IP 阻止。可使用「替代下载并导入」获取 Qwen3 4B，或导入已下载的本地 GGUF；无需修改 Clash 配置。"
    } else if detail.contains("redirect target not allowed") {
        "Ollama 拒绝了模型仓库的下载跳转。请检查代理或 DNS 是否将下载域名解析为非公网地址，修正解析后重试。"
    } else if detail.contains("no space")
        || detail.contains("disk full")
        || detail.contains("not enough space")
    {
        "Ollama 模型目录空间不足，请释放磁盘空间后重试下载。"
    } else if detail.contains("not found") || detail.contains("does not exist") {
        "Ollama 找不到此模型，请检查本地翻译模型名称（例如 qwen3:4b）后重试。"
    } else if detail.contains("version")
        || detail.contains("upgrade")
        || detail.contains("update ollama")
    {
        "此模型需要更新版 Ollama，请通过「安装 Ollama」更新后重试。"
    } else if [
        "timeout",
        "timed out",
        "connection",
        "dial tcp",
        "tls",
        "no such host",
        "network",
    ]
    .iter()
    .any(|s| detail.contains(s))
    {
        "Ollama 已连接，但无法从模型仓库下载。请检查 Ollama 的联网或代理设置，再重试下载。"
    } else {
        "Ollama 下载失败，请检查模型名称、磁盘空间及 Ollama 服务日志后重试。"
    };
    AppError::new(ErrorCode::NetworkUnavailable, message)
}
fn credential_error(_: keyring::Error) -> AppError {
    AppError::new(
        ErrorCode::AuthRequired,
        "无法访问 Windows 凭据库，请重试保存 API Key",
    )
}
fn validate_model(value: &str, allow_empty: bool) -> AppResult<()> {
    if (value.is_empty() && !allow_empty)
        || value.len() > 200
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.:/".contains(c))
    {
        return Err(invalid("模型名称无效"));
    }
    Ok(())
}
fn validate_settings(mut settings: RecognitionSettings) -> AppResult<RecognitionSettings> {
    let options = &settings.translation_options;
    if !["off", "on", "low", "medium", "high"].contains(&options.ollama_thinking.as_str())
        || !["default", "off", "on"].contains(&options.deepseek_thinking.as_str())
        || !options.temperature.is_finite() || !(0.0..=2.0).contains(&options.temperature)
        || !(512..=16384).contains(&options.max_tokens) {
        return Err(invalid("翻译参数无效：温度范围 0–2，输出长度范围 512–16384"));
    }
    if ![10, 15, 20].contains(&settings.pre_read_seconds) {
        return Err(invalid("识别预读目标请选择 10、15 或 20 秒"));
    }
    if !["ollama", "deepseek"].contains(&settings.translation_provider.as_str()) {
        return Err(invalid("请选择翻译服务"));
    }
    settings.preferences.validate()?;
    settings.ollama_url = settings.ollama_url.trim().trim_end_matches('/').into();
    let url = url::Url::parse(&settings.ollama_url).map_err(|_| invalid("Ollama 地址无效"))?;
    if !["http", "https"].contains(&url.scheme())
        || !matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(invalid(
            "Ollama 地址须为本机地址，例如 http://127.0.0.1:11434",
        ));
    }
    settings.ollama_model = settings.ollama_model.trim().into();
    settings.deepseek_model = settings.deepseek_model.trim().into();
    settings.deepseek_prompt = settings.deepseek_prompt.trim().into();
    if settings.deepseek_prompt.chars().count() > 4000 || settings.deepseek_prompt.contains('\0') {
        return Err(invalid("全局 Prompt 最多 4000 字，不能包含空字符"));
    }
    validate_model(&settings.ollama_model, false)?;
    validate_model(&settings.deepseek_model, true)?;
    Ok(settings)
}

impl RecognitionService {
    pub async fn translation_ready(&self) -> bool {
        let Ok(settings) = self.settings() else { return false; };
        self.prepare_translation_service(&settings).await.is_ok()
            && self.require_translation().await.is_ok()
    }
    pub async fn prepare_translation_service(&self, settings: &RecognitionSettings) -> AppResult<()> {
        let settings = validate_settings(settings.clone())?;
        if settings.translation_provider == "ollama" {
            self.ollama_installer.ensure_running(&settings.ollama_url).await?;
        }
        Ok(())
    }
    pub async fn require_translation(&self) -> AppResult<()> {
        let settings = self.settings()?;
        if settings.translation_provider == "deepseek" {
            if settings.deepseek_model.is_empty() { return Err(invalid("尚未选择 DeepSeek 模型，请在识别与翻译设置中保存模型配置")); }
            if self.key()?.is_none() { return Err(invalid("尚未保存 DeepSeek API Key，请在识别与翻译设置中配置")); }
            return Ok(());
        }
        let models = self.check_ollama(settings.ollama_url).await?;
        let canonical = |name: &str| name.trim().strip_suffix(":latest").unwrap_or(name.trim()).to_owned();
        if models.iter().any(|model| canonical(&model.name) == canonical(&settings.ollama_model)) {
            Ok(())
        } else {
            Err(invalid(&format!("Ollama 已连接，但当前保存的翻译模型「{}」未安装。请在设置中选择已安装模型并保存，或手动下载该模型", settings.ollama_model)))
        }
    }
    pub fn model_directory(&self) -> PathBuf {
        self.directory.clone()
    }
    pub fn runtime_directory(&self) -> PathBuf {
        self.db.data_dir.join("speech-runtime-1.12.20")
    }
    pub fn require_engine(&self) -> AppResult<()> {
        self.require_engine_for(&self.settings()?.preferences.recognition_engine)
    }
    pub fn require_engine_for(&self, engine: &str) -> AppResult<()> {
        if ["faster_whisper","kotoba_whisper"].contains(&engine) {
            return if self.ct2_installed(engine) { Ok(()) } else { Err(invalid("所选 Whisper 模型未安装完整，请在识别与翻译设置中下载模型与运行库")) };
        }
        if !["sensevoice", "sensevoice_fp16", "whisper"].contains(&engine) { return Err(invalid("未知识别引擎")); }
        if engine == "sensevoice_fp16" {
            return if self.sensevoice_fp16_installed() { Ok(()) } else {
                Err(invalid("SenseVoiceSmall FP16 与 GPU 运行库未安装，请在设置 → 识别与翻译中手动下载"))
            };
        }
        if engine == "whisper" {
            return if self.whisper_installed() { Ok(()) } else {
                Err(invalid("Whisper 尚未安装，请到设置 → 识别与翻译手动下载模型与运行库"))
            };
        }
        if !FILES.iter().all(|(name, size, _)| {
            self.directory
                .join(name)
                .metadata()
                .map(|m| m.len() == *size)
                .unwrap_or(false)
        }) {
            return Err(invalid(
                "识别模型未安装，请到设置 → 识别与翻译手动下载或导入",
            ));
        }
        if !self
            .runtime_directory()
            .join("sherpa-onnx-c-api.dll")
            .is_file()
        {
            return Err(invalid(
                "识别运行库未安装，请到设置 → 识别与翻译点击下载运行库",
            ));
        }
        Ok(())
    }
    pub fn start_runtime(&self) -> AppResult<()> {
        let mut cancel = Self::begin(&self.recognition, "下载识别运行库…".into())?;
        let service = self.clone();
        tokio::spawn(async move {
            let staging = service.runtime_directory().with_extension("part");
            let result = tokio::select! {
                biased;
                result = service.install_runtime(&staging) => Some(result),
                _ = cancel.changed() => None,
            };
            let _ = tokio::fs::remove_dir_all(staging).await;
            Self::finish(&service.recognition, result);
        });
        Ok(())
    }
    async fn install_runtime(&self, staging: &Path) -> AppResult<()> {
        if self.runtime_directory().exists() {
            return Err(invalid("识别运行库已安装"));
        }
        tokio::fs::create_dir_all(staging).await.map_err(io_error)?;
        let archive = staging.join("runtime.tar.bz2");
        let mut file = tokio::fs::File::create(&archive).await.map_err(io_error)?;
        let response = Self::checked(Self::download_client(false)?.get("https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.12.20/sherpa-onnx-v1.12.20-win-x64-shared-no-tts.tar.bz2").send().await.map_err(network_error)?).await?;
        let mut stream = response.bytes_stream();
        let mut hash = Sha256::new();
        let mut size = 0;
        self.recognition.lock().job.total = Some(23161059);
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(network_error)?;
            size += chunk.len() as u64;
            if size > 23161059 {
                return Err(invalid("运行库大小校验失败"));
            }
            hash.update(&chunk);
            file.write_all(&chunk).await.map_err(io_error)?;
            self.recognition.lock().job.completed = size;
        }
        verify_file(
            size,
            &hex::encode(hash.finalize()),
            23161059,
            "c10a095df39d10d48d97e22d9297ebd3bc12cca80ec3be70eb71f7a5b4a2f6c3",
        )?;
        file.sync_all().await.map_err(io_error)?;
        drop(file);
        self.recognition.lock().job.label = "解压识别运行库…".into();
        let tar =
            PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()))
                .join("System32/tar.exe");
        let mut command = tokio::process::Command::new(tar);
        command
            .args(["-xf"])
            .arg(&archive)
            .arg("-C")
            .arg(staging)
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let result = command.output().await.map_err(io_error)?;
        if !result.status.success() {
            return Err(invalid("识别运行库解压失败，请检查 Windows tar 与剩余空间"));
        }
        let lib = staging.join("sherpa-onnx-v1.12.20-win-x64-shared-no-tts/lib");
        let ready = staging.join("ready");
        tokio::fs::create_dir_all(&ready).await.map_err(io_error)?;
        for name in [
            "sherpa-onnx-c-api.dll",
            "onnxruntime.dll",
            "onnxruntime_providers_shared.dll",
            "cargs.dll",
        ] {
            tokio::fs::copy(lib.join(name), ready.join(name))
                .await
                .map_err(io_error)?;
        }
        tokio::fs::write(
            ready.join("LICENSE"),
            include_str!("../../native/sherpa/LICENSE"),
        )
        .await
        .map_err(io_error)?;
        tokio::fs::rename(ready, self.runtime_directory())
            .await
            .map_err(io_error)?;
        Ok(())
    }
    pub async fn translate_sentence(
        &self,
        settings: &RecognitionSettings,
        text: &str,
        context: &str,
    ) -> AppResult<String> {
        let settings = validate_settings(settings.clone())?;
        let system = settings.preferences.translation_instruction();
        let glossary: Vec<_> = settings
            .preferences
            .glossary
            .iter()
            .filter(|entry| entry.matches(text))
            .collect();
        let messages = serde_json::json!([{ "role":"system", "content":system }, { "role":"user", "content":serde_json::json!({"preferences":settings.deepseek_prompt,"context":context,"text":text,"glossary":glossary}).to_string() }]);
        let value: serde_json::Value = if settings.translation_provider == "deepseek" {
            if settings.deepseek_model.is_empty() {
                return Err(invalid(
                    "请先在识别与翻译设置中检测 DeepSeek 连接并保存模型",
                ));
            }
            let key = self
                .key()?
                .ok_or_else(|| invalid("请先保存 DeepSeek API Key"))?;
            let client = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(120))
                .build()
                .map_err(network_error)?;
            Self::checked(client.post("https://api.deepseek.com/chat/completions").bearer_auth(key)
                .json(&settings.translation_options.deepseek_body(&settings.deepseek_model, messages)).send().await.map_err(network_error)?).await?.json().await.map_err(network_error)?
        } else {
            // /api/chat never pulls absent models. The server returns an actionable error instead.
            Self::checked(Self::client(true)?.post(format!("{}/api/chat",settings.ollama_url)).timeout(Duration::from_secs(180))
                .json(&serde_json::json!({"model":settings.ollama_model,"messages":messages,"stream":false,"think":settings.translation_options.ollama_think(),"format":"json","options":{"temperature":settings.translation_options.temperature,"num_predict":settings.translation_options.max_tokens}})).send().await.map_err(network_error)?).await?.json().await.map_err(network_error)?
        };
        if value["done_reason"].as_str() == Some("length") || value["choices"][0]["finish_reason"].as_str() == Some("length") {
            return Err(invalid("译文输出达到长度上限，请提高输出长度后重试"));
        }
        let content = if settings.translation_provider == "deepseek" {
            value["choices"][0]["message"]["content"].as_str()
        } else {
            value["message"]["content"].as_str()
        };
        let translated =
            parse_translation(content.ok_or_else(|| invalid("翻译服务未返回正文，请重试"))?)?;
        if local_translation::is_untranslated_copy(text, &translated, &settings)
            || local_translation::fabricated_fragment_quote(text, &translated, &settings) {
            return Err(invalid("译文照搬了未确认的原文片段，请核对原文后重试"));
        }
        apply_translation_punctuation(translated, &settings.preferences.translation_punctuation)
    }
    pub fn new(db: Database, data_dir: PathBuf, identifier: &str) -> Self {
        Self {
            model_access: Default::default(),
            db,
            directory: data_dir.join("models").join("sensevoice-small-int8"),
            credential_service: format!("{identifier}.translation"),
            recognition: Default::default(),
            translation: Default::default(),
            ollama_installer: super::ollama_install::OllamaInstaller::new(data_dir.join("installers/ollama")),
        }
    }
    pub fn settings(&self) -> AppResult<RecognitionSettings> {
        self.db
            .get_setting(SETTINGS_KEY)?
            .map(|raw| serde_json::from_str(&raw).map_err(|_| invalid("识别与翻译设置无法读取")))
            .unwrap_or_else(|| Ok(RecognitionSettings::default()))
    }
    pub fn save_settings(&self, settings: RecognitionSettings) -> AppResult<RecognitionSettings> {
        let settings = validate_settings(settings)?;
        self.db.set_setting(
            SETTINGS_KEY,
            &serde_json::to_string(&settings).map_err(|_| invalid("无法保存设置"))?,
        )?;
        Ok(settings)
    }
    fn credential(&self) -> AppResult<keyring::Entry> {
        keyring::Entry::new(&self.credential_service, "deepseek-api-key").map_err(credential_error)
    }
    fn key(&self) -> AppResult<Option<String>> {
        match self.credential()?.get_password() {
            Ok(key) => Ok(Some(key)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(credential_error(e)),
        }
    }
    pub fn save_key(&self, key: String) -> AppResult<()> {
        let key = key.trim();
        if key.is_empty() || key.len() > 1024 || key.chars().any(char::is_whitespace) {
            return Err(invalid("请输入有效的 API Key"));
        }
        self.credential()?
            .set_password(key)
            .map_err(credential_error)
    }
    pub fn clear_key(&self) -> AppResult<()> {
        match self.credential()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(credential_error(e)),
        }
    }
    pub fn snapshot(&self) -> AppResult<RecognitionSnapshot> {
        Ok(RecognitionSnapshot {
            model_directories: self.model_directories(),
            ollama_directory_hint: Self::ollama_directory_hint(),
            sensevoice_fp16_installed: self.sensevoice_fp16_installed(),
            sensevoice_fp16_bytes: self.sensevoice_fp16_bytes(),
            whisper_installed: self.whisper_installed(),
            whisper_bytes: whisper_assets::MODEL_BYTES + whisper_assets::RUNTIME_BYTES,
            faster_whisper_installed: self.ct2_installed("faster_whisper"),
            faster_whisper_bytes: self.ct2_bytes("faster_whisper"),
            kotoba_whisper_installed: self.ct2_installed("kotoba_whisper"),
            kotoba_whisper_bytes: self.ct2_bytes("kotoba_whisper"),
            runtime_installed: self
                .runtime_directory()
                .join("sherpa-onnx-c-api.dll")
                .is_file(),
            runtime_bytes: 23161059,
            settings: self.settings()?,
            has_deepseek_key: self.key()?.is_some(),
            recognition_installed: FILES.iter().all(|(name, size, _)| {
                self.directory
                    .join(name)
                    .metadata()
                    .map(|m| m.is_file() && m.len() == *size)
                    .unwrap_or(false)
            }),
            recognition_bytes: FILES.iter().map(|(_, size, _)| size).sum(),
            recognition_directory: self.directory.to_string_lossy().into(),
            recognition_job: self.recognition.lock().job.clone(),
            translation_job: self.translation.lock().job.clone(),
            ollama_install_job: self.ollama_installer.snapshot(),
        })
    }
    fn client(local: bool) -> AppResult<reqwest::Client> {
        Self::client_with_read_timeout(local, Duration::from_secs(120))
    }
    fn download_client(local: bool) -> AppResult<reqwest::Client> {
        // No total deadline: progress can continue for hours. Ollama may spend
        // longer verifying a layer without sending another progress message.
        Self::client_with_read_timeout(local, Duration::from_secs(if local { 1800 } else { 600 }))
    }
    fn client_with_read_timeout(local: bool, idle: Duration) -> AppResult<reqwest::Client> {
        let mut builder = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(idle)
            .redirect(if local {
                reqwest::redirect::Policy::none()
            } else {
                reqwest::redirect::Policy::limited(5)
            });
        if local {
            builder = builder.no_proxy();
        }
        builder.build().map_err(network_error)
    }
    async fn checked(response: reqwest::Response) -> AppResult<reqwest::Response> {
        if response.status().is_success() {
            return Ok(response);
        }
        let code = match response.status().as_u16() {
            401 | 403 => ErrorCode::AuthRequired,
            429 => ErrorCode::RateLimited,
            _ => ErrorCode::NetworkUnavailable,
        };
        Err(AppError::new(
            code,
            format!(
                "模型服务返回 HTTP {}，请检查凭据、模型名称或服务状态",
                response.status().as_u16()
            ),
        ))
    }
    pub async fn check_ollama(&self, base: String) -> AppResult<Vec<AvailableModel>> {
        let settings = validate_settings(RecognitionSettings {
            ollama_url: base,
            ..Default::default()
        })?;
        let response = Self::checked(
            Self::client(true)?
                .get(format!("{}/api/tags", settings.ollama_url))
                .timeout(Duration::from_secs(15))
                .send()
                .await
                .map_err(ollama_network_error)?,
        )
        .await?;
        #[derive(Deserialize)]
        struct Tags {
            models: Vec<AvailableModel>,
        }
        Ok(response
            .json::<Tags>()
            .await
            .map_err(ollama_network_error)?
            .models)
    }
    pub async fn check_deepseek(&self) -> AppResult<Vec<AvailableModel>> {
        let key = self
            .key()?
            .ok_or_else(|| AppError::new(ErrorCode::AuthRequired, "请先保存 DeepSeek API Key"))?;
        let response = Self::checked(
            reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(20))
                .build()
                .map_err(network_error)?
                .get("https://api.deepseek.com/models")
                .bearer_auth(key)
                .send()
                .await
                .map_err(network_error)?,
        )
        .await?;
        #[derive(Deserialize)]
        struct Model {
            id: String,
        }
        #[derive(Deserialize)]
        struct Models {
            data: Vec<Model>,
        }
        let models = response.json::<Models>().await.map_err(network_error)?.data;
        if models.is_empty() {
            return Err(invalid("DeepSeek 未返回可用模型"));
        }
        Ok(models
            .into_iter()
            .map(|m| AvailableModel {
                name: m.id,
                size: None,
            })
            .collect())
    }
    fn begin(slot: &Mutex<JobSlot>, label: String) -> AppResult<watch::Receiver<bool>> {
        let mut slot = slot.lock();
        if slot.cancel.is_some() {
            return Err(invalid("此类模型任务正在运行，请等待或取消"));
        }
        let (tx, rx) = watch::channel(false);
        slot.job = ModelJob {
            state: "running".into(),
            label,
            ..Default::default()
        };
        slot.cancel = Some(tx);
        Ok(rx)
    }
    pub fn cancel(&self, kind: &str) -> AppResult<()> {
        let slot = match kind {
            "recognition" => &self.recognition,
            "translation" => &self.translation,
            _ => return Err(invalid("未知模型任务")),
        };
        let mut slot = slot.lock();
        if let Some(tx) = &slot.cancel {
            let _ = tx.send(true);
            slot.job.state = "cancelling".into();
        }
        Ok(())
    }
    fn finish(slot: &Mutex<JobSlot>, result: Option<AppResult<()>>) {
        let mut slot = slot.lock();
        match result {
            Some(Ok(())) => {
                slot.job.state = "completed".into();
                slot.job.label = "模型已就绪".into();
                slot.job.alternative_available = false;
            }
            Some(Err(error)) => {
                slot.job.state = "failed".into();
                slot.job.alternative_available |= error.message.contains("Fake-IP");
                slot.job.error = Some(error.message);
            }
            None => {
                slot.job.state = "cancelled".into();
                slot.job.label = "已取消".into();
            }
        }
        slot.cancel = None;
    }
    pub fn start_recognition(&self, import_directory: Option<PathBuf>) -> AppResult<()> {
        let mut cancel = Self::begin(&self.recognition, "准备识别模型…".into())?;
        let service = self.clone();
        tokio::spawn(async move {
            let staging = service.directory.with_extension("part");
            let result = tokio::select! {
                biased;
                result = service.install_recognition(&staging, import_directory) => Some(result),
                _ = cancel.changed() => None,
            };
            // Only this service's staging directory is removed; imported originals remain intact.
            let _ = tokio::fs::remove_dir_all(&staging).await;
            Self::finish(&service.recognition, result);
        });
        Ok(())
    }
    async fn install_recognition(&self, staging: &Path, source: Option<PathBuf>) -> AppResult<()> {
        if self.directory.exists() {
            return Err(invalid("识别模型目录已存在；请使用已有模型"));
        }
        tokio::fs::create_dir_all(staging).await.map_err(io_error)?;
        let total: u64 = FILES.iter().map(|(_, size, _)| size).sum();
        self.recognition.lock().job.total = Some(total);
        let client = Self::download_client(false)?;
        for (name, size, expected) in FILES {
            self.recognition.lock().job.label =
                format!("{} {name}", if source.is_some() { "导入" } else { "下载" });
            let mut output = tokio::fs::File::create(staging.join(name))
                .await
                .map_err(io_error)?;
            let mut hash = Sha256::new();
            let mut written = 0;
            if let Some(ref source) = source {
                let mut input = tokio::fs::File::open(source.join(name))
                    .await
                    .map_err(io_error)?;
                let mut buffer = vec![0u8; 128 * 1024];
                loop {
                    let count = input.read(&mut buffer).await.map_err(io_error)?;
                    if count == 0 {
                        break;
                    }
                    written += count as u64;
                    if written > size {
                        return Err(invalid("导入文件与 SenseVoiceSmall INT8 版本不匹配"));
                    }
                    hash.update(&buffer[..count]);
                    output.write_all(&buffer[..count]).await.map_err(io_error)?;
                    self.recognition.lock().job.completed += count as u64;
                }
            } else {
                let response = Self::checked(
                    client
                        .get(format!("{MODEL_BASE}/{name}"))
                        .send()
                        .await
                        .map_err(network_error)?,
                )
                .await?;
                let mut stream = response.bytes_stream();
                while let Some(chunk) = stream.next().await {
                    let chunk = chunk.map_err(network_error)?;
                    written += chunk.len() as u64;
                    if written > size {
                        return Err(invalid("模型文件大小与发布版本不匹配"));
                    }
                    hash.update(&chunk);
                    output.write_all(&chunk).await.map_err(io_error)?;
                    self.recognition.lock().job.completed += chunk.len() as u64;
                }
            }
            self.recognition.lock().job.label = "校验模型文件…".into();
            verify_file(written, &hex::encode(hash.finalize()), size, expected)?;
            output.sync_all().await.map_err(io_error)?;
        }
        tokio::fs::rename(staging, &self.directory)
            .await
            .map_err(io_error)?;
        Ok(())
    }
    pub fn start_translation(&self) -> AppResult<()> {
        let settings = validate_settings(self.settings()?)?;
        let mut cancel = Self::begin(&self.translation, format!("下载 {}", settings.ollama_model))?;
        let service = self.clone();
        tokio::spawn(async move {
            let result = tokio::select! {
                biased;
                result = service.pull_translation(&settings) => Some(result),
                _ = cancel.changed() => None,
            };
            Self::finish(&service.translation, result);
        });
        Ok(())
    }
    async fn pull_translation(&self, settings: &RecognitionSettings) -> AppResult<()> {
        self.translation.lock().job.label = "检测 Ollama 服务…".into();
        self.check_ollama(settings.ollama_url.clone()).await?;
        self.translation.lock().job.label = "获取模型信息…".into();
        let response = Self::download_client(true)?
            .post(format!("{}/api/pull", settings.ollama_url))
            .json(&serde_json::json!({"model": settings.ollama_model, "stream": true}))
            .send()
            .await
            .map_err(ollama_network_error)?;
        if !response.status().is_success() {
            let value: serde_json::Value = response.json().await.unwrap_or_default();
            return Err(ollama_download_error(
                value["error"].as_str().unwrap_or_default(),
            ));
        }
        let mut stream = response.bytes_stream();
        let mut pending = Vec::new();
        let mut success = false;
        while let Some(chunk) = stream.next().await {
            pending.extend_from_slice(&chunk.map_err(ollama_network_error)?);
            while let Some(end) = pending.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = pending.drain(..=end).collect();
                if !line.iter().all(u8::is_ascii_whitespace) {
                    success |= self.pull_progress(&line)?;
                }
            }
            if pending.len() > 1024 * 1024 {
                return Err(invalid("Ollama 下载响应过长"));
            }
        }
        if !pending.iter().all(u8::is_ascii_whitespace) {
            success |= self.pull_progress(&pending)?;
        }
        if !success {
            return Err(invalid("Ollama 下载中断，请重试以继续下载"));
        }
        let installed = self.check_ollama(settings.ollama_url.clone()).await?;
        let name = if settings.ollama_model.contains(':') {
            settings.ollama_model.clone()
        } else {
            format!("{}:latest", settings.ollama_model)
        };
        if !installed.iter().any(|m| m.name == name) {
            return Err(invalid(
                "下载响应已结束，但 Ollama 尚未列出所选模型，请刷新后重试",
            ));
        }
        self.translation.lock().job.installed_model = Some(name);
        Ok(())
    }
    fn pull_progress(&self, line: &[u8]) -> AppResult<bool> {
        let value: serde_json::Value =
            serde_json::from_slice(line).map_err(|_| invalid("Ollama 下载响应无效"))?;
        if let Some(detail) = value.get("error") {
            return Err(ollama_download_error(detail.as_str().unwrap_or_default()));
        }
        let status = value["status"]
            .as_str()
            .ok_or_else(|| invalid("Ollama 未返回下载状态"))?;
        let mut slot = self.translation.lock();
        slot.job.label = match status {
            "success" => "下载完成",
            "verifying sha256 digest" => "校验模型…",
            "writing manifest" => "保存模型…",
            "pulling manifest" => "获取模型信息…",
            _ => "下载模型分层…",
        }
        .into();
        slot.job.completed = value["completed"].as_u64().unwrap_or(0);
        slot.job.total = value["total"].as_u64().filter(|n| *n > 0);
        Ok(status == "success")
    }
}

fn verify_file(actual_size: u64, actual_hash: &str, size: u64, hash: &str) -> AppResult<()> {
    if actual_size != size || actual_hash != hash {
        return Err(invalid("模型文件校验失败，请重新下载或导入指定版本"));
    }
    Ok(())
}

fn apply_translation_punctuation(text: String, punctuation: &str) -> AppResult<String> {
    if punctuation != "minimal" {
        return Ok(text);
    }
    // The uncertainty marker carries review state; it is not stylistic punctuation.
    let text = text.split("〔原文不清〕").map(|part| part.chars().map(|c| {
        if super::speech_engine::is_punctuation(c) { ' ' } else { c }
    }).collect::<String>()).collect::<Vec<_>>().join("〔原文不清〕");
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return Err(invalid("翻译服务只返回了标点，请重试"));
    }
    Ok(text)
}

#[test]
fn uncertainty_marker_survives_minimal_punctuation() {
    assert_eq!(apply_translation_punctuation("不会，〔原文不清〕。".into(), "minimal").unwrap(), "不会 〔原文不清〕");
}

fn validate_translation_text(text: &str) -> AppResult<String> {
    let text = text.trim();
    if text.is_empty() || text.len() > 16384 || text.contains('\0') {
        return Err(invalid("译文为空或过长，请重试"));
    }
    Ok(text.to_owned())
}

fn parse_translation(content: &str) -> AppResult<String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Translation {
        translation: String,
    }
    let result: Translation = serde_json::from_str(content)
        .map_err(|_| invalid("译文格式不正确，请重试；已完成的句子会保留"))?;
    validate_translation_text(&result.translation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{body_json, method, path},
        Mock, MockServer, ResponseTemplate,
    };
    #[test]
    fn t1513_translation_options_defaults_validation_and_provider_payloads() {
        let old: RecognitionSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(old.translation_options, TranslationOptions::default());
        assert_eq!(old.translation_options.ollama_think(), serde_json::json!(false));
        let mut settings = old;
        for level in ["off", "on", "low", "medium", "high"] {
            settings.translation_options.ollama_thinking = level.into();
            assert!(validate_settings(settings.clone()).is_ok());
        }
        assert_eq!(settings.translation_options.ollama_think(), serde_json::json!("high"));
        let options = &mut settings.translation_options;
        options.deepseek_thinking = "on".into(); options.temperature = 0.8; options.max_tokens = 8192;
        let body = options.deepseek_body("selected-model", serde_json::json!([]));
        assert_eq!(body["thinking"]["type"], "enabled"); assert_eq!(body["max_tokens"], 8192);
        assert!(body.get("temperature").is_none());
        options.deepseek_thinking = "off".into();
        assert_eq!(options.deepseek_body("selected-model", serde_json::json!([]))["temperature"], 0.8);
        options.max_tokens = 0; assert!(validate_settings(settings.clone()).is_err());
        settings.translation_options.max_tokens = 2048;
        settings.translation_options.temperature = f64::NAN;
        assert!(validate_settings(settings).is_err());
    }
    #[test]
    fn t1513_automatic_recognition_modes_migrate_legacy_settings() {
        for (stored, expected) in [("false", AutoRecognitionMode::Off), ("true", AutoRecognitionMode::Missing), ("\"off\"", AutoRecognitionMode::Off), ("\"missing\"", AutoRecognitionMode::Missing), ("\"all\"", AutoRecognitionMode::All)] {
            let settings: RecognitionSettings = serde_json::from_str(&format!("{{\"autoRecognition\":{stored}}}")).unwrap();
            assert_eq!(settings.auto_recognition, expected);
            let serialized = serde_json::to_string(&settings).unwrap();
            assert_eq!(serde_json::from_str::<RecognitionSettings>(&serialized).unwrap(), settings);
        }
        assert_eq!(serde_json::from_str::<RecognitionSettings>("{}").unwrap().auto_recognition, AutoRecognitionMode::Off);
        assert_eq!(serde_json::from_str::<RecognitionSettings>("{}").unwrap().preferences.language, "ja");
        assert_eq!(serde_json::from_str::<RecognitionSettings>(r#"{"preferences":{"language":"auto"}}"#).unwrap().preferences.language, "auto");
        assert!(serde_json::from_str::<RecognitionSettings>(r#"{"autoRecognition":"unknown"}"#).is_err());
    }
    #[test]
    fn t1513_prefetch_settings_migrate_default_and_validate_supported_targets() {
        let old: RecognitionSettings = serde_json::from_str(r#"{"autoRecognition":true}"#).unwrap();
        assert_eq!(old.pre_read_seconds, 15);
        for target in [10, 15, 20] {
            assert!(validate_settings(RecognitionSettings {
                pre_read_seconds: target,
                ..old.clone()
            })
            .is_ok());
        }
        for target in [0, 9, 14, 21, u32::MAX] {
            assert!(validate_settings(RecognitionSettings {
                pre_read_seconds: target,
                ..old.clone()
            })
            .is_err());
        }
    }

    #[tokio::test]
    async fn t1513_ollama_missing_service_explains_installation_without_pulling() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let (_root, service) = service();
        service
            .save_settings(RecognitionSettings {
                ollama_url: format!("http://{address}"),
                ..Default::default()
            })
            .unwrap();
        service.start_translation().unwrap();
        let job = wait_done(&service.translation).await;
        assert_eq!(job.state, "failed");
        assert!(job.error.unwrap().contains("安装 Ollama"));

        let server = MockServer::start().await;
        service
            .save_settings(RecognitionSettings {
                ollama_url: server.uri(),
                ..Default::default()
            })
            .unwrap();
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not Ollama"))
            .mount(&server)
            .await;
        service.start_translation().unwrap();
        assert_eq!(wait_done(&service.translation).await.state, "failed");
        assert!(server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|r| r.method == "GET"));
    }

    #[tokio::test]
    async fn t1513_ollama_http_and_stream_failures_show_actionable_categories() {
        for (status, detail, expected) in [
            (404, "pull model manifest: file does not exist", "模型名称"),
            (
                200,
                "write /private/path: no space left on device",
                "空间不足",
            ),
            (
                200,
                "dial tcp https://secret.example/?token=hidden: connection refused",
                "联网或代理",
            ),
            (400, "requires a newer version of Ollama", "更新版"),
            (200, "redirect target not allowed: private.r2.cloudflarestorage.com resolves to non-public 198.18.0.154", "Fake-IP"),
            (200, "redirect target not allowed: private.r2.cloudflarestorage.com resolves to non-public 198.19.1.2", "Fake-IP"),
            (200, "redirect target not allowed: secret.example resolves to non-public 127.0.0.1", "非公网地址"),
        ] {
            let server = MockServer::start().await;
            let (_root, service) = service();
            service
                .save_settings(RecognitionSettings {
                    ollama_url: server.uri(),
                    ..Default::default()
                })
                .unwrap();
            Mock::given(method("GET"))
                .and(path("/api/tags"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(serde_json::json!({"models":[]})),
                )
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .and(path("/api/pull"))
                .respond_with(
                    ResponseTemplate::new(status)
                        .set_body_json(serde_json::json!({"error":detail})),
                )
                .mount(&server)
                .await;
            service.start_translation().unwrap();
            let job = wait_done(&service.translation).await;
            assert_eq!(job.state, "failed");
            assert_eq!(job.alternative_available, expected == "Fake-IP");
            let message = job.error.unwrap();
            assert!(message.contains(expected), "{message}");
            assert!(
                !message.contains("private")
                    && !message.contains("secret")
                    && !message.contains("hidden")
            );
            service.start_translation().unwrap();
            assert!(service.translation.lock().job.error.is_none());
            assert_eq!(wait_done(&service.translation).await.state, "failed");
        }
    }

    #[test]
    fn t1513_translation_parser_rejects_incomplete_empty_or_extraneous_results() {
        assert_eq!(
            parse_translation("{\"translation\":\" 晚安 \"}").unwrap(),
            "晚安"
        );
        for invalid in [
            "",
            "{\"translation\":",
            "{\"translation\":\" \"}",
            "{\"translation\":\"晚安\",\"extra\":1}",
            "```json\n{\"translation\":\"晚安\"}\n```",
        ] {
            assert!(parse_translation(invalid).is_err());
        }
    }

    fn service() -> (tempfile::TempDir, RecognitionService) {
        let root = tempfile::tempdir().unwrap();
        let db = Database::open_and_migrate(&root.path().join("data"), &root.path().join("cache"))
            .unwrap();
        let service = RecognitionService::new(db, root.path().join("data"), "test-recognition");
        (root, service)
    }
    async fn wait_done(slot: &Mutex<JobSlot>) -> ModelJob {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if slot.lock().cancel.is_none() {
                    return slot.lock().job.clone();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap()
    }
    #[test]
    fn t1513_defaults_do_not_install_or_start_models_and_settings_survive_reopen() {
        let (root, service) = service();
        assert_eq!(service.settings().unwrap(), RecognitionSettings::default());
        assert!(!service.directory.exists());
        assert!(service.recognition.lock().cancel.is_none());
        assert!(service.translation.lock().cancel.is_none());
        let mut settings = RecognitionSettings {
            translation_provider: "deepseek".into(),
            deepseek_model: "test-model".into(),
            ..Default::default()
        };
        settings.preferences.language = "ja".into();
        settings.preferences.translation_style = "soft".into();
        settings
            .preferences
            .glossary
            .push(super::super::subtitle_preferences::Term {
                source: "先輩".into(),
                target: "前辈".into(), ..Default::default()
            });
        service.save_settings(settings.clone()).unwrap();
        drop(service);
        let db = Database::open_and_migrate(&root.path().join("data"), &root.path().join("cache"))
            .unwrap();
        let next = RecognitionService::new(db, root.path().join("data"), "test-recognition");
        assert_eq!(next.settings().unwrap(), settings);
        assert!(!next.directory.exists());
    }
    #[test]
    fn t1513_old_settings_and_documents_default_to_original_preferences() {
        let settings: RecognitionSettings =
            serde_json::from_str(r#"{"autoRecognition":true}"#).unwrap();
        assert_eq!(settings.preferences, Default::default());
        let doc:crate::db::subtitles::SubtitleDocument=serde_json::from_value(serde_json::json!({"versionId":"v","sourceKey":"external:old","original":[],"translations":{},"completedChunks":[],"recognitionComplete":true,"translationKey":""})).unwrap();
        assert!(doc.rule_corrections.is_empty() && !doc.rules_suppressed);
    }
    #[test]
    fn t1513_local_endpoint_and_settings_validation() {
        for url in [
            "https://example.com",
            "http://127.0.0.1@evil.test",
            "http://localhost/api",
            "http://localhost?key=private",
            "file:///tmp/model",
        ] {
            assert!(validate_settings(RecognitionSettings {
                ollama_url: url.into(),
                ..Default::default()
            })
            .is_err());
        }
        for url in [
            "http://127.0.0.1:11434",
            "http://localhost:11434/",
            "http://[::1]:11434",
        ] {
            assert!(validate_settings(RecognitionSettings {
                ollama_url: url.into(),
                ..Default::default()
            })
            .is_ok());
        }
        let (_root, service) = service();
        assert!(service
            .save_settings(RecognitionSettings {
                ollama_model: "".into(),
                ..Default::default()
            })
            .is_err());
        assert_eq!(service.settings().unwrap(), RecognitionSettings::default());
    }
    #[test]
    fn t1513_hash_and_size_both_required() {
        let hash = hex::encode(Sha256::digest(b"fixture"));
        assert!(verify_file(7, &hash, 7, &hash).is_ok());
        assert!(verify_file(6, &hash, 7, &hash).is_err());
        assert!(verify_file(7, "different", 7, &hash).is_err());
    }
    #[tokio::test]
    async fn t1513_bad_import_never_installs_or_changes_originals_and_can_retry() {
        let (root, service) = service();
        let source = root.path().join("originals");
        tokio::fs::create_dir(&source).await.unwrap();
        tokio::fs::write(source.join("model.int8.onnx"), b"not a model")
            .await
            .unwrap();
        service.start_recognition(Some(source.clone())).unwrap();
        assert!(service.start_recognition(Some(source.clone())).is_err());
        assert_eq!(wait_done(&service.recognition).await.state, "failed");
        assert!(!service.directory.exists());
        assert!(!service.directory.with_extension("part").exists());
        assert_eq!(
            tokio::fs::read(source.join("model.int8.onnx"))
                .await
                .unwrap(),
            b"not a model"
        );
        service.start_recognition(Some(source)).unwrap();
        assert_eq!(wait_done(&service.recognition).await.state, "failed");
    }
    #[tokio::test]
    async fn t1513_ollama_pull_requires_explicit_start_and_verified_listed_model() {
        let server = MockServer::start().await;
        let (_root, service) = service();
        let settings = RecognitionSettings {
            ollama_url: server.uri(),
            ..Default::default()
        };
        service.save_settings(settings.clone()).unwrap();
        assert!(server.received_requests().await.unwrap().is_empty());
        Mock::given(method("POST")).and(path("/api/pull")).and(body_json(serde_json::json!({"model":"qwen3:4b","stream":true})))
            .respond_with(ResponseTemplate::new(200).set_body_string("{\"status\":\"pulling manifest\"}\n{\"status\":\"pulling layer\",\"completed\":5,\"total\":10}\n{\"status\":\"success\"}"))
            .expect(1).mount(&server).await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"models":[{"name":"qwen3:4b","size":10}]})),
            )
            .mount(&server)
            .await;
        service.start_translation().unwrap();
        assert!(service.start_translation().is_err());
        let job = wait_done(&service.translation).await;
        assert_eq!(
            job.state,
            "completed",
            "{job:?}; requests: {:?}",
            server.received_requests().await
        );
        assert_eq!(
            service.check_ollama(settings.ollama_url).await.unwrap()[0].name,
            "qwen3:4b"
        );
        assert_eq!(job.installed_model.as_deref(), Some("qwen3:4b"));
    }
    #[tokio::test]
    async fn t1513_ollama_cancel_releases_task_and_retry_can_finish() {
        let server = MockServer::start().await;
        let (_root, service) = service();
        service
            .save_settings(RecognitionSettings {
                ollama_url: server.uri(),
                ..Default::default()
            })
            .unwrap();
        Mock::given(method("POST"))
            .and(path("/api/pull"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(60)))
            .mount(&server)
            .await;
        service.start_translation().unwrap();
        service.cancel("translation").unwrap();
        assert_eq!(wait_done(&service.translation).await.state, "cancelled");
        server.reset().await;
        Mock::given(method("POST"))
            .and(path("/api/pull"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{\"status\":\"success\"}\n"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"models":[{"name":"qwen3:4b"}]})),
            )
            .mount(&server)
            .await;
        service.start_translation().unwrap();
        assert_eq!(wait_done(&service.translation).await.state, "completed");
    }
    #[tokio::test]
    async fn t1513_ollama_truncated_error_and_unlisted_results_are_not_success() {
        for body in [
            "{\"status\":\"pulling layer\",\"completed\":2,\"total\":5}\n",
            "{\"error\":\"secret upstream detail\"}\n",
            "invalid json\n",
            "{\"status\":\"success\"}\n",
        ] {
            let server = MockServer::start().await;
            let (_root, service) = service();
            service
                .save_settings(RecognitionSettings {
                    ollama_url: server.uri(),
                    ..Default::default()
                })
                .unwrap();
            Mock::given(method("POST"))
                .and(path("/api/pull"))
                .respond_with(ResponseTemplate::new(200).set_body_string(body))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/api/tags"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(serde_json::json!({"models":[]})),
                )
                .mount(&server)
                .await;
            service.start_translation().unwrap();
            let job = wait_done(&service.translation).await;
            assert_eq!(job.state, "failed");
            assert!(!job.error.unwrap().contains("secret upstream detail"));
        }
    }
}
