---
name: asmr-player-architecture
description: "ASMR Player architecture principles, domain invariants, and engineering guardrails. MUST USE for any architecture design request, and whenever working on PlaybackService, MpvController (Windows Named Pipe IPC), four-layer content/version modeling (works/tracks/versions/sources), AsmrOneProvider, DownloadService (Range 206 resume & .part safety), LibraryService scanning, Tauri commands/events, or React player control surfaces."
---

# ASMR Player Architecture & Engineering Guardrails

## When To Load This Skill

Load this skill for any architecture design, database schema change, or cross-layer refactor, without exception.

Also load it for:

- `PlaybackService` state machine, `session_generation` race control, queue auto-advance, seek/resume, A–B loop, or sleep timer work.
- `MpvController` sidecar process lifecycle, Windows Named Pipe JSON IPC (`\\.\pipe\asmrplayer-mpv-<uuid>`), `request_id` correlation, property observation, or EOF/crash recovery.
- Four-layer domain & SQLite modeling (`works` → `tracks` → `track_versions` → `media_sources`, plus `work_overrides` and `playback_positions`).
- Local library import, incremental scanning, natural sorting, root relocation, or directory group queueing (`LibraryService`).
- `asmr.one` online integration (`AsmrOneProvider`), RJ/source work ID normalization, remote tree parsing, on-demand media URL resolution, or error classification/retry.
- Offline download pipeline (`DownloadService`), `.part` file isolation, HTTP Range `206`/`200`/`416` handling, Windows path sanitization, or atomic rename + DB transactions.
- Tauri IPC bridge additions (`src-tauri/src/commands/mod.rs`, `src/lib/api.ts`) and React control plane views (`src/app/App.tsx`, `src/features/*`).

## Purpose

This skill enforces a strict **host-centered audio and session architecture** for **ASMR Player** (Windows x64 desktop player built with Tauri 2 + Rust + SQLite + `mpv` sidecar + React 18 + TypeScript):

- **Rust Backend + `mpv` Sidecar** is the single backend authority for media resolution, decoding, audio output, playback state, queue progression, sleep timers, A–B loops, downloads, and SQLite persistence.
- **React Frontend** is strictly a **control plane**: it dispatches user intent via typed Tauri `invoke` commands and renders `PlaybackSnapshot` state pushed via Tauri events (`playback:snapshot`) guarded by monotonic `eventSeq`.
- **Four-layer content modeling** (`works` → `tracks` → `track_versions` → `media_sources`) decouples listening progress and bookmarks (`version_id`) from physical or remote files, allowing seamless transition from online streaming to offline local playback without losing progress.
- **Safety and acoustic fidelity first**: local playback works completely offline; `.part` files are never playable; stereo L/R channels pass through untouched without hidden DSP/normalization.

## Three Unshakable Principles

1. **毫不动摇宿主（Rust + mpv）作为音频与会话后端的地位。**
   - `MpvController` 与 `PlaybackService` 独占完整播放与会话生命周期：来源优选、在线流地址按需解析、mpv 子进程托管、音频解码输出、位置追踪、自然结束（EOF）判定、A–B 循环回跳、睡眠定时触发与 SQLite 持久化。
   - 前端（React）绝不能自行实现隐式音频引擎、用前端定时器推进队列、或绕过 Rust 直接访问数据库、系统凭据或子进程。
   - 应用启动时恢复会话默认处于 `paused` 状态（等待用户点击播放后再加载/出声）；页面切换、组件重挂载、窗口最小化或关闭到系统托盘，绝不能销毁播放器或打断播放。

2. **毫不动摇前端（React）作为纯控制面的地位。**
   - Renderer 仅作为控制面存在：通过 `callCommand` 发送操作意图，通过 `subscribePlaybackSnapshot` 接收并渲染状态快照。
   - 前端允许在用户拖动进度条时做乐观视觉反馈，但真实播放位置、时长、缓冲状态与错误必须以宿主发布的 `PlaybackSnapshot` 为准。
   - 严禁在前端用推测性的定时器、延迟切歌兜底或状态猜想来掩盖 Rust 状态机或 `mpv` IPC 的缺陷。

