//! Child of recognition: explicit download action shares its cancellable job slot.
use super::*;

// Official release asset digest and Hugging Face LFS metadata checked 2026-10-15.
pub(super) const MODEL_BYTES: u64 = 190085487;
pub(super) const RUNTIME_BYTES: u64 = 3832432;
const ASSETS: [(&str, u64, &str, &str); 2] = [
    ("runtime.zip", RUNTIME_BYTES, "b1514ebc099765e39fa37eb780b92a140a94c86bb0b3b3d98226b38825979732",
     "https://github.com/ggml-org/whisper.cpp/releases/download/v1.8.2/whisper-bin-x64.zip"),
    ("ggml-small-q5_1.bin", MODEL_BYTES, "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb",
     "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-small-q5_1.bin"),
];

impl RecognitionService {
    pub fn whisper_directory(&self) -> PathBuf { self.db.data_dir.join("models/whisper-small-q5_1-1.8.2") }
    pub fn whisper_installed(&self) -> bool {
        let dir = self.whisper_directory();
        ["whisper.dll", "ggml.dll", "ggml-base.dll", "ggml-cpu.dll"].iter().all(|name| dir.join(name).is_file())
            && dir.join("ggml-small-q5_1.bin").metadata().is_ok_and(|m| m.is_file() && m.len() == MODEL_BYTES)
    }
    pub fn start_whisper(&self) -> AppResult<()> {
        let mut cancel = Self::begin(&self.recognition, "下载 Whisper 模型与运行库…".into())?;
        let service = self.clone();
        // This entry point is called by a synchronous command on the UI thread.
        tauri::async_runtime::spawn(async move {
            let staging = service.whisper_directory().with_extension("part");
            let result = tokio::select! {
                biased;
                result = service.install_whisper(&staging) => Some(result),
                _ = cancel.changed() => None,
            };
            let _ = tokio::fs::remove_dir_all(&staging).await;
            Self::finish(&service.recognition, result);
        });
        Ok(())
    }
    async fn install_whisper(&self, staging: &Path) -> AppResult<()> {
        if self.whisper_directory().exists() { return Err(invalid("Whisper 目录已存在，请检查已安装文件")); }
        tokio::fs::create_dir_all(staging).await.map_err(io_error)?;
        self.recognition.lock().job.total = Some(MODEL_BYTES + RUNTIME_BYTES);
        for (name, size, digest, url) in ASSETS {
            self.recognition.lock().job.label = format!("下载 {name}");
            let response = Self::checked(Self::download_client(false)?.get(url).send().await.map_err(network_error)?).await?;
            let mut stream = response.bytes_stream();
            let mut output = tokio::fs::File::create(staging.join(name)).await.map_err(io_error)?;
            let mut hash = Sha256::new();
            let mut written = 0;
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(network_error)?;
                written += chunk.len() as u64;
                if written > size { return Err(invalid("Whisper 文件大小与发布版本不匹配")); }
                hash.update(&chunk);
                output.write_all(&chunk).await.map_err(io_error)?;
                self.recognition.lock().job.completed += chunk.len() as u64;
            }
            verify_file(written, &hex::encode(hash.finalize()), size, digest)?;
            output.sync_all().await.map_err(io_error)?;
        }
        self.recognition.lock().job.label = "解压并检查 Whisper 运行库…".into();
        let tar = PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into())).join("System32/tar.exe");
        let mut command = tokio::process::Command::new(tar);
        command.arg("-xf").arg(staging.join("runtime.zip")).arg("-C").arg(staging).kill_on_drop(true);
        #[cfg(windows)] command.creation_flags(0x08000000);
        if !command.output().await.map_err(io_error)?.status.success() { return Err(invalid("Whisper 解压失败，请检查磁盘空间")); }
        // The verified upstream archive may contain a Release directory. Only copy its DLLs.
        let dll_dir = find_runtime(staging, 0).ok_or_else(|| invalid("Whisper 压缩包缺少运行库"))?;
        let ready = staging.join("ready");
        tokio::fs::create_dir_all(&ready).await.map_err(io_error)?;
        for name in ["whisper.dll", "ggml.dll", "ggml-base.dll", "ggml-cpu.dll"] {
            tokio::fs::copy(dll_dir.join(name), ready.join(name)).await.map_err(io_error)?;
        }
        tokio::fs::rename(staging.join("ggml-small-q5_1.bin"), ready.join("ggml-small-q5_1.bin")).await.map_err(io_error)?;
        tokio::fs::write(ready.join("LICENSE"), include_str!("../../native/whisper/LICENSE")).await.map_err(io_error)?;
        tokio::fs::rename(ready, self.whisper_directory()).await.map_err(io_error)?;
        Ok(())
    }
}
fn find_runtime(directory: &Path, depth: usize) -> Option<PathBuf> {
    if directory.join("whisper.dll").is_file() { return Some(directory.to_owned()); }
    if depth >= 4 { return None; }
    std::fs::read_dir(directory).ok()?.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .find_map(|entry| find_runtime(&entry.path(), depth + 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t1513_whisper_download_from_ui_thread_reports_failure_and_allows_retry() {
        // Synchronous Tauri commands run without an entered Tokio runtime.
        assert!(tokio::runtime::Handle::try_current().is_err());
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache")).unwrap();
        let service = RecognitionService::new(db.clone(), db.data_dir.clone(), "test.whisper");
        // Fail before any network request, while exercising the actual background job.
        let installed = service.whisper_directory();
        std::fs::create_dir_all(&installed).unwrap();
        std::fs::write(installed.join("sentinel"), b"preserve").unwrap();
        for _ in 0..2 {
            let staging = installed.with_extension("part");
            std::fs::create_dir_all(&staging).unwrap();
            service.start_whisper().unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                if service.recognition.lock().cancel.is_none() { break; }
                assert!(std::time::Instant::now() < deadline, "Whisper job did not finish");
                std::thread::sleep(Duration::from_millis(10));
            }
            let job = service.recognition.lock().job.clone();
            assert_eq!(job.state, "failed");
            assert_eq!(job.error.as_deref(), Some("Whisper 目录已存在，请检查已安装文件"));
            assert!(!staging.exists());
            assert_eq!(std::fs::read(installed.join("sentinel")).unwrap(), b"preserve");
        }
    }

    #[test]
    fn t1513_whisper_selection_and_readiness_never_create_or_download_assets() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache")).unwrap();
        let service = RecognitionService::new(db.clone(), db.data_dir.clone(), "test.whisper");
        let mut settings = service.settings().unwrap();
        settings.preferences.recognition_engine = "whisper".into();
        service.save_settings(settings).unwrap();
        assert!(service.require_engine().is_err());
        assert!(!service.whisper_installed());
        assert!(!service.whisper_directory().exists());
        assert!(!service.whisper_directory().with_extension("part").exists());
        assert!(service.recognition.lock().cancel.is_none());
        let reopened = RecognitionService::new(db.clone(), db.data_dir.clone(), "test.whisper");
        assert_eq!(reopened.settings().unwrap().preferences.recognition_engine, "whisper");
        assert!(reopened.require_engine().is_err());
        for (_, size, digest, _) in ASSETS {
            assert!(verify_file(size, digest, size, digest).is_ok());
            assert!(verify_file(size - 1, digest, size, digest).is_err());
            assert!(verify_file(size, "wrong", size, digest).is_err());
        }
        let release = dir.path().join("extract/Release");
        std::fs::create_dir_all(&release).unwrap();
        std::fs::write(release.join("whisper.dll"), b"synthetic").unwrap();
        assert_eq!(find_runtime(&dir.path().join("extract"), 0), Some(release));
    }
}
