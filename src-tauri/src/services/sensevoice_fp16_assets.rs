//! Explicit FP16/CUDA installation. All inputs are pinned; conversion is local.
use super::*;
use tokio::process::Command;

#[derive(Clone, Deserialize)]
struct Asset {
    name: String,
    size: u64,
    sha256: String,
    url: String,
    kind: String,
}
const DEPENDENCIES: &str = include_str!("../../native/sherpa/fp16-dependencies.json");
const CONVERTER: &str = include_str!("../../native/sherpa/convert-fp16.py");
const GPU_MODEL: &str = "model.fp16.onnx";
const REQUIRED: &[&str] = &[
    GPU_MODEL,
    "tokens.txt",
    "sherpa-onnx-c-api.dll",
    "onnxruntime.dll",
    "onnxruntime_providers_cuda.dll",
    "onnxruntime_providers_shared.dll",
    "cargs.dll",
    "cudart64_12.dll",
    "cublas64_12.dll",
    "cublasLt64_12.dll",
    "cudnn64_9.dll",
    "cufft64_11.dll",
    "nvJitLink_120_0.dll",
];

fn assets() -> Vec<Asset> {
    let mut list: Vec<Asset> =
        serde_json::from_str(DEPENDENCIES).expect("bundled dependency manifest");
    for (name, size, sha256, url, kind) in [
        ("model.onnx", 937617178, "977016bd9c79f9eb343430b5cc305e07ab64d5212dff41b0dcfa1694bee9a8cb", format!("{MODEL_BASE}/model.onnx"), "model"),
        ("tokens.txt", FILES[1].1, FILES[1].2, format!("{MODEL_BASE}/tokens.txt"), "model"),
        ("python.zip", 11133606, "4acbed6dd1c744b0376e3b1cf57ce906f9dc9e95e68824584c8099a63025a3c3", "https://www.python.org/ftp/python/3.12.10/python-3.12.10-embed-amd64.zip".into(), "embed"),
        ("runtime.tar.bz2", 379567716, "d0181babc7361a7a08af1db59637bb86656dbddbceddfc8be0bae7d65b221005", "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.12.20/sherpa-onnx-v1.12.20-cuda-12.x-cudnn-9.x-win-x64-cuda.tar.bz2".into(), "runtime"),
    ] { list.push(Asset { name: name.into(), size, sha256: sha256.into(), url, kind: kind.into() }); }
    list
}

fn hidden(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    command
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    command
}
async fn unpack(archive: &Path, directory: &Path) -> AppResult<()> {
    tokio::fs::create_dir_all(directory)
        .await
        .map_err(io_error)?;
    let tar = PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()))
        .join("System32/tar.exe");
    if !hidden(tar)
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(directory)
        .output()
        .await
        .map_err(io_error)?
        .status
        .success()
    {
        return Err(invalid("FP16/GPU 文件解压失败，请检查剩余空间后重试"));
    }
    Ok(())
}