3. **毫不动摇播放事实与版本真相的唯一归属（Truth Ownership）。**
   - **实时播放事实归 `mpv`，会话与内容版本事实归 Rust + SQLite。**
   - 播放进度（`playback_positions`）、书签（`bookmarks`）、A–B 循环（`ab_loops`）必须且只能绑定在 **`version_id`（`track_versions.id`）** 上，绝不绑定在临时签名 URL、物理文件路径或单纯的文件名上。
   - 当 UI 显示与后端/mpv 播放事实不一致时，必须修复 Rust 事件归一化或快照同步契约，禁止在 UI 层打补丁重定义播放事实。

## Backend & Domain Design Principles

1. **坚定每一个后端服务与控制器都要面向对象、职责单一、生命周期显式。**
   - 核心职责必须收敛在具名服务对象中（`MpvController`、`PlaybackService`、`LibraryService`、`DownloadService`、`SessionService`、`AsmrOneProvider`、`Database`），禁止在 `commands/mod.rs` 薄命令层散落业务分支。
   - `MpvController` 必须显式管理子进程生命周期：按需启动（`ensure_started`）、唯一命名管道连接超时控制、属性订阅、以及退出/重启时的确定性进程回收（`shutdown` + `kill_on_drop`）。

2. **坚定“作品 → 音轨 → 版本 → 来源”四层解耦与统一入口抽象。**
   - 同一内容版本的不同介质统一抽象为 `media_sources`（`local_imported`、`local_downloaded`、`online_provider`），共同挂载在同一 `track_version` 下。
   - 本地播放与在线播放必须共用统一入口 `PlaybackService::play(version_id, preferred_source_id)`，在未指定特定来源时严格按 `local_imported` → `local_downloaded` → `online_provider` 优先级选择完整可用来源，禁止维护两套平行的播放状态机。
   - 外部站点接入统一收敛在 `AsmrOneProvider` 契约后，业务层只消费归一化后的领域 DTO（`ProviderWork`、`ProviderNode`、`ProviderMediaRef`、`ResolvedMedia`），绝不硬编码拼接远端媒体 URL。

## Core Invariants (Learned from Regression Matrix R01–R17)

以下不变量是保障播放、并发、数据与下载正确性的硬性红线，修改相关代码时必须逐条遵守：

1. **`session_generation` 单调递增，严防慢请求覆盖快请求（R05 / T213）。**
   - 每次调用 `play_internal` 切歌时，必须在锁内立即将 `session_generation` 加 1，并清空 `active_ab_loop = None`、`file_loaded_for_gen = false`。
   - **Rule**：在跨越任何异步 `await` 边界（如在线 `provider.resolve_media`、延迟等待、`mpv.load_file`）之后，必须重新核对 `guard.session_generation == my_gen`。一旦不等，说明已有更新的播放请求接管，当前协程必须立即放弃后续状态修改与 `mpv` 指令并返回最新快照。

2. **`event_seq` 单调递增，前端严格丢弃乱序旧快照（T210）。**
   - Rust 端每次构建 `PlaybackSnapshot` 时通过 `event_seq.fetch_add(1, Ordering::SeqCst)` 生成全局递增序号。
   - **Rule**：前端 `App.tsx` 的 `applySnapshot` 必须保留 `next.eventSeq >= lastEventSeqRef.current` 守卫，严禁无条件 `setSnapshot` 导致旧的异步响应覆盖新状态。

3. **仅将 `end-file (reason == "eof")` 视为自然播放结束，严禁提早或误推进队列（R05 / T208 / T212）。**
   - `mpv` 在切歌替换（`loadfile replace`）或手动停止时也会触发 `end-file` 事件（其 `reason` 为 `"stop"`、`"redirect"` 或 `"quit"`）。
   - **Rule**：
     - 只有 `reason == "eof"` 才代表音频真正自然播完，此时才可将 `playback_positions.completed` 标为 `true` 并触发单曲循环或 `step_queue(1)`。
     - 用户手动拖动进度条到接近结尾不直接算作 `completed`；单独重播已标记 `completed` 的音轨时从 `0` 开始。
     - `reason == "error"` 时，若当前为 `online_provider` 且本代会话尚未重试（`!retried_online_for_gen`），仅允许强制刷新重解析 **1 次**；仍失败则进入 `"error"` 状态，严禁无限重试或静默跳轨。

