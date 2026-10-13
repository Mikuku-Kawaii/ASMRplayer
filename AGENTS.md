# 项目维护说明

## 当前实现

- Windows 桌面应用，使用 Tauri 2、React 18、TypeScript、Rust 和 SQLite。
- 播放器通过 `src-tauri/src/player/libmpv_ffi.rs` 动态加载 `mpv-2.dll`，使用进程内 libmpv C API。旧技能中有关 mpv sidecar 与 Windows Named Pipe IPC 的描述属于历史方案，修改时以当前源码为准。
- Rust 服务负责播放、队列、会话、下载、数据库与更新状态；React 负责展示和发送用户操作。主窗口与桌面灵动岛共享后端播放状态。

## 项目技能

- 架构、播放、下载、库扫描与 Tauri 命令修改：阅读 [.agents/skills/asmr-player-architecture/SKILL.md](.agents/skills/asmr-player-architecture/SKILL.md)。
- React、样式、文案与交互修改：阅读 [.agents/skills/asmr-player-m3-ui-standards/SKILL.md](.agents/skills/asmr-player-m3-ui-standards/SKILL.md)。
- 音频保真、DSP、EQ 与音效控件修改：阅读 [.agents/skills/asmr-player-pro-audio-dsp/SKILL.md](.agents/skills/asmr-player-pro-audio-dsp/SKILL.md)。

## 验证与发布

- 常规检查：`pnpm typecheck`、`pnpm lint`、`pnpm test`、`cargo test --locked --manifest-path src-tauri/Cargo.toml`。
- 本地运行库准备：`scripts/fetch-runtime.ps1`。签名发布流程：`scripts/prepare-release.ps1`，详见 [docs/releases/UPDATING.md](docs/releases/UPDATING.md)。
- 私钥、用户媒体、数据库、日志、缓存和生成产物不得提交；测试音频只保留 `tests/fixtures/audio/` 中的合成夹具。
- 更新包必须通过签名验证，安装由用户明确点击触发，安装前保存会话与数据库状态。实际升级测试应使用独立环境并保留用户数据。
