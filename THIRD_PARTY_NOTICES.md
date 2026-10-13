# 第三方组件声明

ASMR Player 1.1.0 使用以下第三方组件。本次发布提供 Windows x64 安装包、项目源码及功能说明。

## 播放运行库

当前播放器通过 libmpv C API 动态加载 `mpv-2.dll`，运行于应用宿主内。安装包同时包含 `vulkan-1.dll`。

| 文件 | 实测信息 | SHA-256 |
| --- | --- | --- |
| `mpv-2.dll` | Windows x64；文件版本 `v0.41.0-1151-g9d144e57b` | `5080e00cd114f3dfc0eb87f35068fec70afc8bfd6fc29dca4b79c874a42485ee` |
| `vulkan-1.dll` | Vulkan Loader；Windows x64 | `2baae0a109cb5962437b00a182750308ce9f69f59381df62939c00c21150ee6b` |

- mpv 上游：[mpv-player/mpv](https://github.com/mpv-player/mpv)。[上游 Copyright](https://github.com/mpv-player/mpv/blob/master/Copyright) 说明其默认许可为 GPL-2.0-or-later；排除 GPL 专属代码的构建可适用 LGPL-2.1-or-later，链接依赖也会影响最终许可。仓库随附 [Copyright](licenses/mpv-Copyright.txt)、[GPL 文本](licenses/mpv-GPL-2.0.txt) 和 [LGPL 文本](licenses/mpv-LGPL-2.1.txt)。
- 当前 DLL 的嵌入构建配置包含 `mpv-winbuild-cmake` 路径和 `-Dlibmpv=true`。原始下载地址、完整构建配方和依赖许可清单未随现有打包产物留存；不能据此将整个运行库声明为纯 LGPL 构建，也不能将其对应到旧文档中的另一个 mpv 压缩包或哈希。
- Vulkan Loader 上游：[KhronosGroup/Vulkan-Loader](https://github.com/KhronosGroup/Vulkan-Loader)。其主要代码采用 Apache-2.0，少量文件采用上游注明的更宽松许可；随附 [上游许可文本](licenses/Vulkan-Loader-LICENSE.txt)。

第三方运行库保留各自权利人的版权与许可，以上记录不替代相应许可中的分发要求。

## 主要源码依赖

| 组件 | 用途 | 上游许可 |
| --- | --- | --- |
| Tauri / tauri-build / tauri-plugin-dialog / tauri-plugin-updater | 桌面宿主、窗口、托盘、系统对话框与签名更新 | Apache-2.0 OR MIT |
| React / React DOM | 用户界面 | MIT |
| TypeScript | 类型检查 | Apache-2.0 |
| Vite / Vitest / ESLint | 构建、测试与静态检查 | MIT |
| lucide-react | 界面图标 | ISC |
| Tokio | 异步运行时 | MIT |
| rusqlite | SQLite 访问 | MIT；SQLite 核心为 Public Domain |
| reqwest / serde / serde_json | HTTP 与数据序列化 | Apache-2.0 OR MIT |
| lofty / keyring | 音频标签与系统凭据存储 | Apache-2.0 OR MIT |

这是一份主要组件索引，不是全部传递依赖清单。其他 Rust crate、npm 包及其依赖的许可应以对应版本随附的 `LICENSE` / `NOTICE` 与上游元数据为准。

## 在线内容与项目源码

在线作品信息、封面、音频与字幕属于其各自权利人。本项目源码和安装包不包含用户作品音频。

项目开发源码已随 1.1.0 发布到本仓库。此声明不为第三方组件或在线内容授予额外权利。