4. **恢复进度与未加载前的 Seek 必须通过 `pending_seek_ms` 延迟到 `file-loaded` 之后执行（R04 / T212）。**
   - 在 `mpv` 完成文件头加载前直接发送 `seek` 指令会被丢弃或失败。
   - **Rule**：当 `file_loaded_for_gen == false` 时，所有恢复位置与用户 `seek_ms` 统一写入 `pending_seek_ms`。待收到 `MpvEvent::FileLoaded` 或有效 `DurationChanged` 且 `file_loaded_for_gen == true` 时，再 `take()` 出 `pending_seek_ms` 执行 `mpv.seek_absolute_ms`。在 `file_loaded_for_gen` 变为 `true` 之前，忽略 `TimePosChanged` 事件。

5. **网络缓冲（`paused-for-cache`）绝不篡改用户的暂停意图（`user_paused_intent`）。**
   - **Rule**：`MpvEvent::BufferingChanged(true)` 可将状态展示为 `"buffering"`，但当 `BufferingChanged(false)` 触发时，必须依据 `guard.user_paused_intent` 恢复为 `"paused"` 或 `"playing"`，绝不能把处于暂停意图下的播放器因缓冲结束而自动启动播放。

6. **mpv IPC 命令必须用 `request_id` 精确配对，管道断开或进程崩溃必须干净收敛（R06 / T207）。**
   - `mpv` sidecar 必须以无窗口模式启动：`CREATE_NO_WINDOW (0x0800_0000)`、`--no-config`、`--load-scripts=no`、`--idle=yes`、`--no-video`、`--audio-display=no`、`--keep-open=no`、`--input-ipc-server=\\.\pipe\asmrplayer-mpv-<uuid>`。
   - **Rule**：每次写入命名管道必须携带自增 `request_id` 并注册 `oneshot::Sender` 到 `pending_reqs`。当管道读取循环结束或进程异常退出时，必须 `drain()` 所有未决请求返回 `ErrorCode::MpvIpcError`，发出 `MpvEvent::ProcessExited`，立即持久化当前已知进度，并将活动播放状态收敛为 `"error"`。

7. **作品关联不等于音轨合并，目录队列不跨候选分组自动混播（R03 / T209 / T405 / T406）。**
   - 相同 RJ 号仅生成作品级关联候选；不同剪辑版、翻译版或未经哈希/人工确认的 MP3/WAV 目录拥有独立 `version_id` 与时间轴。
   - **Rule**：`play_directory_group` 必须严格按用户选定的 `logical_dir_path`（`group_path`）筛选音轨并按 `version_id` 去重，绝不能把同一作品下未确认合并的 MP3 目录和 WAV 目录自动混进同一队列连播两遍。

8. **显示值优先级铁律：用户覆盖 (`work_overrides`) > 在线元数据 > 本地标签/目录回退（R09 / T407）。**
   - `work_overrides` 支持 `override_mode = 'set'`（覆盖为指定值）与 `'cleared'`（显式清空，不回退到底层值）。
   - **Rule**：刷新在线元数据或重新扫描本地目录时，只允许更新 `works` 原始字段或 `work_sources` 缓存，绝不能覆盖或删除 `work_overrides`；查询作品详情/列表时必须通过 `apply_work_overrides` 合成最终展示值。

9. **`.part` 临时文件永远不能成为本地可播放来源（R10 / T503）。**
   - **Rule**：无论在数据库查询（`list_sources_for_version`）、播放前校验（`play_internal`）还是本地目录扫描（`LibraryService`）中，凡是路径以 `.part` 结尾或物理文件不存在的记录，一律判定为不可用（`"missing"`）或跳过扫描，严禁交给 `mpv` 播放。

10. **HTTP Range 206 续传必须严格校验响应头，200 响应必须截断重写（R11 / T505）。**
    - 断点续传发送 `Range: bytes={existing_bytes}-`（及可用的 `If-Range` / `ETag` / `Last-Modified`）后：
      - **收到 `206 Partial Content`**：必须用 `verify_content_range_start` 核对响应头 `Content-Range` 的起始偏移等于 `existing_bytes`，通过后才允许 `append(true)` 追加写入；若不匹配则报错中止。
      - **收到 `200 OK`**：表示服务器忽略了 `Range` 或远端文件已更新，**必须以 `write(true).truncate(true)` 从 `0` 覆写 `.part` 文件并重置 `downloaded_bytes = 0`**，严禁将 `200` 响应体追加到已有 `.part` 末尾。
      - **收到 `416 Range Not Satisfiable`**：仅在已知 `total_bytes > 0` 且 `existing_bytes == total_bytes` 时进入完成校验，否则删除 `.part` 从零重下。

