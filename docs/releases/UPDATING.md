# 签名更新与发布

应用从 `https://github.com/Mikuku-Kawaii/ASMRplayer/releases/latest/download/latest.json` 检查正式版本。版本按 SemVer 比较，排除预发布、草稿、相同版本和降级。网络失败或缺少元数据会显示错误。

更新由 Rust UpdateService 管理，设置页和导航提醒订阅同一快照。下载、取消、签名验证与安装互斥。用户点击“安装并重启”后才暂停播放、保存会话、检查点数据库并运行安装器；验证失败不执行安装包。

## 本地发布准备

1. 同步修改 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` 的版本，并更新 Cargo.lock；添加 `docs/releases/v版本.md`。
2. 发布者私钥保存在本机 `.release-signing/updater.key`（已被 Git 忽略），公钥写入 Tauri 配置。后续版本必须沿用相同密钥。私钥及该目录不得上传到源码仓库或 Release。
3. 运行 `powershell -ExecutionPolicy Bypass -File scripts/prepare-release.ps1`。也支持从环境变量 `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 提供密钥。
4. 将 `output/release/v版本/` 中的 EXE、MSI 和 `latest.json` 上传到对应正式 Release。许可声明保留在仓库；无需额外上传校验表、许可 ZIP 或独立通知附件。

`latest.json` 包含 Windows x64 的 NSIS/MSI 平台映射和签名，不需要另外上传 `.sig`。不要手写签名或混用不同版本的安装包。

生成清单前会使用 Tauri 配置中的更新公钥验证每个安装包的 Minisign 签名、受签名保护的注释与版本。安装包被修改、密钥不匹配或签名版本与发布版本不同都会中止清单生成。

自行构建的开发者需要生成自己的密钥，并替换 Tauri 公钥与更新仓库地址；该密钥不能用于官方客户端更新。

## GitHub Actions

工作流保存在 `.github/workflows/release.yml`。仓库已配置更新签名 Actions Secret；支持云端签名构建，同时保留本地签名发布。私钥不进入源码、构建附件或 Release。

要启用云端签名构建，由仓库维护者在 Actions Secrets 中配置 `TAURI_SIGNING_PRIVATE_KEY`；若私钥加密，另配 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。这些值不得写进工作流文件。

手动触发只构建并上传 Actions 附件，即使选择标签也不创建 Release。要让推送 `v*` 标签触发签名构建，需另设仓库变量 `SIGNED_RELEASE_CI=true`。标签推送触发时创建草稿供维护者审核，不会自动公开；已在本地发布过的标签不要再次运行创建 Release 步骤。缺少签名 Secret 会在安装依赖和下载运行库前报错。

启用顺序：先登录有仓库管理权限的 GitHub 账户，将已有发布私钥配置为 Actions Secret（使用 CLI 标准输入，不作为命令行参数或输出），加密私钥还需密码 Secret；将本次工作流和验签脚本提交到验收分支，手动运行并下载附件复核。确认云端产物与应用公钥、版本一致后，再启用 `SIGNED_RELEASE_CI`。未完成云端验收前继续使用本地签名发布。

流程从已发布 1.0.0 的 MSI 提取运行库，并校验安装包及两个 DLL 的固定 SHA-256。Release 正式发布后，更新元数据才可供客户端读取。

## 验收安排

本地验证包括 TypeScript、ESLint、前端与 Rust 回归测试和 Windows 正式打包。单元测试覆盖版本比较、取消与并发互斥、签名拒绝、乱序状态、WAL 备份和安装前会话保存。