impl RecognitionService {
    pub fn sensevoice_fp16_directory(&self) -> PathBuf {
        self.db
            .data_dir
            .join("models/sensevoice-small-fp16-cuda-1.12.20")
    }
    pub fn sensevoice_fp16_bytes(&self) -> u64 {
        assets().iter().map(|a| a.size).sum()
    }
    pub fn sensevoice_fp16_installed(&self) -> bool {
        let dir = self.sensevoice_fp16_directory();
        let Ok(bytes) = std::fs::read(dir.join("installed.json")) else {
            return false;
        };
        let Ok(files) = serde_json::from_slice::<std::collections::BTreeMap<String, u64>>(&bytes)
        else {
            return false;
        };
        REQUIRED.iter().all(|name| files.contains_key(*name))
            && files.iter().all(|(name, size)| {
                // Only direct files created by this installer are part of the receipt.
                !name.contains(['/', '\\', ':'])
                    && *size > 0
                    && dir
                        .join(name)
                        .metadata()
                        .is_ok_and(|m| m.is_file() && m.len() == *size)
            })
    }
    pub fn start_sensevoice_fp16(&self) -> AppResult<()> {
        let mut cancel = Self::begin(
            &self.recognition,
            "准备 SenseVoiceSmall FP16 与 CUDA…".into(),
        )?;
        let service = self.clone();
        tauri::async_runtime::spawn(async move {
            let result = tokio::select! {
                biased;
                result = service.install_sensevoice_fp16() => Some(result),
                _ = cancel.changed() => None,
            };
            // Keep only our staging download cache for verified resume. It is never playable/ready.
            Self::finish(&service.recognition, result);
        });
        Ok(())
    }
    async fn download_fp16_asset(&self, asset: &Asset, directory: &Path) -> AppResult<()> {
        self.download_model_asset(&directory.join(&asset.name), &asset.url, asset.size, &asset.sha256).await
    }
    async fn install_sensevoice_fp16(&self) -> AppResult<()> {
        let destination = self.sensevoice_fp16_directory();
        if destination.exists() {
            return Err(invalid("FP16 模型目录已存在，请检查已安装文件"));
        }
        let staging = destination.with_extension("part");
        let downloads = staging.join("downloads");
        tokio::fs::create_dir_all(&downloads)
            .await
            .map_err(io_error)?;
        let assets = assets();
        self.recognition.lock().job.total = Some(assets.iter().map(|a| a.size).sum());
        for asset in &assets {
            self.recognition.lock().job.label = format!("下载并校验 {}", asset.name);
            self.download_fp16_asset(asset, &downloads).await?;
        }
        self.recognition.lock().job.label = "准备 FP16 转换与 GPU 运行库…".into();
        let python = staging.join("python");
        unpack(&downloads.join("python.zip"), &python).await?;
        tokio::fs::write(
            python.join("python312._pth"),
            "python312.zip\n.\nimport site\n",
        )
        .await
        .map_err(io_error)?;
        for asset in assets.iter().filter(|a| a.kind == "python") {
            unpack(&downloads.join(&asset.name), &python).await?;
        }
        let unpacked = staging.join("unpacked");
        unpack(&downloads.join("runtime.tar.bz2"), &unpacked).await?;
        let ready = staging.join("ready");
        tokio::fs::create_dir_all(&ready).await.map_err(io_error)?;
        let runtime = unpacked.join("sherpa-onnx-v1.12.20-cuda-12.x-cudnn-9.x-win-x64-cuda/lib");
        for name in [
            "sherpa-onnx-c-api.dll",
            "onnxruntime.dll",
            "onnxruntime_providers_cuda.dll",
            "onnxruntime_providers_shared.dll",
            "cargs.dll",
        ] {
            tokio::fs::copy(runtime.join(name), ready.join(name))
                .await
                .map_err(io_error)?;
        }
        for asset in assets.iter().filter(|a| a.kind == "cuda") {
            let extracted = staging.join(&asset.name);
            unpack(&downloads.join(&asset.name), &extracted).await?;
            for entry in walkdir::WalkDir::new(&extracted)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_file())
            {
                if entry
                    .path()
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("dll"))
                {
                    tokio::fs::copy(entry.path(), ready.join(entry.file_name()))
                        .await
                        .map_err(io_error)?;
                } else if entry
                    .file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .contains("license")
                {
                    let license = ready.join("licenses").join(&asset.name);
                    tokio::fs::create_dir_all(&license)
                        .await
                        .map_err(io_error)?;
                    tokio::fs::copy(entry.path(), license.join(entry.file_name()))
                        .await
                        .map_err(io_error)?;
                }
            }
        }
        self.recognition.lock().job.label = "将官方模型转换为 FP16，请稍候…".into();
        let script = python.join("convert-fp16.py");
        tokio::fs::write(&script, CONVERTER)
            .await
            .map_err(io_error)?;
        tokio::fs::write(
            python.join("ort_float16.py"),
            include_str!("../../native/sherpa/ort_float16.py"),
        )
        .await
        .map_err(io_error)?;
        let result = hidden(python.join("python.exe"))
            .arg("-I")
            .arg(script)
            .arg(downloads.join("model.onnx"))
            .arg(ready.join(GPU_MODEL))
            .output()
            .await
            .map_err(io_error)?;
        if !result.status.success() {
            return Err(invalid(
                "FP16 模型转换失败，请检查剩余内存与磁盘空间后重试；下载文件已保留",
            ));
        }
        let report: serde_json::Value =
            serde_json::from_slice(&result.stdout).map_err(|_| invalid("FP16 转换结果校验失败"))?;
        if report["fp16Tensors"].as_u64().unwrap_or(0) == 0 {
            return Err(invalid("模型未生成 FP16 权重"));
        }
        tokio::fs::copy(downloads.join("tokens.txt"), ready.join("tokens.txt"))
            .await
            .map_err(io_error)?;
        tokio::fs::write(
            ready.join("LICENSE-sherpa"),
            include_str!("../../native/sherpa/LICENSE"),
        )
        .await
        .map_err(io_error)?;
        tokio::fs::write(
            ready.join("LICENSE-onnxruntime"),
            include_str!("../../native/sherpa/ONNXRUNTIME-LICENSE"),
        )
        .await
        .map_err(io_error)?;
        tokio::fs::write(
            ready.join("LICENSE-SenseVoice"),
            include_str!("../../native/sherpa/SENSEVOICE-LICENSE"),
        )
        .await
        .map_err(io_error)?;
        let mut receipt = std::collections::BTreeMap::new();
        let mut entries = tokio::fs::read_dir(&ready).await.map_err(io_error)?;
        while let Some(entry) = entries.next_entry().await.map_err(io_error)? {
            let meta = entry.metadata().await.map_err(io_error)?;
            if meta.is_file() {
                receipt.insert(entry.file_name().to_string_lossy().into_owned(), meta.len());
            }
        }
        if !REQUIRED
            .iter()
            .all(|name| receipt.get(*name).is_some_and(|size| *size > 0))
        {
            return Err(invalid("GPU 运行库文件不完整"));
        }
        tokio::fs::write(
            ready.join("installed.json"),
            serde_json::to_vec(&receipt).unwrap(),
        )
        .await
        .map_err(io_error)?;
        tokio::fs::rename(&ready, &destination)
            .await
            .map_err(io_error)?;
        let _ = tokio::fs::remove_dir_all(staging).await;
        Ok(())
    }
    pub fn check_sensevoice_gpu(&self) -> AppResult<String> {
        let _model_use = self.model_access.try_read().map_err(|_| invalid("正在删除模型，请稍后检测"))?;
        self.require_engine_for("sensevoice_fp16")?;
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let mut engine = super::super::sensevoice_gpu::GpuRecognizer::new(
            &self.sensevoice_fp16_directory(),
            "ja",
            &cancel,
        )?;
        engine.transcribe(&vec![0.0; 16000], &cancel)?;
        Ok(format!("{} · CUDA · FP16 推理通过", engine.device))
    }
}