11. **下载路径清洗与“同盘原子更名 → 事务入库 → 启动自愈”闭环（R12 / R13 / T502 / T507 / T508）。**
    - **Rule 1（路径安全）**：`sanitize_relative_path` 必须剔除盘符前缀、`..` 路径穿越与非法字符（`<>:"/\|?*` 及控制字符），并将 Windows 保留设备名（`CON`, `PRN`, `AUX`, `NUL`, `COM1..9`, `LPT1..9`，无论是否带扩展名）加前缀 `_` 转义。
    - **Rule 2（落盘与自愈）**：下载完成并校验字节数/SHA-256/非 HTML 错误页后，先在同目录将 `.part` 原子 `rename` 为目标文件，再在 SQLite 事务中写入 `local_downloaded` `media_source`（复用原 `version_id`）并更新 `downloads.status = 'completed'`。若更名成功但入库前崩溃，`DownloadService::reconcile_startup` 必须在下次启动时检测到目标文件已存在且无 `.part`，自动补齐 `media_sources` 关联并将任务标为 `completed`。

12. **ASMR 专属体验守则：睡眠定时跨休眠校准、A–B 循环隔离、原声双声道直通与原生 SMTC 集成（R14 / R15 / R16 / R31）。**
    - **睡眠定时**：`"minutes"` 模式必须将到期时间存为 UTC RFC3339（`deadline_utc`），由后台循环对比 `Utc::now() >= deadline_utc`，保证系统睡眠唤醒后立即暂停且仅触发一次；`"end_of_track"` 模式在收到 `eof` 时暂停并关闭定时器，**绝不推进下一轨**。
    - **A–B 循环**：必须校验 `0 <= start_ms < end_ms <= duration_ms`；切歌时自动清除当前激活的 `active_ab_loop`。
    - **双声道直通与专业 DSP 链路**：默认保留原始左右声道定位（严禁单声道混音或开启会自动放大微弱底噪的自动增益补偿）；当启用专业 ASMR DSP / EQ 与防炸耳限制器链路时，必须严格遵循 `asmr-player-pro-audio-dsp` 技能规范。
    - **Windows 原生 SMTC（System Media Transport Controls）宿主直管与隐私掩码**：
      - SMTC 必须在 Rust 宿主层（通过 `windows` / `souvlaki` 绑定窗口 HWND）与 `PlaybackService` 直接双向同步，**严禁依赖前端 WebView 的 `navigator.mediaSession`**（因音频在独立 `mpv` sidecar 进程输出且窗口隐藏到托盘时 WebView 媒体会话会失效）。
      - 同步内容包含：播放状态（`Playing`/`Paused`/`Stopped`）、元数据（音轨标题、声优/社团、作品名/RJ 号、本地缩略图路径）与时间轴（`position_ms` / `duration_ms`），并响应系统下发的 `Play`/`Pause`/`Next`/`Previous`/`Stop`/`Seek`（键盘多媒体键、蓝牙耳机触控/线控、Windows 11 快捷媒体卡片与锁屏控件）。
      - **隐私防社死掩码模式（SMTC Privacy Mask Mode）**：支持在设置中切换 `完整展示`、`隐私脱敏模式（仅保留按键控制，系统浮层与锁屏隐藏真实封面并替换为中性标题 "ASMR Audio Track"）` 或 `禁用 SMTC`。

## Anti-Patterns

- 在 React 组件里用 `setInterval` 或前端音频对象自己维护播放进度和自动切歌。
- 在 `play_internal` 的 `await` 之后不检查 `session_generation == my_gen` 就直接改状态或调用 `mpv.load_file`。
- 在前端不检查 `eventSeq >= lastEventSeqRef.current` 就直接应用 `PlaybackSnapshot`。
- 把 `MpvEvent::EndFile { reason: "stop" | "redirect" }` 当作自然结束去触发 `step_queue(1)`。
- 在 `file_loaded_for_gen == false` 时直接向 `mpv` 发送 `seek` 指令，而不是存入 `pending_seek_ms`。
- 把进度、书签或 A–B 循环绑定在文件路径、文件名或 `source_id` 上，而不是 `version_id` 上。
- 收到 HTTP `200 OK` 响应时仍然以 `append(true)` 方式往 `.part` 文件尾部追加字节。
- 将 `.part` 文件识别为可用本地媒体来源，或在取消下载时误删用户导入的原始音频文件。
- 在 `commands/mod.rs` 或异步执行器线程上直接跑耗时目录遍历/Lofty 标签解析而不做取消与错误处理。
- 刷新线上元数据时直接覆盖用户通过 `work_overrides` 手动编辑或清空的字段。
- 在日志或错误提示中明文输出认证 Token、Cookie 或带签名的临时媒体 URL。
- 在输入框（`INPUT` / `TEXTAREA` / `SELECT` / `isContentEditable`）聚焦时截获 `Space` 或方向键全局快捷键。