2026-10-15 完成 [Windows 云端签名构建验收](https://github.com/Mikuku-Kawaii/ASMRplayer/actions/runs/38722734680)：下载附件后核验 EXE/MSI 与应用公钥、签名版本、清单平台映射及 SHA-256 一致，手动触发跳过 Release 创建。自动安装、重启和用户记录恢复仍需在独立 Windows 环境验收。

### 带播放会话的冷启动回归

升级验收不能只测试空库或调试版。使用独立 Tauri identifier 构建 release 程序，在独立配置目录中准备旧版本数据库副本，保留暂停的当前曲目、进度和灵动岛开启设置；禁用副本的下载、目录扫描、自动备份和自动更新。不得直接使用正式配置目录运行会强制终止进程的测试。

对隔离程序至少连续运行三次：

```powershell
pwsh -NoProfile -File scripts/test-startup-responsiveness.ps1 -Executable <隔离程序绝对路径> -ReportPath <测试报告路径> -DurationSeconds 20
```

脚本仅探测并结束自己启动的进程；窗口必须出现，且持续响应 Windows 消息。另行确认曲目和进度仍然保留、恢复时保持暂停。测试副本及报告不提交。该检查覆盖页面提前请求灵动岛布局与宿主窗口初始化之间的竞争：初始化不得在 UI 线程等待布局操作锁，否则持锁线程查询屏幕时会与 UI 线程互相等待。

窗口验收同时检查：设置中的界面缩放在 50%、100%、150% 下即时生效，重启后保持选择，灵动岛与桌面歌词保持各自尺寸；最大化时双击标题栏只还原一次，再次双击只最大化一次，右上角图标同步更新。界面缩放是应用附加比例，100% 仍遵循当前显示器的 Windows DPI。



## Android 与 PC 共用更新清单

Android 和 Windows 读取同一个 `releases/latest/download/latest.json`。保留原 `windows-x86_64` / NSIS / MSI 条目，增加 `platforms.android-universal`：

```json
{
  "signature": "Tauri base64 Minisign signature",
  "url": "https://github.com/Mikuku-Kawaii/ASMRplayer/releases/download/v1.4.0/ASMR.Player_1.4.0_14_universal.apk",
  "versionCode": 14,
  "minSdk": 23,
  "size": 12345678,
  "sha256": "64 lowercase hex characters"
}
```

这是字段示例，不是已发布版本。Android 版本名从根 package.json 生成，Tauri 公钥从 tauri.conf.json 编入 BuildConfig，避免两套配置漂移。每次共同发布先同步 package.json、Cargo.toml、tauri.conf.json 的稳定版本；Android 另外指定大于上一发行版的 versionCode。当前纯 Kotlin/Java 包使用 universal APK；不向清单提供 AAB。应用安装身份必须一直使用同一包名和 Android keystore；Android 证书不能用 PC 更新私钥代替。

本地准备（PowerShell 7，JDK 17、Android SDK 36 和 build-tools 已安装）：

1. 按既有流程准备 Windows 发行包。设置 `ANDROID_HOME`、`JAVA_HOME`，以及 `ANDROID_KEYSTORE_FILE`、`ANDROID_KEYSTORE_PASSWORD`、`ANDROID_KEY_ALIAS`、`ANDROID_KEY_PASSWORD`。keystore 和密码保存在私有位置，不进入源码或日志。
2. 设置已有更新签名的 `TAURI_SIGNING_PRIVATE_KEY_PATH`（或 `TAURI_SIGNING_PRIVATE_KEY`）及必要的 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。脚本不生成/替换现有正式密钥。
3. 执行 `pwsh -File scripts/prepare-android-release.ps1 -VersionCode <递增整数>`。脚本构建签名 release APK，以 apksigner 验证，读取实际包名、版本和 minSdk，拒绝 debuggable 包，再调用 `tauri signer sign --app-version`；验签后合并到同版本 `output/release/v版本/latest.json`，保留 Windows 条目，并刷新 SHA256SUMS。
4. PC 与 Android 的本地构建顺序可以互换：PC 清单生成器会重新验证同目录 android-release.json 对应的 APK 后保留 Android 条目。该文件是本地构建元数据，不必上传。准备两端同版本产物后一起上传 EXE/MSI/APK 和唯一的 latest.json 到同一 Release，确认后发布。

GitHub Actions 保留默认 Windows 构建，并支持可选联合发行：手动运行勾选 `build_android`，填写递增的 `android_version_code`（或仓库变量 `ANDROID_VERSION_CODE`）；标签构建使用仓库变量 `ANDROID_SIGNED_RELEASE_CI=true` 启用 Android，同时保留原 `SIGNED_RELEASE_CI` 总开关。

启用前配置 `ANDROID_KEYSTORE_BASE64`、`ANDROID_KEYSTORE_PASSWORD`、`ANDROID_KEY_ALIAS`、`ANDROID_KEY_PASSWORD` 四个 Secret，复用既有 Tauri 更新签名 Secret。keystore 必须是持续使用的正式安装密钥，不因新版本更换。工作流提前检查配置，安装 JDK 17/SDK 36，运行 Android JVM/lint 与清单测试，再构建签名 APK 并合并已有 Windows latest.json；临时 keystore 在 finally 中删除，不进 artifact。联合 artifact 名称为 signed-windows-android-release，纯 Windows 名称保持不变。标签运行仍只创建待审查 draft，手动运行仍不创建 Release。

本地实现不等于远端已启用：本轮没有写入 GitHub Secrets、开启变量或发布版本。启用前先手动构建核验附件与安装升级；实际远端验收另行记录。若只发布 Windows 清单，Android 不会误报更新；不要在已有联合 Release 中用缺少 Android 条目的清单覆盖它。

Android 正式版启动及约每 24 小时自动检查，下载由用户发起，安装先保存播放会话与数据库，再交给系统安装器确认；REQUEST_INSTALL_PACKAGES 授权入口通过后端返回给 UI。自动下载/静默安装不在此契约内。安装器仍会验证 APK；包签名不一致需正确签名重发，不能通过卸载应用绕过（卸载会删除数据）。发布到 Google Play 时需采用对应商店更新渠道，当前实现面向 GitHub APK 分发。
