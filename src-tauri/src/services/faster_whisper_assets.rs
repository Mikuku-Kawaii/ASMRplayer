//! Explicit, pinned, hash-verified standalone Python + CTranslate2 installations.
use super::*;
use std::sync::atomic::AtomicBool;
#[derive(Clone, Deserialize)]
struct Asset {
    name: String,
    url: String,
    size: u64,
    sha256: String,
}
#[derive(Deserialize)]
struct Manifest {
    runtime: Vec<Asset>,
    models: std::collections::BTreeMap<String, Vec<Asset>>,
}
fn manifest() -> Manifest {
    serde_json::from_str(include_str!("../../native/faster-whisper/assets.json"))
        .expect("pinned CT2 manifest")
}
fn assets(engine: &str) -> AppResult<Vec<Asset>> {
    let mut manifest = manifest();
    let model = manifest
        .models
        .remove(engine)
        .ok_or_else(|| invalid("未知 Whisper 模型"))?;
    manifest.runtime.extend(model);
    Ok(manifest.runtime)
}
const INSTALLER: &str = r#"import pathlib,sys,zipfile
root=pathlib.Path(sys.argv[1]); target=root/'Lib'/'site-packages'; target.mkdir(parents=True,exist_ok=True)
for wheel in root.glob('*.whl'):
    with zipfile.ZipFile(wheel) as archive:
        archive.extractall(target)
    wheel.unlink()