## Current Architecture Map

> 下表为 `ASMRplayer` 当前代码库的模块分层映射。新增功能、排查 Bug 或评审架构时，必须严格对照此表确认“该逻辑属于宿主后端还是前端控制面”。

| 模块 | 路径 | 职责 | 层级 |
| --- | --- | --- | --- |
| `mpv` sidecar | `src-tauri/binaries/mpv-x86_64-pc-windows-msvc.exe` | 独立音频解码、网络流拉取、音频设备输出（伴随 `vulkan-1.dll`） | native host (sidecar) |
| `MpvController` | `src-tauri/src/player/mod.rs` | `mpv` 子进程启停、唯一 Windows 命名管道 JSON IPC、`request_id` 配对、属性监听 | 后端 / 播放器桥接层 |
| `PlaybackService` | `src-tauri/src/services/playback.rs` | 播放会话状态机、`session_generation` 并发控制、来源优选、队列推进、5s/关键点进度持久化、A–B 循环、睡眠定时 | 后端 / 核心编排层 |
| `LibraryService` | `src-tauri/src/services/library.rs` | 本地作品文件夹与根目录导入预览、增量扫描、自然排序、Lofty 标签读取、根目录重定位 | 后端 / 本地库服务 |
| `DownloadService` | `src-tauri/src/services/download.rs` | 离线下载队列（并发 2）、安全路径清洗、`.part` 写入、Range `206`/`200`/`416` 续传、校验、原子更名与启动自愈 | 后端 / 下载服务 |
| `SessionService` | `src-tauri/src/services/session.rs` | 书签 CRUD、A–B 循环持久化、睡眠定时委托、字幕（UTF-8 SRT/VTT）解析与偏移绑定、退出优雅回收 | 后端 / 会话与体验服务 |
| `AsmrOneProvider` | `src-tauri/src/providers/mod.rs` | `asmr.one` API 客户端、RJ/作品 ID 归一化、远端文件树解析、按需流/下载媒体解析、重试与凭据存储（`keyring`） | 后端 / 在线内容源 |
| `Database` & `repo` | `src-tauri/src/db/mod.rs`, `src-tauri/src/db/repo.rs` | SQLite 连接池化封装、迁移执行器（`001`–`004`）、四层模型 CRUD、作品/音轨关联与撤销、`work_overrides` 合成 | 后端 / 数据持久层 |
| `domain` | `src-tauri/src/domain/mod.rs` | 跨层 DTO、`ErrorCode`、统一 `AppError`（含 `retryable` 与 `hint`）、自然排序键算法 | 后端 / 领域模型 |
| `commands` & `lib` | `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs` | Tauri `#[tauri::command]` 薄封装、`AppState` 注入、系统托盘、窗口关闭拦截（最小化到托盘 vs 真正退出） | 宿主壳 / IPC 边界 |
| `api.ts` | `src/lib/api.ts` | 前端类型化 Tauri `invoke` / `listen` 封装、DTO 类型定义、错误归一化、时间格式化 | 控制面 / IPC 客户端 |
| `App.tsx` | `src/app/App.tsx` | 顶层导航壳、`eventSeq` 快照守卫、全局键盘快捷键（含输入焦点隔离）、错误横幅与抽屉路由 | 控制面 / 应用壳 |
| `PlayerBar` & `RightDrawer` | `src/features/player/PlayerBar.tsx`, `src/features/player/RightDrawer.tsx` | 底部常驻播放控制栏、来源切换、睡眠定时、A–B 循环、队列/书签/字幕抽屉 | 控制面 / 播放控件 |
| `Library` / `Online` / `Downloads` / `Settings` | `src/features/{library,online,downloads,settings}/*` | 本地作品列表与详情树、在线浏览搜索、下载任务管理、目录/API/代理/诊断设置页面 | 控制面 / 业务视图 |

