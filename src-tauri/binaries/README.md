# Windows x64 播放运行库

ASMR Player 通过 libmpv C API 在 Rust 宿主中播放音频。开发与打包需要下列文件：

| 文件 | 用途 |
| --- | --- |
| `mpv-2.dll` | libmpv 音频解码与播放 |
| `vulkan-1.dll` | 该构建使用的 Vulkan Loader 依赖 |

第三方二进制不提交到源码仓库。安装 [ASMR Player Release](https://github.com/Mikuku-Kawaii/ASMRplayer/releases/latest) 后，从应用安装目录复制以上两个 DLL 到本目录，再运行 `pnpm tauri dev` 或 `pnpm tauri build`。

`tauri.conf.json` 将两个 DLL 作为资源打包到安装目录。只复制 `asmr-player.exe` 无法提供完整播放运行环境。

1.1.0 使用的 libmpv 文件版本为 `v0.41.0-1151-g9d144e57b`，文件校验值及第三方许可说明见 [THIRD_PARTY_NOTICES.md](../../THIRD_PARTY_NOTICES.md)。