"#;
impl RecognitionService {
    pub fn ct2_directory(&self, engine: &str) -> AppResult<PathBuf> {
        let name = match engine {
            "faster_whisper" => "faster-whisper-small-1.2.1",
            "kotoba_whisper" => "kotoba-whisper-v2-1.2.1",
            _ => return Err(invalid("未知 Whisper 模型")),
        };
        Ok(self.db.data_dir.join("models").join(name))
    }
    pub fn ct2_bytes(&self, engine: &str) -> u64 {
        assets(engine)
            .map(|files| files.iter().map(|a| a.size).sum())
            .unwrap_or(0)
    }
    pub fn ct2_installed(&self, engine: &str) -> bool {
        let Ok(dir) = self.ct2_directory(engine) else {
            return false;
        };
        if ![
            "python.exe",
            "python312.dll",
            "python312.zip",
            "worker.py",
            "installed.json",
            "Lib/site-packages/faster_whisper/__init__.py",
        ]
        .iter()
        .all(|p| dir.join(p).is_file())
        {
            return false;
        }
        manifest().models.get(engine).is_some_and(|files| {
            files.iter().all(|a| {
                dir.join(&a.name)
                    .metadata()
                    .is_ok_and(|m| m.len() == a.size)
            })
        })
    }
    pub fn start_ct2(&self, engine: String) -> AppResult<()> {
        let target = self.ct2_directory(&engine)?;
        let mut cancel = Self::begin(&self.recognition, "下载 Whisper 模型与运行库…".into())?;
        let service = self.clone();
        tauri::async_runtime::spawn(async move {
            let staging = target.with_extension("part");
            let result = tokio::select! {
                biased;
                result=service.install_ct2(&engine,&target,&staging)=>Some(result),
                _=cancel.changed()=>None,
            };
            // Preserve verified downloads and partial files after failure or cancellation.
            Self::finish(&service.recognition, result);
        });
        Ok(())
    }
    async fn install_ct2(&self, engine: &str, target: &Path, staging: &Path) -> AppResult<()> {
        if target.exists() {
            return Err(invalid("模型目录已存在，请先在设置中删除该模型再重装"));
        }
        let files = assets(engine)?;
        tokio::fs::create_dir_all(staging.join("model"))
            .await
            .map_err(io_error)?;
        self.recognition.lock().job.total = Some(files.iter().map(|a| a.size).sum());
        let downloads = staging.join("downloads");
        tokio::fs::create_dir_all(downloads.join("model"))
            .await
            .map_err(io_error)?;
        for asset in &files {
            self.recognition.lock().job.label = format!("下载 {}", asset.name);
            self.download_model_asset(
                &downloads.join(&asset.name),
                &asset.url,
                asset.size,
                &asset.sha256,
            )
            .await?;
        }
        // Extract from verified cached inputs; installation may consume its copies.
        self.recognition.lock().job.label = "校验完成，准备安装文件…".into();
        for asset in &files {
            tokio::fs::copy(downloads.join(&asset.name), staging.join(&asset.name))
                .await
                .map_err(io_error)?;
        }
        self.recognition.lock().job.label = "准备独立识别运行库…".into();
        let tar =
            PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()))
                .join("System32/tar.exe");
        let mut command = tokio::process::Command::new(tar);
        command
            .arg("-xf")
            .arg(staging.join("python.zip"))
            .arg("-C")
            .arg(staging)
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        if !command.output().await.map_err(io_error)?.status.success() {
            return Err(invalid("无法解压独立 Python 运行库"));
        }
        tokio::fs::write(
            staging.join("python312._pth"),
            "python312.zip\n.\nLib/site-packages\n",
        )
        .await
        .map_err(io_error)?;
        tokio::fs::write(staging.join("install.py"), INSTALLER)
            .await
            .map_err(io_error)?;
        let mut command = tokio::process::Command::new(staging.join("python.exe"));
        command
            .arg("-I")
            .arg(staging.join("install.py"))
            .arg(staging)
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        if !command.output().await.map_err(io_error)?.status.success() {
            return Err(invalid("无法安装独立识别依赖"));
        }
        let mut command = tokio::process::Command::new(staging.join("python.exe"));
        command
            .args(["-I", "-c", "import faster_whisper, ctranslate2, numpy"])
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        if !command.output().await.map_err(io_error)?.status.success() {
            return Err(invalid(
                "Faster-Whisper 运行库加载失败，请检查 Visual C++ 运行库",
            ));
        }
        tokio::fs::write(
            staging.join("worker.py"),
            include_str!("../../native/faster-whisper/worker.py"),
        )
        .await
        .map_err(io_error)?;
        tokio::fs::write(
            staging.join("installed.json"),
            include_str!("../../native/faster-whisper/assets.json"),
        )
        .await
        .map_err(io_error)?;
        for name in ["python.zip", "install.py"] {
            tokio::fs::remove_file(staging.join(name))
                .await
                .map_err(io_error)?;
        }
        tokio::fs::rename(staging, target).await.map_err(io_error)?;
        let _ = tokio::fs::remove_dir_all(target.join("downloads")).await;
        Ok(())
    }
    pub fn check_ct2(&self, engine: &str) -> AppResult<String> {
        let _lease = self
            .model_access
            .try_read()
            .map_err(|_| invalid("模型正在删除"))?;
        self.require_engine_for(engine)?;
        let cancel = AtomicBool::new(false);
        let mut worker = super::super::faster_whisper::FasterWhisper::new(
            &self.ct2_directory(engine)?,
            &self.sensevoice_fp16_directory(),
            "ja",
            "",
            &cancel,
        )?;
        let probe: Vec<f32> = (0..16000).map(|i| (i as f32 * 0.1).sin() * 0.002).collect();
        worker.transcribe_phrases(&probe, 0, &cancel)?;
        Ok(format!("模型可用 · {}", worker.device))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t1513_ct2_manifest_and_model_selection_are_pinned_and_offline() {
        let root = tempfile::tempdir().unwrap();
        let db = Database::open_and_migrate(&root.path().join("data"), &root.path().join("cache"))
            .unwrap();
        let service = RecognitionService::new(db.clone(), db.data_dir.clone(), "ct2-test");
        for engine in ["faster_whisper", "kotoba_whisper"] {
            let mut settings = service.settings().unwrap();
            settings.preferences.recognition_engine = engine.into();
            service.save_settings(settings).unwrap();
            assert!(!service.ct2_installed(engine));
            assert!(service.require_engine_for(engine).is_err());
            assert!(!service.ct2_directory(engine).unwrap().exists());
            for asset in assets(engine).unwrap() {
                assert!(asset.url.starts_with("https://"));
                assert_eq!(asset.sha256.len(), 64);
                assert!(asset.size > 0);
                assert!(!asset.name.contains(".."));
            }
        }
        assert!(service.ct2_directory("../other").is_err());
    }
}