## Out-of-Scope & Forbidden Surfaces (Do Not Introduce)

以下设计或假设在本项目中明确禁止引入：

- **禁止前端直连数据库或外部网络**：React 层不得直接发起 `asmr.one` 业务请求、读取 SQLite 或读写本地任意路径，所有 I/O 必须经由受控的 Tauri Commands。
- **禁止将临时流地址作为持久化主键**：`mediaStreamUrl` / `mediaDownloadUrl` 可能带短期签名，数据库 `media_sources` 只能持久化 `(provider, source_work_id, remote_file_id, remote_relative_path)`，播放或下载前通过 `resolve_media` 按需解析。
- **禁止因磁盘离线或扫描中断批量删除作品**：外接硬盘拔出或 API 报错只能将 `availability` 更新为 `"missing"` / `"offline"`，必须完整保留用户的收藏、历史进度、书签和覆盖字段。
- **禁止清理缓存时误删离线下载或用户原文件**：“清理播放缓存”仅限清理应用缓存目录与过期元数据缓存，绝不能触碰用户导入的媒体根目录和已完成的离线下载目录。
- **范围边界克制**：不引入多站点聚合爬虫、云同步、推荐算法、边播边存或多分片并发加速；所有专业 DSP / EQ 与防炸耳限制器均在宿主侧（`mpv` `lavfi` 滤镜链）按 `asmr-player-pro-audio-dsp` 规范实现，不在前端做 WebAudio 劫持。

## Known Fix & Implementation Patterns

### 1. Slow Track Resolution Race (`session_generation` Guard — R05)

**Observed symptom**:
- 用户先点在线音轨 A（网络解析慢），紧接着点本地音轨 B（立即播放）；数秒后 A 解析完成，把正在播放的 B 强行替换掉。

**Correct implementation pattern**:
- `play_internal` 入口处递增 `session_generation` 并记录局部变量 `my_gen`。
- 在 `provider.resolve_media(...).await` 返回后、`mpv.load_file(...).await` 调用前，必须再次加锁检查 `if guard.session_generation != my_gen { return Ok(drop_and_snap(&self, guard)?); }`。
- 相关回归测试：`src-tauri/src/services/playback.rs` 中的 `test_generation_race_slow_a_does_not_override_fast_b_r05`。

### 2. Deferred Seek on Startup / Unloaded Track (`pending_seek_ms` — R04 / T212)

**Observed symptom**:
- 应用重启恢复上次播放位置，或在音轨尚处于 `resolving`/`loading` 阶段拖动进度条，位置跳回 `0:00`。

**Correct implementation pattern**:
- 启动恢复或切歌初始化时，将目标恢复毫秒数写入 `pending_seek_ms`，同时置 `file_loaded_for_gen = false`。
- 在 `MpvEvent::FileLoaded` 和 `MpvEvent::DurationChanged` 事件分支中，当确认 `file_loaded_for_gen == true` 时，`take()` 出 `pending_seek_ms` 并调用 `mpv.seek_absolute_ms(seek_target)`。

### 3. HTTP Range 206 vs. 200 Fallback in Offline Downloads (R11 / T505)

**Observed symptom**:
- 下载暂停后续传，若服务器不支持 Range 并返回 `200 OK` 全量数据，直接 append 会导致文件体积翻倍且媒体损坏。

**Correct implementation pattern**:
- 检查响应状态码：
  - `206 Partial Content`：调用 `verify_content_range_start(headers, existing_bytes)` 校验 `Content-Range: bytes {start}-{end}/{total}` 的 `start == existing_bytes`，校验通过才以 `append(true)` 打开 `.part`。
  - `200 OK`：强制将 `resume_offset` 重置为 `0`，以 `create(true).write(true).truncate(true)` 覆写 `.part` 文件。
- 相关回归测试：`src-tauri/src/services/download.rs` 中的 `test_download_range_206_and_200_fallback_r11`。

### 4. Crash Recovery Between Atomic Rename and DB Commit (R13 / T508)

**Observed symptom**:
- `.part` 已原子更名为正式音频文件，但进程在 SQLite 事务提交前崩溃；重启后任务仍处于 `downloading`/`verifying`，导致重复下载或报错。

