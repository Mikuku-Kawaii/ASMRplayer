use super::*;
use std::collections::BTreeMap;

impl RecognitionService {
    fn model_path(&self, engine: &str) -> AppResult<PathBuf> {
        match engine {
            "sensevoice" => Ok(self.model_directory()),
            "sensevoice_fp16" => Ok(self.sensevoice_fp16_directory()),
            "whisper" => Ok(self.whisper_directory()),
            "faster_whisper" | "kotoba_whisper" => self.ct2_directory(engine),
            _ => Err(invalid("未知识别模型")),
        }
    }
    pub fn model_directories(&self) -> BTreeMap<String, String> {
        ["sensevoice", "sensevoice_fp16", "whisper", "faster_whisper", "kotoba_whisper"]
            .into_iter()
            .map(|id| {
                (
                    id.into(),
                    self.model_path(id).unwrap().to_string_lossy().into(),
                )
            })
            .collect()
    }
    pub fn ollama_directory_hint() -> String {
        let path = std::env::var_os("OLLAMA_MODELS")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join(".ollama/models"))
            });
        path.map(|p| {
            format!(
                "{}（本机环境推定；服务使用其他账户或环境时可能不同）",
                p.display()
            )
        })
        .unwrap_or_else(|| "由 Ollama 服务管理，当前无法读取路径".into())
    }
    pub fn delete_recognition_model(&self, engine: &str) -> AppResult<()> {
        let _exclusive = self
            .model_access
            .try_write()
            .map_err(|_| invalid("模型正在识别或翻译，请先取消任务再删除"))?;
        let mut slot = self.recognition.lock();
        if slot.cancel.is_some() {
            return Err(invalid("模型下载正在进行，请先取消下载"));
        }
        let target = self.model_path(engine)?;
        if target.exists() {
            let root = self
                .db
                .data_dir
                .join("models")
                .canonicalize()
                .map_err(io_error)?;
            let resolved = target.canonicalize().map_err(io_error)?;
            if resolved == root || !resolved.starts_with(&root) {
                return Err(invalid("模型路径不在应用模型目录内"));
            }
            // Refuse junctions/symlinks; never recursively follow a substituted user path.
            for entry in walkdir::WalkDir::new(&target).follow_links(false) {
                let entry = entry.map_err(|_| invalid("无法检查模型目录"))?;
                let metadata = std::fs::symlink_metadata(entry.path()).map_err(io_error)?;
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if metadata.file_attributes() & 0x400 != 0 {
                        return Err(invalid("模型目录包含链接，不能自动删除"));
                    }
                }
                if metadata.file_type().is_symlink() {
                    return Err(invalid("模型目录包含链接，不能自动删除"));
                }
            }
            std::fs::remove_dir_all(&target).map_err(io_error)?;
        }
        slot.job = ModelJob::default();
        Ok(())
    }
    pub async fn delete_ollama_model(&self, base: String, model: String) -> AppResult<()> {
        let _exclusive = self
            .model_access
            .try_write()
            .map_err(|_| invalid("模型正在识别或翻译，请先取消任务再删除"))?;
        let settings = validate_settings(RecognitionSettings {
            ollama_url: base,
            ollama_model: model,
            ..Default::default()
        })?;
        let _task = Self::begin(&self.translation, "删除本地翻译模型…".into())?;
        let result = async {
            let installed = self.check_ollama(settings.ollama_url.clone()).await?;
            let name = installed
                .iter()
                .find(|m| {
                    m.name == settings.ollama_model
                        || m.name == format!("{}:latest", settings.ollama_model)
                })
                .ok_or_else(|| invalid("该模型未安装，请刷新模型列表"))?
                .name
                .clone();
            Self::checked(
                Self::client(true)?
                    .delete(format!("{}/api/delete", settings.ollama_url))
                    .json(&serde_json::json!({"model":name}))
                    .send()
                    .await
                    .map_err(network_error)?,
            )
            .await?;
            if self
                .check_ollama(settings.ollama_url.clone())
                .await?
                .iter()
                .any(|m| m.name == name)
            {
                return Err(invalid("Ollama 仍列出该模型，请刷新后重试"));
            }
            Ok(())
        }
        .await;
        let mut slot = self.translation.lock();
        slot.cancel = None;
        slot.job = ModelJob::default();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t1513_model_delete_is_scoped_and_refuses_in_use_or_downloading_models() {
        let temp = tempfile::tempdir().unwrap();
        let db = Database::open_and_migrate(&temp.path().join("data"), &temp.path().join("cache"))
            .unwrap();
        let svc = RecognitionService::new(db.clone(), db.data_dir.clone(), "test.model.delete");
        let directory = svc.model_directory();
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("model.int8.onnx"), b"fixture").unwrap();
        let keep = db.data_dir.join("user-subtitle.srt");
        std::fs::write(&keep, b"keep").unwrap();
        assert!(svc.delete_recognition_model("../").is_err());
        let lease = svc.model_access.try_read().unwrap();
        assert!(svc.delete_recognition_model("sensevoice").is_err());
        drop(lease);
        let _job = RecognitionService::begin(&svc.recognition, "download".into()).unwrap();
        assert!(svc.delete_recognition_model("sensevoice").is_err());
        RecognitionService::finish(&svc.recognition, None);
        svc.delete_recognition_model("sensevoice").unwrap();
        assert!(!directory.exists());
        assert!(keep.is_file());
        assert_eq!(svc.settings().unwrap(), RecognitionSettings::default());
    }
    #[tokio::test]
    async fn t1513_ollama_delete_uses_api_and_checks_the_result() {
        use wiremock::{
            matchers::{body_json, method, path},
            Mock, MockServer, ResponseTemplate,
        };
        let temp = tempfile::tempdir().unwrap();
        let db = Database::open_and_migrate(&temp.path().join("data"), &temp.path().join("cache"))
            .unwrap();
        let svc = RecognitionService::new(db.clone(), db.data_dir.clone(), "test.ollama.delete");
        let server = MockServer::start().await;
        let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = reads.clone();
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(move |_: &wiremock::Request| {
                let models = if counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    serde_json::json!([{"name":"test:latest","size":100}])
                } else {
                    serde_json::json!([])
                };
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"models":models}))
            })
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/api/delete"))
            .and(body_json(serde_json::json!({"model":"test:latest"})))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        svc.delete_ollama_model(server.uri(), "test".into())
            .await
            .unwrap();
        assert!(svc.translation.lock().cancel.is_none());
        assert!(svc.translation.lock().job.installed_model.is_none());
    }
}
