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

工作流保存在 `.github/workflows/release.yml`。当前发布使用本地签名构建，未向 GitHub 上传私钥。

要启用云端签名构建，由仓库维护者在 Actions Secrets 中配置 `TAURI_SIGNING_PRIVATE_KEY`；若私钥加密，另配 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。这些值不得写进工作流文件。

手动触发只构建并上传 Actions 附件，即使选择标签也不创建 Release。要让推送 `v*` 标签触发签名构建，需另设仓库变量 `SIGNED_RELEASE_CI=true`。标签推送触发时创建草稿供维护者审核，不会自动公开；已在本地发布过的标签不要再次运行创建 Release 步骤。缺少签名 Secret 会在安装依赖和下载运行库前报错。

启用顺序：先登录有仓库管理权限的 GitHub 账户，将已有发布私钥配置为 Actions Secret（使用 CLI 标准输入，不作为命令行参数或输出），加密私钥还需密码 Secret；将本次工作流和验签脚本提交到验收分支，手动运行并下载附件复核。确认云端产物与应用公钥、版本一致后，再启用 `SIGNED_RELEASE_CI`。未完成云端验收前继续使用本地签名发布。

流程从已发布 1.0.0 的 MSI 提取运行库，并校验安装包及两个 DLL 的固定 SHA-256。Release 正式发布后，更新元数据才可供客户端读取。

## 验收安排

本地验证包括 TypeScript、ESLint、前端与 Rust 回归测试和 Windows 正式打包。单元测试覆盖版本比较、取消与并发互斥、签名拒绝、乱序状态、WAL 备份和安装前会话保存。

1.1.1 发布安装包、元数据和必要源码。实际自动安装、重启和用户记录恢复需在独立 Windows 环境验收；GitHub Actions 云端构建也尚未验收。

### 带播放会话的冷启动回归

升级验收不能只测试空库或调试版。使用独立 Tauri identifier 构建 release 程序，在独立配置目录中准备旧版本数据库副本，保留暂停的当前曲目、进度和灵动岛开启设置；禁用副本的下载、目录扫描、自动备份和自动更新。不得直接使用正式配置目录运行会强制终止进程的测试。

对隔离程序至少连续运行三次：

```powershell
pwsh -NoProfile -File scripts/test-startup-responsiveness.ps1 -Executable <隔离程序绝对路径> -ReportPath <测试报告路径> -DurationSeconds 20
```

脚本仅探测并结束自己启动的进程；窗口必须出现，且持续响应 Windows 消息。另行确认曲目和进度仍然保留、恢复时保持暂停。测试副本及报告不提交。该检查覆盖页面提前请求灵动岛布局与宿主窗口初始化之间的竞争：初始化不得在 UI 线程等待布局操作锁，否则持锁线程查询屏幕时会与 UI 线程互相等待。

窗口验收同时检查：设置中的界面缩放在 50%、100%、150% 下即时生效，重启后保持选择，灵动岛与桌面歌词保持各自尺寸；最大化时双击标题栏只还原一次，再次双击只最大化一次，右上角图标同步更新。界面缩放是应用附加比例，100% 仍遵循当前显示器的 Windows DPI。


