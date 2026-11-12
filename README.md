# ASMR Player

ASMR Player 是一款面向 Windows 和 Android 的音声播放器。可以从本地文件夹建立音频库，也可以浏览在线作品。两端可同步作品、收藏音轨和收听进度，在另一台设备上找到上次听到的位置。

[下载最新版本](https://github.com/Mikuku-Kawaii/ASMRplayer/releases/latest) · [26.11.12 更新说明](docs/releases/v26.11.12.md) · [反馈问题](https://github.com/Mikuku-Kawaii/ASMRplayer/issues)

## 选择安装包

| 设备 | 下载文件 | 安装方式 |
| --- | --- | --- |
| Windows x64 | 以 `x64-setup.exe` 结尾的文件 | 双击运行安装向导。需要 MSI 的用户可下载同一版本的 `.msi`。 |
| Android 6.0 及以上 | 以 `universal.apk` 结尾的文件 | 在手机上打开 APK，按系统提示允许安装。 |

## 从一部作品开始

1. **添加作品。** 在音频库添加本地目录，或在在线浏览中选择作品。Android 首次导入时，在系统文件选择器里授予目录访问权限。
2. **整理想听的内容。** 把作品加入音频库，在喜欢页查看点过心形的作品。可以单独收藏音轨、将音轨加入歌单，或为某个片段保存时间标记。
3. **配合字幕收听。** 为音轨关联原文和译文，按需调整时间偏移。在手机播放页点封面可展开台本，点句子即可跳转。Windows 还可将字幕放到独立的桌面歌词窗口。
4. **换设备继续。** 在两端打开「设置 → 跨设备同步」，选择同一存储服务并绑定同一账号，然后点击同步。本地音频文件需在目标设备另行导入或授权。[同步范围与绑定说明](docs/releases/DEVICE_SYNC.md)

## 两端能做什么

| 场景 | Windows | Android |
| --- | --- | --- |
| 管理与播放 | 本地目录扫描、在线浏览、下载、歌单和播放队列 | 系统目录授权、在线浏览、下载、歌单和后台播放 |
| 字幕 | 双语台本、桌面歌词，可配置语音识别和翻译 | 导入及下载现有字幕、双语台本、原文与译文时间轴 |
| 播放控制 | 桌面灵动岛、系统媒体控制、快捷键 | 迷你播放器、锁屏与通知媒体控制、桌面小组件 |
| 音效与助眠 | 均衡器、沉浸设置、睡眠定时 | 均衡器、耳机音效方案、睡眠定时 |

跨设备同步支持 GitHub Gist、OneDrive、WebDAV 和 Supabase。各方式独立绑定。Supabase 邮箱注册的确认邮件受当前邮件服务的投递限制。

## 从源码运行

Windows 客户端使用 Tauri 2、React、Rust、SQLite 和 libmpv。准备 Node.js、pnpm、Rust 和 Visual Studio C++ 构建工具后，在仓库根目录运行：

```powershell
pnpm install --frozen-lockfile
./scripts/fetch-runtime.ps1
pnpm tauri dev
```

Android 客户端使用 Kotlin、Jetpack Compose、Media3 和 Room。准备 JDK 17、Android SDK 36 后，在根目录执行 `pnpm install --frozen-lockfile` 和 `.\android\gradlew.bat -p android :app:assembleDebug`。也可以用 Android Studio 打开 `android/`。

[Android 开发说明](android/README.md) · [签名与更新机制](docs/releases/UPDATING.md) · [开发计划](docs/releases/TODO.md)