**Correct implementation pattern**:
- 启动时执行 `DownloadService::reconcile_startup()`：
  - 若任务非 `completed`，但 `target_abs.is_file()` 已存在且 `!part_abs.exists()`，自动补记 `media_sources`（绑定原 `version_id`，`source_kind = 'local_downloaded'`）并将任务状态修正为 `completed`。
  - 若 `.part` 仍存在，则将未完成任务安全收敛为 `paused` 等待用户继续。
- 相关回归测试：`src-tauri/src/services/download.rs` 中的 `test_startup_reconcile_after_rename_before_db_commit_r13`。

### 5. Local File Upload → Work-Root Folder Auto-Discovery (`RJ+数字` & Title Folder Fallback) & Recursive Subfile Aggregation (R28 / T1110)

**Observed symptom**:
- 用户在导入本地作品时，往往直接点选或拖拽某个具体的音频文件（如 `...\RJ01234567 治愈耳かき\WAV\01.wav` 或无 RJ 号的 `...\お姉さんの膝枕\SEなし\01.mp3`）。若仅导入该单文件或仅扫描其直接父目录（`WAV/`、`SEなし/`），会丢失外层总文件夹下的作品标题/RJ 号、主封面图、`MP3/` 兄弟目录、`特典/` 目录以及 `字幕/` 子目录中的所有关联子文件。

**Correct implementation pattern**:
1. **三级向上溯源定位“整体作品根文件夹（Work Root Folder）”**：
   - **第一级（`RJ+数字` 强特征锚定）**：从所选文件（或子目录）向上逐层回溯祖先目录（最多向上探测 4 层，遇到盘符根目录、系统目录如 `Desktop`/`Downloads` 或已注册的 `library_root` 立即停止）。若某层目录名包含 `(?i)(RJ|BJ|VJ)\d{6,8}`（如 `RJ01234567`、`[RJ123456] 标题`），立即将该层锁定为整体作品根文件夹。
   - **第二级（无 RJ 号的“标题名”文件夹结构化穿透回退）**：当祖先目录均不含 `RJ+数字`（即整体文件夹以**作品标题名**命名）时，检测文件直接父目录是否属于**内部结构化子目录**：
     - 音质/格式子目录：`WAV`, `MP3`, `FLAC`, `AAC`, `OGG`, `OPUS`, `Hi-Res`, `48kHz`, `96kHz`, `无损`, `高音质`；
     - 版本/分轨子目录：`SEあり`, `SEなし`, `有音效`, `无音效`, `本編`, `本篇`, `特典`, `おまけ`, `Bonus`, `Extra`, `FreeTalk`, `Disc *`, `CD *`, `Track*`, `音声`, `Audio`；
     - 资源附属子目录：`字幕`, `Subtitles`, `Subs`, `LRC`, `SRT`, `台本`, `画像`, `Images`, `Cover`。
     若命中上述结构化子目录（或上级目录同时存在封面图与多个兄弟子目录），自动向上穿透 $1\text{–}2$ 层，将外层的**“标题名”文件夹**判定为整体作品根文件夹；若文件直接放在标题文件夹下，则取其直接父目录。
   - **第三级（预览弹窗目录层级微调）**：在导入预览弹窗中展示自动锁定的整体作品文件夹路径、识别依据（`RJ 编号目录` / `标题名目录`）及汇总的子文件统计，并提供 **“向上一层 / 向下一层”** 快捷修正按钮。
2. **向下全量递归搜寻所有子文件（Recursive Sub-file Discovery）**：
   - 锁定整体作品根文件夹后，通过 `WalkDir`（跳过符号链接与 `.part` 文件）递归搜寻其下**所有子目录与全部子文件**：
     - 自动聚合所有音频文件（保留 `WAV/`、`MP3/`、`特典/` 等相对子目录作为 `logical_dir_path` 分组并按 `natural_sort_key` 自然排序）；
     - 自动搜寻所有封面与插图文件（优先选取 `cover`/`folder`/`jacket`/`main` 或含 RJ 号的主图）；
     - 自动搜寻所有字幕（`.lrc`/`.srt`/`.vtt`/`.ass`）与台本附件（`.txt`/`.md`/`.pdf`），并按双扩展名剥离或文件名主干自动关联到对应音轨版本。

### 6. Multi-Monitor Window Placement Persistence & Secondary Screen Restore (R39 / T1119)

