//! Player-managed, verified GGUF transfer; Ollama only receives local blobs.
use super::*;
use reqwest::{header, StatusCode};
use tokio::io::AsyncSeekExt;

// Qwen's official Q4_K_M artifact, pinned to its repository revision (Apache-2.0).
const QWEN_URL: &str = "https://huggingface.co/Qwen/Qwen3-4B-GGUF/resolve/bc640142c66e1fdd12af0bd68f40445458f3869b/Qwen3-4B-Q4_K_M.gguf";
const QWEN_SIZE: u64 = 2_497_280_256;
const QWEN_SHA: &str = "7485fe6f11af29433bc51cab58009521f205840f5b4ae3a32fa7f92e8534fdf5";

fn gguf_model_name(sha: &str) -> String {
    if sha == QWEN_SHA {
        "asmr-qwen3:4b-q4_k_m-7485fe6f".into()
    } else {
        format!("asmr-gguf:{}", &sha[..16])
    }
}

fn transfer_error(_: reqwest::Error) -> AppError {
    AppError::new(ErrorCode::NetworkUnavailable,
        "替代下载连接失败或中断，已下载部分会保留。可再次替代下载，或下载单文件 GGUF 后从本地导入。")
}

impl RecognitionService {
    pub fn start_translation_alternative(&self) -> AppResult<()> {
        let settings = validate_settings(self.settings()?)?;
        if !matches!(
            settings.ollama_model.to_ascii_lowercase().as_str(),
            "qwen3:4b" | "qwen3:4b-q4_k_m"
        ) {
            return Err(invalid(
                "替代下载目前支持 qwen3:4b（Q4_K_M）；其他模型请导入本地 GGUF。",
            ));
        }
        self.start_gguf_job(settings, None)
    }

