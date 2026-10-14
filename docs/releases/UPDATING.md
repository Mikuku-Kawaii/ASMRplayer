# 签名更新与发布

应用从 `https://github.com/Mikuku-Kawaii/ASMRplayer/releases/latest/download/latest.json` 检查正式版本。版本按 SemVer 比较，排除预发布、草稿、相同版本和降级。网络失败或缺少元数据会显示错误。

更新由 Rust UpdateService 管理，设置页和导航提醒订阅同一快照。下载、取消、签名验证与安装互斥。用户点击“安装并重启”后才暂停播放、保存会话、检查点数据库并运行安装器；验证失败不执行安装包。

## 本地发布准备

1. 同步修改 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` 的版本，并更新 Cargo.lock；添加 `docs/releases/v版本.md`。
2. 发布者私钥保存在本机 `.release-signing/updater.key`（已被 Git 忽略），公钥写入 Tauri 配置。后续版本必须沿用相同密钥。私钥及该目录不得上传到源码仓库或 Release。
3. 运行 `powershell -ExecutionPolicy Bypass -File scripts/prepare-release.ps1`。也支持从环境变量 `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 提供密钥。
4. 将 `output/release/v版本/` 中的 EXE、MSI 和 `latest.json` 上传到对应正式 Release。许可声明保留在仓库；无需额外上传校验表、许可 ZIP 或独立通知附件。

`latest.json` 包含 Windows x64 的 NSIS/MSI 平台映射和签名，不需要另外上传 `.sig`。不要手写签名或混用不同版本的安装包。

自行构建的开发者需要生成自己的密钥，并替换 Tauri 公钥与更新仓库地址；该密钥不能用于官方客户端更新。

## GitHub Actions

工作流保存在 `.github/workflows/release.yml`。当前发布使用本地签名构建，未向 GitHub 上传私钥。

要启用云端签名构建，由仓库维护者在 Actions Secrets 中配置 `TAURI_SIGNING_PRIVATE_KEY`；若私钥加密，另配 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。这些值不得写进工作流文件。

手动触发可构建并上传 Actions 附件。要让 `v*` 标签触发签名构建，需另设仓库变量 `SIGNED_RELEASE_CI=true`。标签触发时创建草稿供维护者审核，不会自动公开；已在本地发布过的标签不要再次运行创建 Release 步骤。

流程从已发布 1.0.0 的 MSI 提取运行库，并校验安装包及两个 DLL 的固定 SHA-256。Release 正式发布后，更新元数据才可供客户端读取。

## 验收安排

本地验证包括 TypeScript、ESLint、前端与 Rust 回归测试和 Windows 正式打包。单元测试覆盖版本比较、取消与并发互斥、签名拒绝、乱序状态、WAL 备份和安装前会话保存。

1.1.1 发布安装包、元数据和必要源码。实际自动安装、重启和用户记录恢复需在独立 Windows 环境验收；GitHub Actions 云端构建也尚未验收。


