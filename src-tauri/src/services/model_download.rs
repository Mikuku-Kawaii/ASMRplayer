//! Resumable, hash-verified downloads for pinned recognition assets.
use super::*;
fn model_transfer_error(e: reqwest::Error) -> AppError {
    AppError::new(
        ErrorCode::NetworkUnavailable,
        if e.is_timeout() {
            "模型下载长时间未收到数据；已下载部分保留，再次下载可续传"
        } else {
            "模型下载连接中断；已下载部分保留，再次下载可续传"
        },
    )
}
async fn checksum(path: &Path) -> AppResult<String> {
    let mut file = tokio::fs::File::open(path).await.map_err(io_error)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let n = file.read(&mut buffer).await.map_err(io_error)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(hex::encode(hash.finalize()))
}

impl RecognitionService {
    pub(super) async fn download_model_asset(
        &self,
        target: &Path,
        url: &str,
        expected_size: u64,
        expected_sha: &str,
    ) -> AppResult<()> {
        let baseline = self.recognition.lock().job.completed;
        let label = self.recognition.lock().job.label.clone();
        for attempt in 0..3 {
            self.recognition.lock().job.completed = baseline;
            let result = self
                .download_model_asset_once(target, url, expected_size, expected_sha)
                .await;
            match result {
                Err(e) if matches!(e.code, ErrorCode::NetworkUnavailable) && attempt < 2 => {
                    self.recognition.lock().job.label =
                        format!("连接中断，正在续传（{}/2） · {}", attempt + 1, label);
                    tokio::time::sleep(Duration::from_secs(2 * (attempt + 1))).await;
                }
                Err(mut e) => {
                    e.message = format!(
                        "{}：{}",
                        target.file_name().unwrap_or_default().to_string_lossy(),
                        e.message
                    );
                    return Err(e);
                }
                Ok(()) => return Ok(()),
            }
        }
        unreachable!()
    }
    async fn download_model_asset_once(
        &self,
        target: &Path,
        url: &str,
        expected_size: u64,
        expected_sha: &str,
    ) -> AppResult<()> {
        if target.metadata().is_ok_and(|m| m.len() == expected_size)
            && checksum(&target).await? == expected_sha
        {
            self.recognition.lock().job.completed += expected_size;
            return Ok(());
        }
        let partial = target.with_extension("part");
        let mut offset = partial.metadata().map(|m| m.len()).unwrap_or(0);
        if offset > expected_size {
            offset = 0;
        }
        if offset == expected_size {
            if checksum(&partial).await? == expected_sha {
                tokio::fs::rename(&partial, &target)
                    .await
                    .map_err(io_error)?;
                self.recognition.lock().job.completed += offset;
                return Ok(());
            }
            offset = 0;
        }
        let mut request = Self::download_client(false)?.get(url);
        if offset > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={offset}-"));
        }
        let response = Self::checked(request.send().await.map_err(model_transfer_error)?).await?;
        if response.status() == reqwest::StatusCode::PARTIAL_CONTENT {
            super::sensevoice_fp16_assets::validate_range(
                response
                    .headers()
                    .get(reqwest::header::CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok()),
                offset,
                expected_size,
            )?;
        } else if response.status() == reqwest::StatusCode::OK {
            offset = 0;
        } else {
            return Err(invalid("模型下载响应无效，请重试"));
        }
        let mut output = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(offset > 0)
            .truncate(offset == 0)
            .open(&partial)
            .await
            .map_err(io_error)?;
        self.recognition.lock().job.completed += offset;
        let mut size = offset;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(model_transfer_error)?;
            size += chunk.len() as u64;
            if size > expected_size {
                return Err(invalid("模型下载文件超出预期大小"));
            }
            output.write_all(&chunk).await.map_err(io_error)?;
            self.recognition.lock().job.completed += chunk.len() as u64;
        }
        output.sync_all().await.map_err(io_error)?;
        drop(output);
        verify_file(
            size,
            &checksum(&partial).await?,
            expected_size,
            expected_sha,
        )?;
        tokio::fs::rename(&partial, &target)
            .await
            .map_err(io_error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    async fn request(socket: &mut tokio::net::TcpStream) -> String {
        let mut bytes = Vec::new();
        while !bytes.ends_with(b"\r\n\r\n") {
            bytes.push(socket.read_u8().await.unwrap());
        }
        String::from_utf8(bytes).unwrap()
    }

    #[tokio::test]
    async fn t1513_model_download_deadline_is_idle_not_total() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/model", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            request(&mut socket).await;
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            for _ in 0..8 {
                tokio::time::sleep(Duration::from_millis(100)).await;
                socket.write_all(b"x").await.unwrap();
            }
            let (mut stalled, _) = listener.accept().await.unwrap();
            request(&mut stalled).await;
            stalled
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nx")
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(900)).await;
        });
        let client =
            RecognitionService::client_with_read_timeout(true, Duration::from_millis(500)).unwrap();
        let started = std::time::Instant::now();
        assert_eq!(
            client
                .get(&url)
                .send()
                .await
                .unwrap()
                .bytes()
                .await
                .unwrap()
                .as_ref(),
            b"xxxxxxxx"
        );
        assert!(started.elapsed() > Duration::from_millis(500));
        assert!(client
            .get(&url)
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap_err()
            .is_timeout());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn t1513_model_download_reconnects_and_retains_verified_cache() {
        let root = tempfile::tempdir().unwrap();
        let db =
            Database::open_and_migrate(&root.path().join("d"), &root.path().join("c")).unwrap();
        let service = RecognitionService::new(db, root.path().join("d"), "resume-test");
        let target = root.path().join("model.bin");
        let sha = hex::encode(Sha256::digest(b"abcdef"));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/model", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut first, _) = listener.accept().await.unwrap();
            assert!(!request(&mut first)
                .await
                .to_ascii_lowercase()
                .contains("range:"));
            first
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nabc")
                .await
                .unwrap();
            drop(first);
            let (mut second, _) = listener.accept().await.unwrap();
            assert!(request(&mut second)
                .await
                .to_ascii_lowercase()
                .contains("range: bytes=3-"));
            second.write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 3-5/6\r\nConnection: close\r\n\r\ndef").await.unwrap();
        });
        service
            .download_model_asset(&target, &url, 6, &sha)
            .await
            .unwrap();
        server.await.unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"abcdef");
        assert_eq!(service.recognition.lock().job.completed, 6);
        assert!(!target.with_extension("part").exists());
        // The server is closed: complete validated files must not reconnect.
        service.recognition.lock().job.completed = 0;
        service
            .download_model_asset(&target, &url, 6, &sha)
            .await
            .unwrap();
        assert_eq!(service.recognition.lock().job.completed, 6);
    }

    #[tokio::test]
    async fn t1513_model_download_cancellation_preserves_partial_for_next_job() {
        let root = tempfile::tempdir().unwrap();
        let db =
            Database::open_and_migrate(&root.path().join("d"), &root.path().join("c")).unwrap();
        let service = RecognitionService::new(db, root.path().join("d"), "cancel-test");
        let target = root.path().join("model.bin");
        let sha = hex::encode(Sha256::digest(b"abcdef"));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/model", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut first, _) = listener.accept().await.unwrap();
            request(&mut first).await;
            first
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nabc")
                .await
                .unwrap();
            let (mut second, _) = listener.accept().await.unwrap();
            assert!(request(&mut second)
                .await
                .to_ascii_lowercase()
                .contains("range: bytes=3-"));
            second.write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 3-5/6\r\nConnection: close\r\n\r\ndef").await.unwrap();
        });
        let (svc, path, endpoint, digest) =
            (service.clone(), target.clone(), url.clone(), sha.clone());
        let transfer =
            tokio::spawn(
                async move { svc.download_model_asset(&path, &endpoint, 6, &digest).await },
            );
        tokio::time::timeout(Duration::from_secs(5), async {
            while service.recognition.lock().job.completed < 3 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        transfer.abort();
        assert!(transfer.await.unwrap_err().is_cancelled());
        assert!(!target.exists());
        assert_eq!(
            std::fs::read(target.with_extension("part")).unwrap(),
            b"abc"
        );
        service.recognition.lock().job.completed = 0;
        service
            .download_model_asset(&target, &url, 6, &sha)
            .await
            .unwrap();
        server.await.unwrap();
        assert_eq!(std::fs::read(target).unwrap(), b"abcdef");
        assert_eq!(service.recognition.lock().job.completed, 6);
    }
}
