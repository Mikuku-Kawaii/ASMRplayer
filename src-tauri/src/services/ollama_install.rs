//! Explicit, user-triggered Windows installation; never runs during startup or detection.
use super::recognition::ModelJob;
use crate::domain::{AppError, AppResult, ErrorCode};
use futures_util::StreamExt;
use parking_lot::Mutex;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tokio::{io::AsyncWriteExt, process::Command};

const INSTALLER_URL: &str = "https://ollama.com/download/OllamaSetup.exe";
const VERIFY_SIGNATURE_SCRIPT: &str = include_str!("verify_ollama_signature.ps1");

fn signature_result(code: Option<i32>) -> AppResult<()> {
    let message = match code {
        Some(0) => return Ok(()),
        Some(20) => "Ollama 安装程序发布者不匹配，已停止安装",
        Some(21) => "Ollama 安装程序没有数字签名（NotSigned），已停止安装",
        Some(22) => "Ollama 安装包内容与签名不一致（HashMismatch），已停止安装；请重新下载，若仍失败需等待官方修复安装包",
        Some(23) => "Windows 不信任 Ollama 安装包证书（NotTrusted）；请检查系统日期、Windows 更新和证书验证网络后重试",
        Some(24) => "下载的 Ollama 文件不是可验证的安装包（NotSupportedFileFormat），请检查下载网络后重试",
        Some(25) => "Ollama 安装包签名与当前 Windows 不兼容（Incompatible），请更新 Windows 后重试",
        Some(26) => "Windows 无法确认 Ollama 安装包签名（UnknownError），已停止安装；请检查系统日期和证书验证网络后重试",
        _ => "无法运行 Ollama 安装包签名检查，请检查 Windows PowerShell 是否可用后重试",
    };
    Err(error(message))
}
#[derive(Clone)]
pub struct OllamaInstaller {
    directory: PathBuf,
    job: Arc<Mutex<ModelJob>>,
    startup: Arc<tokio::sync::Mutex<()>>,
}
fn error(message: &str) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message)
}
fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    command
}
fn signature_command() -> Command {
    let powershell_directory =
        PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()))
            .join("System32/WindowsPowerShell/v1.0");
    let mut command = hidden_command(powershell_directory.join("powershell.exe"));
    // pnpm/dev shells can inherit PowerShell 7 modules. Windows PowerShell 5.1
    // must load its own Security module or Authenticode can fail before checking the file.
    command.env("PSModulePath", powershell_directory.join("Modules"));
    command
}
fn validate_base(base: &str) -> AppResult<url::Url> {
    let url = url::Url::parse(base).map_err(|_| error("Ollama 地址无效"))?;
    if url.scheme() != "http"
        || !matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(error(
            "自动安装只支持本机 HTTP 地址，例如 http://127.0.0.1:11434",
        ));
    }
    Ok(url)
}
async fn service_ready(base: &str) -> bool {
    let Ok(client) = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
    else {
        return false;
    };
    let Ok(response) = client
        .get(format!("{}/api/tags", base.trim_end_matches('/')))
        .send()
        .await
    else {
        return false;
    };
    if !response.status().is_success() {
        return false;
    }
    response
        .json::<serde_json::Value>()
        .await
        .ok()
        .is_some_and(|value| value["models"].is_array())
}
fn installed_cli() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(local).join("Programs/Ollama/ollama.exe"));
    }
    if let Some(paths) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&paths).map(|path| path.join("ollama.exe")));
    }
    candidates.into_iter().find(|path| path.is_file())
}
impl OllamaInstaller {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            job: Default::default(),
            startup: Default::default(),
        }
    }
    pub fn snapshot(&self) -> ModelJob {
        self.job.lock().clone()
    }
    /// Launch an existing local installation only. Never downloads or installs anything.
    pub async fn ensure_running(&self, base: &str) -> AppResult<()> {
        self.ensure_running_from(base, installed_cli()).await
    }
    async fn ensure_running_from(&self, base: &str, cli: Option<PathBuf>) -> AppResult<()> {
        validate_base(base)?;
        let _starting = self.startup.lock().await;
        let response = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .map_err(|_| error("无法检测 Ollama 服务"))?
            .get(format!("{}/api/tags", base.trim_end_matches('/')))
            .send()
            .await;
        match response {
            Ok(response) => {
                if !response.status().is_success() {
                    return Err(error(&format!(
                        "Ollama 服务返回 HTTP {}，请检查地址与服务状态",
                        response.status().as_u16()
                    )));
                }
                let value: serde_json::Value = response
                    .json()
                    .await
                    .map_err(|_| error("Ollama 服务返回格式无效"))?;
                if value["models"].is_array() {
                    return Ok(());
                }
                return Err(error("当前地址未返回 Ollama 模型列表"));
            }
            // On Windows a stopped loopback service can exhaust the connection
            // deadline rather than report ConnectionRefused. Both need startup.
            Err(err) if err.is_connect() || err.is_timeout() => {}
            Err(_) => return Err(error("Ollama 服务连接超时，请稍后重试")),
        }
        let cli =
            cli.ok_or_else(|| error("尚未安装 Ollama，请在设置中手动安装；不会自动下载模型"))?;
        Self::launch_service(base, &cli).await
    }
    pub fn start(&self, base: String) -> AppResult<()> {
        validate_base(&base)?;
        if !cfg!(windows) {
            return Err(error("自动安装 Ollama 仅支持 Windows"));
        }
        {
            let mut job = self.job.lock();
            if job.state == "running" {
                return Err(error("Ollama 正在安装，请等待完成"));
            }
            *job = ModelJob {
                state: "running".into(),
                label: "检测 Ollama 安装状态…".into(),
                ..Default::default()
            };
        }
        let service = self.clone();
        tokio::spawn(async move {
            let result = service.install(&base).await;
            let mut job = service.job.lock();
            match result {
                Ok(()) => {
                    job.state = "completed".into();
                    job.label = "Ollama 已就绪，可以下载翻译模型（安装时关闭代理软件）".into();
                }
                Err(err) => {
                    job.state = "failed".into();
                    job.error = Some(err.message);
                }
            }
        });
        Ok(())
    }
    fn stage(&self, label: &str) {
        let mut job = self.job.lock();
        job.label = label.into();
        job.completed = 0;
        job.total = None;
    }
    async fn download(&self, url: &str, destination: &Path) -> AppResult<()> {
        self.stage("正在下载 Ollama 安装程序…");
        let client = reqwest::Client::builder()
            .https_only(!cfg!(test))
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(600))
            .build()
            .map_err(|_| error("无法创建安装下载连接"))?;
        let response = client
            .get(url)
            .send()
            .await
            .and_then(|response| response.error_for_status())
            .map_err(|_| error("Ollama 安装程序下载失败，请检查网络后重试"))?;
        let total = response.content_length();
        self.job.lock().total = total;
        let mut file = tokio::fs::File::create(destination)
            .await
            .map_err(|_| error("无法保存 Ollama 安装程序，请检查磁盘空间和目录权限"))?;
        let mut stream = response.bytes_stream();
        let mut completed = 0;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| error("Ollama 安装程序下载中断，请重试"))?;
            file.write_all(&chunk)
                .await
                .map_err(|_| error("无法写入 Ollama 安装程序，请检查磁盘空间"))?;
            completed += chunk.len() as u64;
            self.job.lock().completed = completed;
        }
        file.sync_all()
            .await
            .map_err(|_| error("无法保存 Ollama 安装程序"))?;
        if completed == 0 || total.is_some_and(|size| size != completed) {
            return Err(error("Ollama 安装程序不完整，请重试"));
        }
        Ok(())
    }
    async fn verify_signature(path: &Path) -> AppResult<()> {
        // Pass the path as data, never interpolate it into PowerShell code.
        let status = tokio::time::timeout(
            Duration::from_secs(90),
            signature_command()
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    VERIFY_SIGNATURE_SCRIPT,
                ])
                .env("ASMR_OLLAMA_INSTALLER", path)
                .kill_on_drop(true)
                .status(),
        )
        .await
        .map_err(|_| error("Ollama 安装程序签名验证超时，请重试"))?
        .map_err(|_| error("无法验证 Ollama 安装程序签名"))?;
        signature_result(status.code())
    }
    async fn install(&self, base: &str) -> AppResult<()> {
        if service_ready(base).await {
            return Ok(());
        }
        let cli = if let Some(cli) = installed_cli() {
            cli
        } else {
            tokio::fs::create_dir_all(&self.directory)
                .await
                .map_err(|_| error("无法创建 Ollama 安装暂存目录"))?;
            let path = self.directory.join("OllamaSetup.exe");
            let result = async {
                self.download(INSTALLER_URL, &path).await?;
                self.stage("正在验证 Ollama 安装程序…");
                Self::verify_signature(&path).await?;
                self.stage("正在安装 Ollama，请稍候…");
                // The official per-user Inno installer requires no elevation and must not restart Windows.
                let status = hidden_command(&path)
                    .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-"])
                    .status()
                    .await
                    .map_err(|_| error("无法启动 Ollama 安装程序"))?;
                if !status.success() {
                    return Err(error(
                        "Ollama 安装未完成，请检查磁盘空间或关闭其他安装程序后重试",
                    ));
                }
                Ok(())
            }
            .await;
            // Only remove our downloaded installer, never an existing installation or model.
            let _ = tokio::fs::remove_file(&path).await;
            result?;
            installed_cli().ok_or_else(|| error("安装已结束，但未找到 Ollama 程序，请重试安装"))?
        };
        self.stage("正在启动并检测 Ollama 本地服务…");
        let _starting = self.startup.lock().await;
        Self::launch_service(base, &cli).await
    }
    async fn launch_service(base: &str, cli: &Path) -> AppResult<()> {
        if service_ready(base).await {
            return Ok(());
        }
        let url = validate_base(base)?;
        let host = format!(
            "{}:{}",
            url.host_str().unwrap_or("127.0.0.1"),
            url.port_or_known_default().unwrap_or(11434)
        );
        let mut child = hidden_command(cli)
            .arg("serve")
            .env("OLLAMA_HOST", host)
            .spawn()
            .map_err(|_| error("无法启动 Ollama，请检查安装目录权限"))?;
        for _ in 0..30 {
            if service_ready(base).await {
                // Ollama is a shared local service. Reap the child when it exits, without tying its lifetime to a view.
                tokio::spawn(async move {
                    let _ = child.wait().await;
                });
                return Ok(());
            }
            if child
                .try_wait()
                .map_err(|_| error("无法检测 Ollama 进程"))?
                .is_some()
            {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        let _ = child.kill().await;
        let _ = child.wait().await;
        Err(error(
            "Ollama 已安装，但本地服务未就绪，请检查地址、端口占用及 Ollama 日志后重试",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };
    #[tokio::test]
    async fn t1513_translation_startup_reuses_service_and_never_installs_missing_cli() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let installer = OllamaInstaller::new(dir.path().join("not-created"));
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"models":[]})),
            )
            .mount(&server)
            .await;
        installer
            .ensure_running_from(&server.uri(), None)
            .await
            .unwrap();
        assert!(!installer.directory.exists());
        assert!(installer.snapshot().state.is_empty());
        server.reset().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        assert!(installer
            .ensure_running_from(&server.uri(), None)
            .await
            .unwrap_err()
            .message
            .contains("503"));
        assert!(installer
            .ensure_running_from("http://example.invalid:11434", None)
            .await
            .is_err());
        server.reset().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(3)))
            .mount(&server)
            .await;
        assert!(installer
            .ensure_running_from(&server.uri(), None)
            .await
            .unwrap_err()
            .message
            .contains("手动安装"));
        let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", socket.local_addr().unwrap());
        drop(socket);
        assert!(installer
            .ensure_running_from(&address, None)
            .await
            .unwrap_err()
            .message
            .contains("手动安装"));
        assert!(!installer.directory.exists());
    }
    #[test]
    fn signature_failures_explain_the_actual_stage() {
        assert!(signature_result(Some(0)).is_ok());
        for (code, expected) in [
            (20, "发布者不匹配"),
            (21, "NotSigned"),
            (22, "HashMismatch"),
            (23, "NotTrusted"),
            (24, "NotSupportedFileFormat"),
            (25, "Incompatible"),
            (26, "UnknownError"),
            (27, "PowerShell"),
            (1, "PowerShell"),
        ] {
            assert!(signature_result(Some(code))
                .unwrap_err()
                .message
                .contains(expected));
        }
        assert!(signature_result(None).is_err());
    }
    #[cfg(windows)]
    #[tokio::test]
    async fn verifier_only_accepts_valid_ollama_signatures() {
        // Exercise the exact shipped script, with certificate outcomes supplied by a test double.
        for (state, subject, expected) in [
            ("Valid", "CN=Ollama Inc., O=Ollama Inc., C=CA", 0),
            ("Valid", "CN=Microsoft Corporation", 20),
            ("Valid", "", 20),
            ("NotSigned", "", 21),
            ("HashMismatch", "CN=Ollama Inc.", 22),
            ("NotTrusted", "CN=Ollama Inc.", 23),
            ("NotSupportedFileFormat", "", 24),
            ("Incompatible", "CN=Ollama Inc.", 25),
            ("UnknownError", "CN=Ollama Inc.", 26),
            ("throw", "", 27),
        ] {
            let script = format!(
                "function Get-AuthenticodeSignature {{ param($LiteralPath) if ($env:ASMR_TEST_STATUS -eq 'throw') {{ throw 'test failure' }}; [pscustomobject]@{{ Status=$env:ASMR_TEST_STATUS; SignerCertificate=[pscustomobject]@{{ Subject=$env:ASMR_TEST_SUBJECT }} }} }}\n{VERIFY_SIGNATURE_SCRIPT}"
            );
            let status = signature_command()
                .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                .env("ASMR_TEST_STATUS", state)
                .env("ASMR_TEST_SUBJECT", subject)
                .status()
                .await
                .unwrap();
            assert_eq!(status.code(), Some(expected), "{state}: {subject}");
        }
    }
    #[cfg(windows)]
    #[tokio::test]
    async fn loads_windows_signature_module_in_development_environment() {
        let status = signature_command()
            .args(["-NoProfile", "-NonInteractive", "-Command",
                "$ErrorActionPreference='Stop'; Import-Module Microsoft.PowerShell.Security; $null=Get-Command Get-AuthenticodeSignature -ErrorAction Stop; exit 0"])
            .status().await.unwrap();
        assert!(status.success());
    }
    #[test]
    fn only_local_install_targets_are_accepted() {
        for base in [
            "http://127.0.0.1:11434",
            "http://localhost:11435",
            "http://[::1]:11434",
        ] {
            assert!(validate_base(base).is_ok());
        }
        for base in [
            "https://example.com",
            "http://localhost:11434/proxy",
            "http://user@localhost:11434",
        ] {
            assert!(validate_base(base).is_err());
        }
    }
    #[tokio::test]
    async fn recognizes_existing_service_without_installation() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"models":[]})),
            )
            .mount(&server)
            .await;
        let root = tempfile::tempdir().unwrap();
        let installer = OllamaInstaller::new(root.path().join("not-created"));
        assert!(service_ready(&server.uri()).await);
        assert!(installer.snapshot().state.is_empty());
        assert!(!installer.directory.exists());
    }
    #[tokio::test]
    async fn download_failure_and_non_ollama_service_are_not_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        assert!(!service_ready(&server.uri()).await);
        let root = tempfile::tempdir().unwrap();
        let installer = OllamaInstaller::new(root.path().into());
        let destination = root.path().join("setup.exe");
        assert!(installer
            .download(&server.uri(), &destination)
            .await
            .is_err());
        assert!(!destination.exists());
    }
    #[cfg(windows)]
    #[tokio::test]
    async fn refuses_unsigned_installer() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("unsigned.exe");
        tokio::fs::write(&path, b"not an installer").await.unwrap();
        assert!(OllamaInstaller::verify_signature(&path).await.is_err());
    }

    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "manual official installer verification; does not run the installer"]
    async fn verifies_official_installer_without_installing() {
        let path = PathBuf::from(
            std::env::var_os("ASMR_OLLAMA_TEST_INSTALLER")
                .expect("set ASMR_OLLAMA_TEST_INSTALLER to an official installer"),
        );
        OllamaInstaller::verify_signature(&path).await.unwrap();
    }

    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "manual large official download; does not run the installer or download models"]
    async fn downloads_and_verifies_official_installer_without_installing() {
        let directory = PathBuf::from(
            std::env::var_os("ASMR_OLLAMA_DOWNLOAD_TEST_DIR")
                .expect("set ASMR_OLLAMA_DOWNLOAD_TEST_DIR to a disposable test directory"),
        );
        tokio::fs::create_dir_all(&directory).await.unwrap();
        let installer = OllamaInstaller::new(directory.clone());
        let path = directory.join("OllamaSetup.exe");
        installer.download(INSTALLER_URL, &path).await.unwrap();
        OllamaInstaller::verify_signature(&path).await.unwrap();
    }
}