**Observed symptom**:
- 用户将窗口拖动到副屏（`\\.\DISPLAY2` 等，包括正坐标右侧副屏或负坐标左侧/上方副屏）收听或最大化并在副屏关闭软件；下次打开时软件又跳回主屏默认位置，或者因 Windows 最大化 `-8px` 边框偏移导致 `maximize()` 错误落回主屏，亦或拔掉副屏后窗口卡在屏幕外不可见。

**Correct implementation pattern**:
1. **捕获与持久化 (`commands::capture_and_save_window_placement`)**：
   - 在 `WindowEvent::Moved`、`WindowEvent::Resized`、`WindowEvent::CloseRequested`、`commands::window_close`、`commands::window_set_mini_mode` 及托盘 `quit` 时捕获当前窗口状态并写入 `app_settings` (`window_placement_json`)。
   - **最小化与迷你模式守卫 (`is_valid_normal_window_bounds`)**：当 `is_minimized() == true` 或坐标落入 Windows 最小化伪区间 (`x <= -25000 || y <= -25000`) 时严禁覆盖已保存坐标；处于迷你模式时保留常规窗口尺寸但同步更新目标显示器。
2. **启动跨屏解析与恢复 (`domain::resolve_window_restore_placement` & `commands::restore_window_placement_from_db`)**：
   - 在 `main.rs` 的 `.setup()` 阶段枚举 `available_monitors()`，按 **`monitor_name + 原点坐标` → `窗口中心点命中` → `显示器原点匹配` → `monitor_name 匹配`** 四级优先级锁定上次关闭所在的副屏。
   - 若关闭时处于副屏最大化（`maximized == true`），必须先将锚点坐标收敛至副屏内部（`>= target_mon.x + 24`）并执行 `window.set_position(...)`，再调用 `window.maximize()`，确保在副屏上最大化。
   - 若副屏已拔除离线，自动回退并居中到当前主屏，绝不将窗口遗留在不可见区域。

## Verification Guide

按改动面分层执行验证，确保静态检查与对应层的单元/集成测试全部通过：

| 改动面 | 最低验证 | 完整验证 |
| --- | --- | --- |
| 任何前端 TS / React 改动 (`src/*`) | `pnpm typecheck` + `pnpm lint` | + `pnpm test` |
| 前端应用壳 / 快照序号守卫 / 快捷键 (`src/app/*`) | `pnpm test` + `pnpm typecheck` | + `pnpm lint` |
| Rust 领域 / 数据库 / 扫描 (`src-tauri/src/{domain,db,services/library.rs}`) | `cargo test --lib --manifest-path src-tauri/Cargo.toml` | + 检查 SQLite 迁移幂等性 |
| Rust 播放核心 / mpv IPC (`src-tauri/src/{player,services/playback.rs}`) | `cargo test --lib --manifest-path src-tauri/Cargo.toml playback` | + 全量 `cargo test --lib --manifest-path src-tauri/Cargo.toml` |
| Rust 在线源 / 离线下载 (`src-tauri/src/{providers,services/download.rs}`) | `cargo test --lib --manifest-path src-tauri/Cargo.toml download` | + 全量 `cargo test --lib --manifest-path src-tauri/Cargo.toml` |
| 跨前后端 IPC / DTO (`commands/mod.rs` + `src/lib/api.ts`) | `pnpm typecheck` + `pnpm test` + `cargo test --lib --manifest-path src-tauri/Cargo.toml` | + `.\dev.bat --check-only` |
| 启动脚本 / 打包配置 (`dev.bat`, `tauri.conf.json`) | `.\dev.bat --check-only` | + `pnpm build` + `cargo check --manifest-path src-tauri/Cargo.toml` |

## Git & Engineering Discipline

- **原子化提交**：每次完成一个独立特性或修复，将其实现代码与对应的回归测试（Vitest 或 Cargo test）放在同一个提交中。
- **严守仓库卫生**：绝不提交运行时 SQLite 数据库（`*.db`、`*.sqlite*`）、日志文件、临时目录（`.tmp/`）、未完成下载（`*.part`）、私人音频样本或真实站点凭据。
- **数据库迁移安全**：新增表结构必须在 `src-tauri/migrations/` 下按序号新增 SQL 文件并注册到 `Database::migrate`；迁移必须幂等，且迁移失败时必须返回可诊断的 `AppError::DbError`，严禁静默删除或重建用户已有数据库。
- **可追溯引用**：在代码注释、测试函数命名和提交说明中保留对应的任务编号（`T101`–`T706`）或回归矩阵编号（`R01`–`R17`），便于验收核对。