    pub fn start_translation_import(&self, path: PathBuf) -> AppResult<()> {
        if !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("gguf"))
        {
            return Err(invalid("请选择单文件 .gguf 模型"));
        }
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains("-of-"))
        {
            return Err(invalid("此入口支持单文件 GGUF；请先合并分片模型后再导入。"));
        }
        self.start_gguf_job(validate_settings(self.settings()?)?, Some(path))
    }

    fn start_gguf_job(
        &self,
        settings: RecognitionSettings,
        local: Option<PathBuf>,
    ) -> AppResult<()> {
        let mut cancel = Self::begin(&self.translation, "准备导入翻译模型…".into())?;
        self.translation.lock().job.alternative_available = local.is_none();
        let service = self.clone();
        // Both download/import commands may run outside an entered Tokio runtime.
        tauri::async_runtime::spawn(async move {
            let result = tokio::select! {
                biased;
                _ = cancel.changed() => None,
                result = service.prepare_gguf(&settings, local) => Some(result),
            };
            Self::finish(&service.translation, result);
        });
        Ok(())
    }

    fn gguf_progress(&self, label: &str, completed: u64, total: Option<u64>) {
        let mut slot = self.translation.lock();
        slot.job.label = label.into();
        slot.job.completed = completed;
        slot.job.total = total;
    }

    async fn prepare_gguf(
        &self,
        settings: &RecognitionSettings,
        local: Option<PathBuf>,
    ) -> AppResult<()> {
        self.check_ollama(settings.ollama_url.clone()).await?;
        let downloaded = local.is_none();
        let path = if let Some(path) = local {
            path
        } else {
            let cache = self
                .directory
                .parent()
                .ok_or_else(|| invalid("模型目录无效"))?
                .join("translation-downloads");
            tokio::fs::create_dir_all(&cache).await.map_err(io_error)?;
            let client = reqwest::Client::builder()
                .https_only(true)
                .connect_timeout(Duration::from_secs(20))
                .read_timeout(Duration::from_secs(600))
                .redirect(reqwest::redirect::Policy::limited(10))
                .build()
                .map_err(transfer_error)?;
            self.download_gguf(
                &client,
                QWEN_URL,
                &cache.join(format!("{QWEN_SHA}.gguf")),
                QWEN_SIZE,
                QWEN_SHA,
            )
            .await?
        };
        let model = self.import_gguf(settings, &path).await?;
        if downloaded {
            // Only the app-owned verified download copy; never remove a user-selected file.
            let _ = tokio::fs::remove_file(path).await;
        }
        // Preserve unrelated preferences and a newer model selection made in another window.
        let mut current = self.settings()?;
        if current.ollama_url == settings.ollama_url
            && current.ollama_model == settings.ollama_model
        {
            current.ollama_model = model.clone();
            self.save_settings(current)?;
        }
        self.translation.lock().job.installed_model = Some(model);
        Ok(())
    }

    async fn hash_gguf(&self, file: &mut tokio::fs::File) -> AppResult<(u64, String)> {
        let metadata = file.metadata().await.map_err(io_error)?;
        let size = metadata.len();
        if !metadata.is_file() || size < 24 {
            return Err(invalid("GGUF 文件为空或不完整，请选择完整的单文件模型"));
        }
        let mut header = [0u8; 24];
        file.read_exact(&mut header).await.map_err(io_error)?;
        let version = u32::from_le_bytes(header[4..8].try_into().unwrap());
        if &header[..4] != b"GGUF"
            || !matches!(version, 2 | 3)
            || u64::from_le_bytes(header[8..16].try_into().unwrap()) == 0
        {
            return Err(invalid(
                "文件不是完整的 GGUF v2/v3 模型；请勿选择网页、下载指针或未完成文件",
            ));
        }
        file.rewind().await.map_err(io_error)?;
        self.gguf_progress("校验 GGUF 文件…", 0, Some(size));
        let mut hash = Sha256::new();
        let mut count = 0;
        let mut buffer = vec![0; 1024 * 1024];
        loop {
            let length = file.read(&mut buffer).await.map_err(io_error)?;
            if length == 0 {
                break;
            }
            hash.update(&buffer[..length]);
            count += length as u64;
            self.translation.lock().job.completed = count;
        }
        if count != size {
            return Err(invalid(
                "读取过程中模型文件发生变化，请停止修改文件后重新导入",
            ));
        }
        file.rewind().await.map_err(io_error)?;
        Ok((size, hex::encode(hash.finalize())))
    }

    async fn download_gguf(
        &self,
        client: &reqwest::Client,
        url: &str,
        target: &Path,
        size: u64,
        sha: &str,
    ) -> AppResult<PathBuf> {
        if target.is_file() {
            let mut file = tokio::fs::File::open(target).await.map_err(io_error)?;
            let (actual_size, actual_sha) = self.hash_gguf(&mut file).await?;
            verify_file(actual_size, &actual_sha, size, sha)?;
            return Ok(target.into());
        }
        let partial = target.with_extension("gguf.part");
        let mut output = tokio::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&partial)
            .await
            .map_err(io_error)?;
        let mut offset = output.metadata().await.map_err(io_error)?.len();
        if offset > size {
            output.set_len(0).await.map_err(io_error)?;
            offset = 0;
        }
        if offset < size {
            self.gguf_progress("替代下载 Qwen3 4B…", offset, Some(size));
            let mut request = client.get(url).header(header::ACCEPT_ENCODING, "identity");
            if offset > 0 {
                request = request.header(header::RANGE, format!("bytes={offset}-"));
            }
            let response = request.send().await.map_err(transfer_error)?;
            match response.status() {
                StatusCode::PARTIAL_CONTENT => {
                    let expected = format!("bytes {offset}-{}/{size}", size - 1);
                    if response
                        .headers()
                        .get(header::CONTENT_RANGE)
                        .and_then(|v| v.to_str().ok())
                        != Some(expected.as_str())
                    {
                        return Err(invalid(
                            "下载源返回了错误的续传范围，已保留下载进度，请重试",
                        ));
                    }
                }
                StatusCode::OK => {
                    // A server ignoring Range must replace, never append to, the partial file.
                    output.set_len(0).await.map_err(io_error)?;
                    offset = 0;
                }
                _ => {
                    Self::checked(response).await?;
                    return Err(invalid("替代下载响应无效"));
                }
            }
            output
                .seek(std::io::SeekFrom::Start(offset))
                .await
                .map_err(io_error)?;
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(transfer_error)?;
                if offset + chunk.len() as u64 > size {
                    return Err(invalid("下载文件大小超出预期，已停止下载"));
                }
                output.write_all(&chunk).await.map_err(io_error)?;
                offset += chunk.len() as u64;
                self.translation.lock().job.completed = offset;
            }
            output.flush().await.map_err(io_error)?;
        }
        if offset != size {
            return Err(invalid("替代下载中断，已保留下载进度，请再次点击替代下载"));
        }
        output.rewind().await.map_err(io_error)?;
        let verified = self
            .hash_gguf(&mut output)
            .await
            .and_then(|(actual_size, actual_sha)| verify_file(actual_size, &actual_sha, size, sha));
        if let Err(error) = verified {
            output.set_len(0).await.map_err(io_error)?;
            return Err(error);
        }
        output.sync_all().await.map_err(io_error)?;
        drop(output);
        tokio::fs::rename(partial, target).await.map_err(io_error)?;
        Ok(target.into())
    }

    async fn import_gguf(&self, settings: &RecognitionSettings, path: &Path) -> AppResult<String> {
        let mut file = tokio::fs::File::open(path).await.map_err(io_error)?;
        let (size, sha) = self.hash_gguf(&mut file).await?;
        let model = gguf_model_name(&sha);
        // Content-derived names make retries idempotent and keep the user's existing names intact.
        if self
            .check_ollama(settings.ollama_url.clone())
            .await?
            .iter()
            .any(|entry| entry.name == model)
        {
            self.verify_gguf_capabilities(settings, &model).await?;
            return Ok(model);
        }
        let client = Self::download_client(true)?;
        let digest = format!("sha256:{sha}");
        let blob_url = format!("{}/api/blobs/{digest}", settings.ollama_url);
        self.gguf_progress("检查 Ollama 模型文件…", 0, None);
        let head = client
            .head(&blob_url)
            .send()
            .await
            .map_err(ollama_network_error)?;
        if head.status() == StatusCode::NOT_FOUND {
            self.gguf_progress("导入 GGUF 到 Ollama…", 0, Some(size));
            let slot = self.translation.clone();
            let stream = futures_util::stream::try_unfold(file, move |mut file| {
                let slot = slot.clone();
                async move {
                    let mut buffer = vec![0; 1024 * 1024];
                    let length = file.read(&mut buffer).await?;
                    if length == 0 {
                        return Ok::<_, std::io::Error>(None);
                    }
                    buffer.truncate(length);
                    slot.lock().job.completed += length as u64;
                    Ok(Some((buffer, file)))
                }
            });
            let response = client
                .post(&blob_url)
                .header(header::CONTENT_LENGTH, size)
                .body(reqwest::Body::wrap_stream(stream))
                .send()
                .await
                .map_err(ollama_network_error)?;
            Self::checked(response).await?;
        } else {
            Self::checked(head).await?;
        }
        self.gguf_progress("Ollama 正在创建模型…", 0, None);
        let response = client
            .post(format!("{}/api/create", settings.ollama_url))
            .json(&serde_json::json!({"model":model,"files":{"model.gguf":digest},"stream":true}))
            .send()
            .await
            .map_err(ollama_network_error)?;
        self.read_gguf_creation(response).await?;
        self.gguf_progress("确认模型已安装…", 0, None);
        if !self
            .check_ollama(settings.ollama_url.clone())
            .await?
            .iter()
            .any(|entry| entry.name == model)
        {
            return Err(invalid("Ollama 尚未列出导入模型，请检测服务后重试导入"));
        }
        self.verify_gguf_capabilities(settings, &model).await?;
        Ok(model)
    }

    async fn verify_gguf_capabilities(
        &self,
        settings: &RecognitionSettings,
        model: &str,
    ) -> AppResult<()> {
        let response = Self::checked(
            Self::client(true)?
                .post(format!("{}/api/show", settings.ollama_url))
                .json(&serde_json::json!({"model":model}))
                .send()
                .await
                .map_err(ollama_network_error)?,
        )
        .await?;
        let info: serde_json::Value = response.json().await.map_err(ollama_network_error)?;
        if !info["capabilities"]
            .as_array()
            .is_some_and(|caps| caps.iter().any(|cap| cap == "completion"))
        {
            return Err(invalid("模型已导入，但 Ollama 未确认其文本生成能力。请选择对话模型，并确保 Ollama 已更新；当前翻译模型未切换。"));
        }
        Ok(())
    }

    async fn read_gguf_creation(&self, response: reqwest::Response) -> AppResult<()> {
        if !response.status().is_success() {
            return Err(invalid(
                "Ollama 无法导入此 GGUF，请确认模型架构受支持、文件完整且 Ollama 已更新",
            ));
        }
        let mut stream = response.bytes_stream();
        let mut pending = Vec::new();
        let mut success = false;
        while let Some(chunk) = stream.next().await {
            pending.extend_from_slice(&chunk.map_err(ollama_network_error)?);
            while let Some(end) = pending.iter().position(|b| *b == b'\n') {
                let line: Vec<_> = pending.drain(..=end).collect();
                success |= self.gguf_creation_line(&line)?;
            }
            if pending.len() > 1024 * 1024 {
                return Err(invalid("Ollama 导入响应过长"));
            }
        }
        if !pending.is_empty() {
            success |= self.gguf_creation_line(&pending)?;
        }
        if !success {
            return Err(invalid(
                "GGUF 导入未完成，请重试；已传入 Ollama 的文件会复用",
            ));
        }
        Ok(())
    }

    fn gguf_creation_line(&self, line: &[u8]) -> AppResult<bool> {
        if line.iter().all(u8::is_ascii_whitespace) {
            return Ok(false);
        }
        let value: serde_json::Value =
            serde_json::from_slice(line).map_err(|_| invalid("Ollama 导入响应无效"))?;
        if value.get("error").is_some() {
            return Err(invalid("Ollama 无法解析或创建此 GGUF，请检查文件完整性、模型架构和磁盘空间，并更新 Ollama 后重试"));
        }
        let status = value["status"]
            .as_str()
            .ok_or_else(|| invalid("Ollama 未返回导入状态"))?;
        self.gguf_progress(
            if status == "parsing GGUF" {
                "Ollama 正在解析 GGUF…"
            } else {
                "Ollama 正在保存模型…"
            },
            0,
            None,
        );
        Ok(status == "success")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use wiremock::{
        matchers::{body_bytes, header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn fixture() -> (tempfile::TempDir, RecognitionService, Vec<u8>, String) {
        let root = tempfile::tempdir().unwrap();
        let db = Database::open_and_migrate(&root.path().join("data"), &root.path().join("cache"))
            .unwrap();
        let svc = RecognitionService::new(db, root.path().join("data"), "gguf-test");
        let mut bytes = b"GGUF".to_vec();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&1u64.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.extend_from_slice(&[17; 1024]);
        let sha = hex::encode(Sha256::digest(&bytes));
        (root, svc, bytes, sha)
    }

    async fn done(svc: &RecognitionService) -> ModelJob {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if svc.translation.lock().cancel.is_none() {
                    return svc.translation.lock().job.clone();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn gguf_download_resumes_206_and_restarts_on_200() {
        for range_supported in [true, false] {
            let server = MockServer::start().await;
            let (root, svc, bytes, sha) = fixture();
            let target = root.path().join("download.gguf");
            tokio::fs::write(target.with_extension("gguf.part"), &bytes[..128])
                .await
                .unwrap();
            let response = if range_supported {
                ResponseTemplate::new(206)
                    .insert_header(
                        "Content-Range",
                        format!("bytes 128-{}/{}", bytes.len() - 1, bytes.len()),
                    )
                    .set_body_bytes(bytes[128..].to_vec())
            } else {
                ResponseTemplate::new(200).set_body_bytes(bytes.clone())
            };
            Mock::given(method("GET"))
                .and(header("Range", "bytes=128-"))
                .respond_with(response)
                .expect(1)
                .mount(&server)
                .await;
            svc.download_gguf(
                &test_client(),
                &server.uri(),
                &target,
                bytes.len() as u64,
                &sha,
            )
            .await
            .unwrap();
            assert_eq!(tokio::fs::read(&target).await.unwrap(), bytes);
            assert!(!target.with_extension("gguf.part").exists());
            // A verified completed file is reused without another download.
            svc.download_gguf(
                &test_client(),
                &server.uri(),
                &target,
                bytes.len() as u64,
                &sha,
            )
            .await
            .unwrap();
        }
    }

    // No environment/system proxy for isolated loopback fixtures.
    fn test_client() -> reqwest::Client {
        reqwest::Client::builder().no_proxy().build().unwrap()
    }

    #[tokio::test]
    async fn gguf_bad_range_and_bad_hash_never_become_installed_files() {
        let server = MockServer::start().await;
        let (root, svc, bytes, sha) = fixture();
        let target = root.path().join("download.gguf");
        let partial = target.with_extension("gguf.part");
        tokio::fs::write(&partial, &bytes[..128]).await.unwrap();
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(206)
                    .insert_header("Content-Range", "bytes 0-9/10")
                    .set_body_bytes(vec![0; 10]),
            )
            .mount(&server)
            .await;
        assert!(svc
            .download_gguf(
                &test_client(),
                &server.uri(),
                &target,
                bytes.len() as u64,
                &sha
            )
            .await
            .is_err());
        assert_eq!(tokio::fs::read(&partial).await.unwrap(), bytes[..128]);
        server.reset().await;
        let mut corrupt = bytes.clone();
        corrupt[1000] ^= 1;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(corrupt))
            .mount(&server)
            .await;
        assert!(svc
            .download_gguf(
                &test_client(),
                &server.uri(),
                &target,
                bytes.len() as u64,
                &sha
            )
            .await
            .is_err());
        assert!(!target.exists());
        assert_eq!(tokio::fs::metadata(partial).await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn gguf_interrupted_download_keeps_partial_for_retry() {
        let server = MockServer::start().await;
        let (root, svc, bytes, sha) = fixture();
        let target = root.path().join("download.gguf");
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes[..128].to_vec()))
            .mount(&server)
            .await;
        assert!(svc
            .download_gguf(
                &test_client(),
                &server.uri(),
                &target,
                bytes.len() as u64,
                &sha
            )
            .await
            .is_err());
        assert_eq!(
            tokio::fs::metadata(target.with_extension("gguf.part"))
                .await
                .unwrap()
                .len(),
            128
        );
        assert!(!target.exists());
    }

    async fn import_server(
        server: &MockServer,
        bytes: &[u8],
        sha: &str,
        outcome: &'static str,
        listed: bool,
    ) {
        let created = Arc::new(AtomicBool::new(false));
        let flag = created.clone();
        let model = gguf_model_name(sha);
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(move |_: &wiremock::Request| {
                let names = if listed && flag.load(Ordering::SeqCst) {
                    serde_json::json!([{"name":model,"size":1048}])
                } else {
                    serde_json::json!([])
                };
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"models":names}))
            })
            .mount(server)
            .await;
        let blob = format!("/api/blobs/sha256:{sha}");
        Mock::given(method("HEAD"))
            .and(path(blob.clone()))
            .respond_with(ResponseTemplate::new(404))
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path(blob))
            .and(body_bytes(bytes.to_vec()))
            .respond_with(ResponseTemplate::new(201))
            .expect(1)
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/show"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"capabilities":["completion"]})),
            )
            .mount(server)
            .await;
        let expected_digest = format!("sha256:{sha}");
        Mock::given(method("POST"))
            .and(path("/api/create"))
            .respond_with(move |request: &wiremock::Request| {
                let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                assert_eq!(body["files"]["model.gguf"], expected_digest);
                assert_eq!(body["stream"], true);
                assert!(body.get("from").is_none());
                assert!(body.get("insecure").is_none());
                created.store(true, Ordering::SeqCst);
                ResponseTemplate::new(200).set_body_string(outcome)
            })
            .expect(1)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn gguf_import_uploads_verifies_listing_and_preserves_original() {
        let server = MockServer::start().await;
        let (root, svc, bytes, sha) = fixture();
        import_server(
            &server,
            &bytes,
            &sha,
            "{\"status\":\"parsing GGUF\"}\n{\"status\":\"success\"}",
            true,
        )
        .await;
        svc.save_settings(RecognitionSettings {
            ollama_url: server.uri(),
            ..Default::default()
        })
        .unwrap();
        let original = root.path().join("用户模型.gguf");
        tokio::fs::write(&original, &bytes).await.unwrap();
        let ui_service = svc.clone();
        let ui_path = original.clone();
        std::thread::spawn(move || {
            assert!(tokio::runtime::Handle::try_current().is_err());
            ui_service.start_translation_import(ui_path)
        }).join().unwrap().unwrap();
        let job = done(&svc).await;
        assert_eq!(job.state, "completed", "{:?}", job.error);
        let model = gguf_model_name(&sha);
        assert_eq!(job.installed_model.as_deref(), Some(model.as_str()));
        assert_eq!(svc.settings().unwrap().ollama_model, model);
        assert_eq!(tokio::fs::read(&original).await.unwrap(), bytes);
        // Reimporting the same bytes reuses the registered model (no extra upload/create).
        svc.start_translation_import(original).unwrap();
        assert_eq!(done(&svc).await.state, "completed");
    }

    #[tokio::test]
    async fn gguf_failed_or_unlisted_create_does_not_change_selection() {
        for (outcome, listed) in [
            ("{\"error\":\"unsupported private/path\"}", true),
            ("{\"status\":\"success\"}", false),
            ("{\"status\":\"parsing GGUF\"}", true),
        ] {
            let server = MockServer::start().await;
            let (root, svc, bytes, sha) = fixture();
            import_server(&server, &bytes, &sha, outcome, listed).await;
            svc.save_settings(RecognitionSettings {
                ollama_url: server.uri(),
                ..Default::default()
            })
            .unwrap();
            let original = root.path().join("model.gguf");
            tokio::fs::write(&original, &bytes).await.unwrap();
            svc.start_translation_import(original.clone()).unwrap();
            let job = done(&svc).await;
            assert_eq!(job.state, "failed");
            assert!(job.installed_model.is_none());
            assert!(!job.error.unwrap().contains("private"));
            assert_eq!(svc.settings().unwrap().ollama_model, "qwen3:4b");
            assert_eq!(tokio::fs::read(original).await.unwrap(), bytes);
        }
    }

    #[tokio::test]
    async fn gguf_cancel_does_not_switch_model_or_remove_original() {
        let server = MockServer::start().await;
        let (root, svc, bytes, _) = fixture();
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(1))
                    .set_body_json(serde_json::json!({"models":[]})),
            )
            .mount(&server)
            .await;
        svc.save_settings(RecognitionSettings {
            ollama_url: server.uri(),
            ..Default::default()
        })
        .unwrap();
        let original = root.path().join("model.gguf");
        tokio::fs::write(&original, &bytes).await.unwrap();
        svc.start_translation_import(original.clone()).unwrap();
        assert!(svc.start_translation_import(original.clone()).is_err());
        svc.cancel("translation").unwrap();
        assert_eq!(done(&svc).await.state, "cancelled");
        assert_eq!(svc.settings().unwrap().ollama_model, "qwen3:4b");
        assert_eq!(tokio::fs::read(original).await.unwrap(), bytes);

        // The alternative-download button also uses a synchronous Tauri command.
        // Cancel during the delayed local Ollama check, before requesting any model.
        let ui_service = svc.clone();
        std::thread::spawn(move || {
            assert!(tokio::runtime::Handle::try_current().is_err());
            ui_service.start_translation_alternative().unwrap();
            assert!(ui_service.start_translation_alternative().is_err());
            ui_service.cancel("translation").unwrap();
        }).join().unwrap();
        assert_eq!(done(&svc).await.state, "cancelled");
        assert_eq!(svc.settings().unwrap().ollama_model, "qwen3:4b");
    }

    #[tokio::test]
    async fn gguf_invalid_header_is_rejected_before_upload() {
        let (root, svc, _, _) = fixture();
        for bytes in [b"<html>not a model</html>".to_vec(), vec![0; 64]] {
            let file = root.path().join("fake.gguf");
            tokio::fs::write(&file, bytes).await.unwrap();
            let mut input = tokio::fs::File::open(file).await.unwrap();
            assert!(svc.hash_gguf(&mut input).await.is_err());
        }
        assert!(svc
            .start_translation_import(root.path().join("model.part"))
            .is_err());
        assert!(svc
            .start_translation_import(root.path().join("model-00001-of-00002.gguf"))
            .is_err());
        svc.save_settings(RecognitionSettings {
            ollama_model: "qwen3:8b".into(),
            ..Default::default()
        })
        .unwrap();
        assert!(svc.start_translation_alternative().is_err());
    }

    #[tokio::test]
    async fn gguf_embedding_only_model_is_not_selected_for_translation() {
        let server = MockServer::start().await;
        let (_root, svc, _, _) = fixture();
        Mock::given(method("POST"))
            .and(path("/api/show"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"capabilities":["embedding"]})),
            )
            .mount(&server)
            .await;
        let settings = RecognitionSettings {
            ollama_url: server.uri(),
            ..Default::default()
        };
        assert!(svc
            .verify_gguf_capabilities(&settings, "embedding:latest")
            .await
            .is_err());
        assert_eq!(svc.settings().unwrap().ollama_model, "qwen3:4b");
    }

    #[tokio::test]
    #[ignore = "Explicit network smoke test; fetches only the GGUF header from the pinned official source"]
    async fn gguf_official_source_header_smoke() {
        let client = reqwest::Client::builder()
            .https_only(true)
            .timeout(Duration::from_secs(45))
            .build()
            .unwrap();
        let response = client
            .get(QWEN_URL)
            .header(header::RANGE, "bytes=0-23")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            response.headers()[header::CONTENT_RANGE],
            format!("bytes 0-23/{QWEN_SIZE}")
        );
        let bytes = response.bytes().await.unwrap();
        assert_eq!(&bytes[..4], b"GGUF");
        assert_eq!(bytes.len(), 24);
    }
}