pub(super) fn validate_range(value: Option<&str>, offset: u64, size: u64) -> AppResult<()> {
    let parsed = value
        .and_then(|v| v.strip_prefix("bytes "))
        .and_then(|v| v.split_once('/'))
        .and_then(|(range, total)| {
            range
                .split_once('-')
                .map(|(start, end)| (start, end, total))
        })
        .and_then(|(start, end, total)| {
            Some((
                start.parse::<u64>().ok()?,
                end.parse::<u64>().ok()?,
                total.parse::<u64>().ok()?,
            ))
        });
    if !parsed.is_some_and(|(start, end, total)| {
        start == offset && end >= start && end < total && total == size
    }) {
        return Err(invalid("模型续传范围不匹配，已停止写入"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn t1513_fp16_download_resume_checks_range_hash_and_reuses_complete_files() {
        use wiremock::{
            matchers::{header, method, path},
            Mock, MockServer, ResponseTemplate,
        };
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache"))
            .unwrap();
        let svc = RecognitionService::new(db.clone(), db.data_dir.clone(), "test.gpu.resume");
        let server = MockServer::start().await;
        let asset = Asset {
            name: "fixture.bin".into(),
            size: 6,
            sha256: hex::encode(Sha256::digest(b"abcdef")),
            url: format!("{}/file", server.uri()),
            kind: "model".into(),
        };
        std::fs::write(dir.path().join("fixture.part"), b"abc").unwrap();
        Mock::given(method("GET"))
            .and(path("/file"))
            .and(header("Range", "bytes=3-"))
            .respond_with(
                ResponseTemplate::new(206)
                    .insert_header("Content-Range", "bytes 3-5/6")
                    .set_body_bytes(b"def".to_vec()),
            )
            .expect(1)
            .mount(&server)
            .await;
        svc.download_fp16_asset(&asset, dir.path()).await.unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("fixture.bin")).unwrap(),
            b"abcdef"
        );
        svc.download_fp16_asset(&asset, dir.path()).await.unwrap(); // no additional HTTP
        server.verify().await;
        server.reset().await;
        std::fs::remove_file(dir.path().join("fixture.bin")).unwrap();
        std::fs::write(dir.path().join("fixture.part"), b"abc").unwrap();
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"abcdef".to_vec()))
            .mount(&server)
            .await;
        svc.download_fp16_asset(&asset, dir.path()).await.unwrap(); // ignored Range must truncate
        assert_eq!(
            std::fs::read(dir.path().join("fixture.bin")).unwrap(),
            b"abcdef"
        );
        server.reset().await;
        std::fs::remove_file(dir.path().join("fixture.bin")).unwrap();
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"abcdeg".to_vec()))
            .mount(&server)
            .await;
        assert!(svc.download_fp16_asset(&asset, dir.path()).await.is_err());
        assert!(!dir.path().join("fixture.bin").exists());
    }

    #[tokio::test]
    #[ignore = "requires verified asset cache; performs the full isolated FP16 installer without network"]
    async fn t1513_fp16_real_installer_from_verified_cache() {
        let cache = PathBuf::from(
            std::env::var_os("ASMR_FP16_ASSET_CACHE").expect("asset cache directory"),
        );
        let dir = tempfile::tempdir_in(cache.parent().unwrap()).unwrap();
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache"))
            .unwrap();
        let svc = RecognitionService::new(db.clone(), db.data_dir.clone(), "test.gpu.install");
        let staging = svc
            .sensevoice_fp16_directory()
            .with_extension("part")
            .join("downloads");
        std::fs::create_dir_all(&staging).unwrap();
        for asset in assets() {
            std::fs::hard_link(cache.join(&asset.name), staging.join(&asset.name)).unwrap();
        }
        svc.install_sensevoice_fp16().await.unwrap();
        assert!(svc.sensevoice_fp16_installed());
        assert!(svc.require_engine_for("sensevoice_fp16").is_ok());
        assert!(!svc
            .sensevoice_fp16_directory()
            .with_extension("part")
            .exists());
        assert!(
            std::fs::metadata(svc.sensevoice_fp16_directory().join(GPU_MODEL))
                .unwrap()
                .len()
                < 600_000_000
        );
    }
    #[test]
    fn t1513_fp16_selection_and_receipt_do_not_install_or_mislabel_cpu() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache"))
            .unwrap();
        let svc = RecognitionService::new(db.clone(), db.data_dir.clone(), "test.gpu");
        let mut settings = svc.settings().unwrap();
        settings.preferences.recognition_engine = "sensevoice_fp16".into();
        svc.save_settings(settings).unwrap();
        assert!(!svc.sensevoice_fp16_installed());
        assert!(svc.require_engine().is_err());
        assert!(!svc.sensevoice_fp16_directory().exists());
        assert!(svc.recognition.lock().cancel.is_none());
        for a in assets() {
            assert!(a.url.starts_with("https://"));
            assert_eq!(a.sha256.len(), 64);
            assert!(a.size > 0);
        }
        assert!(validate_range(Some("bytes 100-199/200"), 100, 200).is_ok());
        for bad in [
            None,
            Some("bytes 0-199/200"),
            Some("bytes 100-200/200"),
            Some("bytes 100-199/*"),
        ] {
            assert!(validate_range(bad, 100, 200).is_err());
        }
    }
}
