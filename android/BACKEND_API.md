# Android 底层接口（AN004 / AN005 / AN006 / AN007 字幕 / AN009 资料迁移与更新）

UI 术语（AN007.R29）：单曲 `favorite` / `setVersionFavorite` 对应“书签”；`Bookmark` / `bookmarks` / `addBookmark` 对应带播放时间与笔记的“标记”。AN007.R38 在喜欢主页将两类记录分别显示为「收藏音轨」和「时间标记」。两者继续使用各自的 Room 身份、播放命令与备份结构；作品 favorite 仍表示作品喜欢。

## 响度均衡（AN017）

AN007.R25 的 `TonePreset` 仅修改 highPass/cutoffHz/deEsser/air/warm 并解除 bypass，保留 PEQ/前置增益、响度、平衡、交换与限幅。新增 `DspConfig.warm` 默认 false，DspSettings JSON 缺失字段按 false 兼容；开启时使用 4kHz / −2dB / Q √0.5 双声道同系数高架滤波，不占用户 10 段 PEQ。沿用现有服务热更新与限幅。

`DspConfig.leveler` 与 `normalize` 为独立开关，默认关闭；`targetLufs` 默认 −23、范围 −30…−16，`maxBoostDb` 默认 6、范围 0…12。旧 DSP/耳机方案解码缺失字段时采用默认值。仍通过 `SET_DSP` 与服务唯一音频链应用。

同应用 `ANALYZE_LOUDNESS` 强制重测当前版本、`CANCEL_LOUDNESS` 取消；`GET_DSP` / 耳机状态响应增加 `loudnessStatus`、`loudnessBusy`、`loudnessProgress`（未知为 −1）、`normalizationGainDb`。`LoudnessSession` 持有任务，按版本代次拒绝过期结果；解码和缓存位于 IO 线程，播放线程只接收原子增益快照。缓存不写入耳机配置或 Room，切轨清零后只应用当前来源的有效完整测量。行为与边界见 [响度均衡说明](../docs/releases/ANDROID_LOUDNESS.md)。

Compose 页面通过 `PlayerViewModel`、`CollectionTools` 和 `MaintenanceTools` 接入统一数据与服务接口。不要在 UI 中另建 Room、播放器或定时器推进队列。前端适配模型统一从本接口取得作品、收藏与分级，避免另建临时 JSON 作为第二份资料库。

## 数据入口

AN007.R32：在线 `OnlineWork.score: Double?` 接受来源 `rate_average_2dp` 中大于 0 且不超过 5 的有效数值，其他值为 null。仅用于发现页展示，和年龄分级 `rating` 独立；不修改 Room schema 或历史作品资料。CV、tags、hasSubtitle、rating 继续取来源原始字段。

AN007.R31：本应用控制器通过 `LibraryPlayback.SEEK_POSITION` 提交 `versionId` 与 `positionMs`，服务验证仍为当前版本、非直播且可定位或处于 IDLE；负值截为 0，已知时长时限制上界。先 seek，再仅在 IDLE 时 prepare，保持原有 playWhenReady，不主动 play。结果沿用会话响应与持久化流程；UI 的 `canPrepareSeek` 配合已保存时长允许暂停恢复会话操作。未知时长或不可定位流不开放进度条。

`val backend = AndroidBackend.get(application)`（`com.asmrplayer.android.data`）。进程级单例，内部 Room 数据库 `asmr-library.db`，schema 8；1→2 新增在线引用/下载任务，2→3 新增附件引用，3→4 新增音轨版本喜欢，4→5 新增作品元数据，5→6 新增同步状态，6→7 新增作品 `inLibrary` 标记（默认 1，保留已有作品），7→8 新增保存的队列分组。保留任务 ID、状态、续传信息、库、进度和会话，不做破坏性迁移。

`backend.library`：

| API | 调用与结果 |
| --- | --- |
| `roots`, `works`, `visibleWorks`, `preferences`, `positions`, `scanStates` | Flow / StateFlow；在 ViewModel 收集，Compose 用 lifecycle-aware 收集。`works` 是完整库，`visibleWorks` 按同一库的 NSFW/未分级偏好过滤。 |
| `authorizeRoot(treeUri, mode, existingRootId?)` | suspend；先用 `OpenDocumentTree` 取得 URI。保存 READ 持久授权并返回根目录；再调用 `scan(root.id)`。重新授权原逻辑目录时传原 ID。 |
| `preview(treeUri, mode)` | suspend；只读扫描预览，不写目录内容。`RootMode.AUTO/WORK/COLLECTION`；WORK 整棵树一个作品，COLLECTION 首层目录各一个作品。AUTO 根据根名/RJ、根部音频及格式目录判断。可以预览后更换 mode，不能上探未授权父目录。 |
| `scan(rootId)`, `cancelScan(rootId)` | 前者 suspend，返回 `ScanPreview`；进度在 `scanStates[rootId]`。取消调用扫描协程或 cancelScan；取消/不完整结果不提交。成功扫描只标记消失来源不可用，不删收藏、进度。 |
| `work(id)`, `versions(workId)`, `assets(workId)` | 作品详情、自然排序版本 Flow、附件 Flow。音频 URI 由 `resolve(versionId)` 获取；列表不要逐项打开音频。封面 URI 在 work.coverUri。 |
| `setFavorite(workId, favorite)`, `setRating(workId, rating)` | suspend；rating 为 `UNRATED` / `SAFE` / `NSFW`，扫描不会自动判断内容分级。 |
| `setContentFilter(includeNsfw, includeUnrated)` | suspend；NSFW 开启表示包含 NSFW，不是只显示 NSFW，不影响系统媒体隐私。 |
| `importDocument(uri, title)` | suspend；只导入已由 OpenDocument 授权的单文件，不扫描父目录。返回稳定 versionId/sourceId。 |
| `position(versionId)`, `bookmarks(versionId)`, `addBookmark(versionId, positionMs, note)`, `removeBookmark(id)` | suspend 查询/命令与书签 Flow；全部绑定版本 ID。当前播放位置的书签优先使用服务命令。 |

作品 → 音轨 → 版本 → 来源四层 ID；相对路径在同一 root ID 下重扫/换 URI 保持身份。文件重命名或改变作品分组会产生新身份，不按相似名称合并。目录队列应只选择同一 `groupPath` 和用户选择的格式版本，避免把 WAV/MP3 重复连播。自动字幕关联仅在候选唯一时进行；未自动绑定的字幕在 `assets(workId)`，可让用户明确绑定。

## 本地分类、收藏与收听记录（AN007.C 底层）

AN007.R14 将作品 `inLibrary` 与 `favorite` 分开：普通音频库只列主动入库的作品，喜欢页仍可显示未入库但已喜欢的作品/音轨。在线播放缓存、队列与历史继续使用同一稳定身份。UI 的“撤销入库”只还原标记；删除书签撤销通过 `bookmark(id)` 与 `restoreBookmark(record)` 还原原记录，不创建新时间点。播放器队列页复用同一 MediaController；REMOVE_QUEUE_ITEM 把完整 MediaItem 留在服务内并返回撤销令牌，UNDO_QUEUE_REMOVE 在令牌及队列快照仍匹配时恢复原条目。控制器不回传缺少播放地址的时间线对象，临时流与库音轨沿用相同撤销路径。

入口 `backend.library.collections`。返回 Room 驱动的 Flow；页面更换查询时可用 `flatMapLatest` 切换，并按生命周期收集。全部列表遵守同一 includeNsfw/includeUnrated 偏好，隐藏作品的音轨、历史与书签备注不会进入这些结果。

| API | 语义 |
| --- | --- |
| `catalog(LibraryQuery())` | `Flow<List<WorkSummary>>`，包含原 work（含离线优先封面）、版本总数、已有进度数、完成数、本地导入/已下载/在线版本数和最近收听时间。多个来源只计算一次版本；WAV/MP3 是不同版本。 |
| `favoriteVersions(text="")` | 已收藏的稳定版本，按收藏更新时间倒序；返回标题、作品/分组/格式、可用来源标记和已有进度。缺失来源不会删除收藏。 |
| `history(text="")` | 按最后进度更新时间倒序，每个版本一条最近记录；不是多次收听流水或时长统计。 |
| `bookmarks(text="")` | 跨作品书签集合，按创建时间倒序，附作品/音轨/格式与来源可用状态；关键词可匹配备注。删除仍调用 library.removeBookmark，当前播放添加仍走 LibraryPlayback.bookmark。 |
| `setVersionFavorite(versionId, favorite)` | suspend；显式保存喜欢/取消，要求版本存在。与作品收藏独立，不跨格式/版本传播；扫描或在线元数据刷新不会覆盖。取消记录保留在资料包中，支持明确的覆盖导入策略。 |

`LibraryQuery` 字段：text 匹配作品标题或编号（忽略大小写）；favoritesOnly 仅作品收藏；source 为 ALL / LOCAL（含已失联的导入版本，便于恢复）/ DOWNLOADED（已发布且可用的下载来源）/ ONLINE（可用在线来源）；listening 为 ALL / UNPLAYED / IN_PROGRESS / COMPLETED。IN_PROGRESS 表示有过进度且并非所有版本已完成，COMPLETED 要求至少一个版本且所有版本完成。sort 为 TITLE / CODE / RECENTLY_PLAYED，descending 控制方向；最近收听优先传 descending=true，标题采用自然排序，使用稳定 ID 打破同名顺序。

本地来源可用性是最近扫描/下载确认结果，列表查询不逐项打开媒体；点击播放仍由既有来源解析验证授权和实际文件。队列编辑继续调用 MediaController.moveMediaItem/removeMediaItem；服务事件保存顺序和当前位置，重建时默认暂停，无需另外持久化一份 UI 队列。

## 下载存储统计与封面清理（AN007.C 底层）

- `suspend backend.downloads.storageUsage()`：按实际文件长度采样。`totalBytes` 保留全部下载任务文件的原有口径；设置页使用互不重叠的 `downloadBytes`（下载文件及断点，扣除已完成的主封面）和 `cacheBytes`（已完成主封面、浏览封面、在线播放分片、响度分析结果）。下载的图片附件仍属于下载文件。两项均不含用户 SAF 原件、数据库、更新 APK 和缓存索引。
- `suspend clearCompletedCoverCache()`：只清理已完成的在线主封面缓存，经过原任务的取消/删除/来源失效流程；返回 removedIds 与 failedIds，失败项可重试。音频、字幕、图片附件、作品收藏、进度、书签和系统授权均保留。未完成封面任务仍由下载管理处理。
- 选定下载的移除复用 `downloads.remove(taskId)`；调用前由 UI 明确展示删除范围。不要以递归删除 files/noBackupFilesDir 代替业务接口。

## 播放服务命令

复用现有 `MediaController`，调用 `com.asmrplayer.android.playback.LibraryPlayback`；返回 `ListenableFuture<SessionResult>`，检查 `resultCode == RESULT_SUCCESS` 后更新反馈。普通播放/暂停/seek/队列移除与重排仍用 MediaController；所有操作在主线程提交。

| API | 语义 |
| --- | --- |
| `play(controller, versionIds, index=0, resume=true)` | 最多 500 条，服务解析来源并替换运行队列。恢复该版本未完成进度；完成过的版本从零重播。迟到解析不能覆盖更新请求。 |
| `state(controller)` | 返回服务扩展快照 Bundle。`ready`, `versionId`, `abVersionId`, 可选 `aMs/bMs/deadlineMs`, `endOfTrack`, `unavailableVersionIds`, `warning`。位置/时长/播放状态仍读 MediaController。 |
| `setAb(controller, versionId, aMs, bMs)` | 必须当前稳定版本、可 seek，0 ≤ A < B ≤ 时长；两个 null 清除。切换版本清除活动循环。 |
| `setSleep(controller, deadlineEpochMs?, endOfTrack=false)` | 两个模式互斥；null + false 取消。绝对 UTC epoch 毫秒，服务持有截止；播完本轨暂停，不推进下一轨。 |
| `bookmark(controller, note)` | 服务用真实当前版本和当前位置存书签，结果 extras.bookmarkId。`ADD_BOOKMARK` 的 Bundle 可额外携带 `versionId`；UI 提交草稿时携带该 ID，服务拒绝切歌后的过期提交。 |
| `save(controller)` | 等待队列与当前位置落库，可用于明确退出/测试；常规无需 UI 主动保存，服务已定期和事件保存。 |

恢复队列默认暂停、不主动 prepare/出声。本地版本及导入库的在线版本均可持久化；临时 URI、直接 HTTPS URL 仍可用现有 MediaController 播放，但不把临时地址当永久内容身份。旧单文件入口持有持久 READ 授权时，服务自动注册为稳定版本。离线来源保留队列记录，state 提供不可用 ID，重新授权后通过 `play` 重新解析。实际进程强杀最多丢失最近约两秒的位置；系统强停后不承诺自动播放。

## 在线来源（AN006 底层）

`backend.online` 复用桌面 asmr.one 兼容 API 契约；默认 API origin 为 `https://api.asmr-200.com`。以下方法均为 suspend，UI 在 ViewModel 协程调用；取消旧搜索/详情请求并核对当前请求身份，避免迟到响应覆盖新页面。`CancellationException` 继续抛出。

| API | 调用与结果 |
| --- | --- |
| `account.state` | StateFlow：`apiBase`、`hasToken`，不返回明文凭据。 |
| `account.setApiBase(origin)` | HTTPS origin，不带路径/query/用户信息；切换来源后重新加载列表。不同 origin 分开保存 Token，旧库的来源引用仍使用原 origin。 |
| `account.setToken(tokenOrNull)` | 当前 origin 的 Bearer Token；null/空串清除。Android Keystore AES-GCM 加密后持久化；跨 origin 重定向不转发认证头。没有用户名/密码登录接口。 |
| `browse(query="", page=1, order="release", ascending=false)` | 返回 `OnlinePage(items, page, pageSize, total)`；order 支持 release/dl_count/rate_average_2dp/id。使用本地库同一 NSFW/未分级偏好；total 是来源总数，过滤后本页可能不足 pageSize。 |
| `detail(id)` | 接受数字或 RJ 编号，返回作品元数据和目录树展开后的音频/字幕/图片条目；不改本地库。返回的媒体 URL 只供当次请求使用，不能持久化。 |
| `importWork(id, addToLibrary=true)` | 导入或刷新版本及附件引用，返回 `ImportedOnlineWork(workId, versionIds, assetIds, coverAssetId)`。临时播放/加入队列/喜欢操作传 false，只缓存播放资料；主动入库和下载传 true。false 不会移除已有入库状态。保留喜欢、分级、进度、书签和下载附件；来源消失只标记在线引用不可用。 |
| `assets(workId)` | Flow<List<RemoteAsset>>：id/workId/relativePath/kind/format/byteSize/available；kind 为 subtitle/image/cover，available 只表示上次刷新时远端存在。不是本地下载完成状态，不能据此显示离线可用。 |

播放顺序：`val imported = backend.online.importWork(id, addToLibrary=false)`，再 `LibraryPlayback.play(controller, selectedVersionIds)`；选择同一目录与目标格式，避免不同格式重复连播。音频库列表经 `library.collections.catalog()` 过滤 `inLibrary`；`library.works` 仍提供所有缓存记录，供当前队列、详情及历史解析。`library.setMembership(workId, value)` 只变更入库标记，不删除媒体、喜欢、进度、书签或队列。来源作品 ID + origin、远端文件 ID + 相对路径构成稳定身份；不按相似标题强行合并。

在线详情按来源、账号范围和作品 ID 暂存在进程内，五分钟内复用，过期时页面可先显示已有详情并异步刷新；显式刷新重新获取。详情的播放和入库操作可复用同批作品目录。切换内容分级后重新检查展示权限。

导入的在线版本以 `asmr://version/{versionId}` 进入 Media3。`VersionDataSource` 在加载线程依次选择本地原件、已完成下载和在线来源。在线音轨先按稳定版本、账号范围和来源修订读取 Media3 磁盘片段，仅在缺少字节时使用临时地址；401/403 刷新并重试一次。片段在应用私有缓存目录按最近使用淘汰，预算最多 512 MiB。临时文件不登记为下载任务。下载完成后下次打开/seek 重新解析即可使用离线来源，不改变进度/书签的版本 ID，不强制打断正在播放的流。队列恢复不提前解析全队列网络地址；签名 URL 不写入 Room 的来源身份或会话。

在线错误使用 `OnlineException.code`：AUTH_REQUIRED、NOT_FOUND、RATE_LIMITED、NETWORK、SCHEMA_CHANGED、SOURCE_CHANGED、STORAGE；消息不包含 Token 或签名 URL。参数错误为 IllegalArgumentException；UI 转成对应反馈，不把原始网络异常/地址写入用户日志。

## 离线下载（AN006 底层）

`backend.downloads` 为进程级下载入口；UI 只发送操作、收集 `tasks` Flow，不自建下载循环或 WorkManager。任务 ID 与音轨 versionId 或附件 assetId 固定对应，下载到应用 `noBackupFilesDir/offline`，卸载应用会移除这些文件；尚无 SAF 导出接口。

| API | 调用与结果 |
| --- | --- |
| `tasks` | Flow<List<DownloadTask>>；提供 id/versionId/assetId/state/downloadedBytes/totalBytes/errorCode/updatedMs，移除的任务不再出现在列表。**versionId 与 assetId 恰有一个非 null**：音轨通过版本所属作品取标题，附件通过 online.assets 的 workId/relativePath 取标题；不要把附件送入播放队列。totalBytes 为 null 时显示不定进度，系统通知使用中性文字。 |
| `awaitReady()` | 等待启动时任务与文件核对；写操作内部已等待。初次收集 tasks 时不要把恢复前的瞬时状态当最终状态。 |
| `enqueue(versionIds)` | 先导入在线作品，传 1–500 个版本，返回任务 ID；已完成任务不重复下载。Android 14+ 必须由可见界面的用户操作触发。 |
| `enqueueAssets(assetIds)` | 传 1–500 个字幕/图片/封面引用 ID，返回任务 ID，复用同一队列、暂停/续传/恢复/删除。需要已有在线引用；Android 14+ 同样由可见用户操作触发。 |
| `cacheCover(workId)` | 入队该已导入作品的主封面，返回任务 ID；没有主封面引用时返回 NOT_FOUND。已完成缓存不重复请求；要更新缓存可显式 remove 旧任务，再 cacheCover。 |
| `pause(taskId)` | 保存 PAUSED 后取消传输，保留临时文件；不影响播放队列。 |
| `resume(taskId)` | 用最新来源地址续传；失败任务也通过此接口重试。来源变化/校验失败/文件丢失时清理本应用对应临时文件后重下。Android 14+ 同样需要可见用户操作。 |
| `remove(taskId)` | 停止任务并仅删除本应用的下载文件，撤销对应音频来源或附件可用状态。字幕绑定保留但标为不可用，封面回退到在线元数据地址（无地址则为空）；保留在线作品、收藏、进度和书签，不删除用户导入原文件。 |

状态为 QUEUED、DOWNLOADING、VERIFYING、PAUSED、COMPLETED、FAILED；内部 REMOVED 不展示。FAILED 的 errorCode 使用在线错误码，并有 INTEGRITY、MISSING_FILE、INVALID_CONTENT（附件解码/解析失败或超过大小限制）；SCHEDULING 表示后台任务未启动，任务暂停，用户可在前台继续。网络错误最多自动尝试三次，之后明确失败；不要将暂停/失败显示为正在下载。

Range 206 校验起点、长度及来源验证标识；200 从零截断重写；416 仅在总长度与验证标识吻合时接受完整临时文件。无续传标识时重新下载，服务分段返回合法 206 时继续请求余下数据。完成后校验长度、计算本地 SHA-256、同步文件、更名，再事务发布离线来源；`.part` 永不用于播放。SHA-256 用于本地更名/数据库中断后的恢复核对，不是来源提供的数字签名。

启动时核对更名后未入库的文件；已完成文件丢失/长度变化则撤销离线来源。进程中断留下的活动任务恢复为可继续的 PAUSED；系统正常取消留下 QUEUED 时可由调度器重试。重启/强停后不承诺无条件自动续传，用户始终可以前台继续。

Android 14+ 使用用户发起的数据传输 JobScheduler；23–33 使用 WorkManager + dataSync 前台通知。`PlayerApplication` 为 WorkManager 分配 10000–19999 的 job ID，与 UIDT 6006 隔离。UI 在 Android 13+ 提供通知权限申请/拒绝反馈，不用权限结果伪造任务成功；调度失败以持久任务状态为准。通知提供“暂停全部”，后台下载不依赖播放服务或页面存活。[UIDT](https://developer.android.com/develop/background-work/background-tasks/uidt)、[长时间 WorkManager 任务](https://developer.android.com/develop/background-work/background-tasks/persistent/how-to/long-running)。

### 附件与离线封面接线

先 `importWork`，再按用户选择调用 `enqueueAssets(imported.assetIds)`、`cacheCover(imported.workId)`；音频仍用 `enqueue(selectedVersionIds)`。没有自动下载所有发现页封面的隐式网络任务。只缓存站点提供的主封面，不把任意图片猜作主封面。

下载成功后 `backend.library.assets(workId)` 返回本地 WorkAsset（与 RemoteAsset 共用 id，URI 为 file:）；只读取 available=true 的条目。在线附件的 rootId 是内部命名空间 `online:{workId}`，不是 SAF 根目录，不用于重新授权。图片和封面可通过 ContentResolver/图片加载器读取 URI；库列表、visibleWorks 和 work(id) 的 coverUri 自动优先使用完成的本地封面。刷新在线元数据或远端删除附件不覆盖/删除已下载缓存；删除或启动核对发现缓存缺失才回退。等待 `downloads.awaitReady()` 后可取得本次启动核对后的状态。

字幕上限 8 MiB，图片/封面上限 32 MiB；流式检查大小，字幕按已有 UTF-8/BOM UTF-16 与时间轴解析，图片先读尺寸再降采样解码验证（不改写下载原图）。HTML 错误页和无法解析的附件不发布为本地可用文件。来源签名 URL 在下载时重新解析，稳定附件引用不保存 URL；封面每次实际下载从作品详情重新取得地址。`.part` 不显示为附件、更不用于字幕读取；更名恢复和本地 SHA-256 校验沿用音频流程。

下载字幕后调用 `backend.subtitles.attachDownloaded(versionId, assetId)`，返回 SubtitleSource，再用 `select` 指定原文或已有译文。只允许同作品、已完成且可解析的字幕；同一版本/附件重复绑定返回相同 ID，不猜测原文/译文、不自动更改当前字幕选择。删除附件保留选择与偏移，并将所有对应绑定标为不可用；重新下载后恢复这些绑定。需要换轨时重新核对 versionId，UI 不应沿用已离开的音轨提交。

在线发现、来源 Token 配置、附件选择/字幕绑定及下载管理页面已由 `OnlineTools` 与 Compose 接线。`OnlineTools` 只协调请求和操作：查询以来源/搜索/筛选/详情身份取消旧结果，读取真实 tasks，不执行下载循环。封面预览通过现有 HttpTransport 限流与降采样读取，不附带 Token；显式离线缓存仍由 DownloadRepository 执行。

## 字幕接口

`backend.subtitles.sources(versionId)` / `preference(versionId)` 为 Flow。`attach(versionId, uri, name, persistGrant=true)` 从 OpenDocument 导入；绑定已扫描 asset 时传 false，使用已有树授权。先验证格式后落库。支持 UTF-8、带 BOM 的 UTF-16，SRT/VTT/LRC/ASS/SSA；不静默猜测传统编码。

`select(SubtitlePreference(versionId, originalId, translationId, offsetMs, translationOffsetMs))` 持久保存原文/译文与偏移，两路可独立选择/清空；字幕不能跨版本绑定。`load(versionId)` 返回不可变 `BilingualSubtitles`；用 `active(mediaController.currentPosition)` 获取两路当前 cue，`timeline.cues` 提供完整文本。正 offset 延后显示；译文总偏移为 offsetMs + translationOffsetMs。UI 在偏好/来源变动或版本变化时重新 load，加载结束前核对当前 versionId；不在每个播放 tick 重新读文件。加载失败保留错误反馈，不自行改绑其他版本。

AN007.R26：`load` 同时检查当前作品已索引的本地/已下载/在线字幕，不要求旧作品重新导入。匹配复用 PC 的文件名去双扩展名、语言/音效标记归一化、包含匹配、前导轨号及单轨单附件规则；同目录优先，WAV/MP3 等格式对应目录次优先，避免同名音效版本的次序碰巧决定结果。关联仍按 versionId 独立保存，不合并不同音频版本。只有尚未保存字幕偏好时自动选择最高分可解析来源，本地同等匹配优先；默认选择不落为用户偏好。已有手动选择（包括双空值“不显示”）和偏移始终保留。

在线字幕来源只存 `asmr-subtitle://<remoteAssetId>` 稳定引用，读取时优先已下载附件，否则用来源适配器重新解析地址并在内存解析，最多 8 MiB，不保存签名 URL；读取失败显示错误，可在字幕设置重试。切轨取消旧请求，附件刷新也触发重新匹配；不改播放服务的音轨、队列或进度。资料包导出该稳定附件关联，恢复后重建自动来源；无时间戳 TXT/MD 不当作播放时间轴，未新增识别或对齐任务。

AN007.R27：`SubtitleTimeline.display(positionMs, offsetMs)` 仅供播放页摘句，返回最近已开始时间戳的一组台词，保留至下一时间戳；首次台词前为空，倒退和偏移由传入的真实播放时间重新计算。`active` 仍严格使用字幕结束时间，文件时间轴不改写。句子切换动效不延迟 seek、播放或任何保存操作。

手动绑定已扫描附件时会保留所属 root ID 和相对路径；同一目录重新授权并扫描后，字幕 URI 更新，原有选择与偏移保持不变。

按用户 2026-11-02 决定，Android 不开发字幕自动识别、自动翻译、模型下载/管理或远端代跑接口；已有双语字幕导入与显示保留（AN008 取消）。

## 版本化资料导入/导出（AN009 底层）

入口 `backend.backups`。当前格式为 UTF-8 JSON，`format = "asmr-player.android.library"`、`version = 4`，与 Room schema 8 独立版本化。v4 保存作品入库标记和队列分组；兼容 v1/v2/v3，旧作品按原版本规则保留入库状态；v1 不含音轨喜欢，不覆盖已有音轨喜欢。仅接受该格式，不直接打开桌面 SQLite、桌面备份 JSON 或其他应用的数据文件。资料包是明文用户资料，不是整个应用目录的镜像。

包含根目录逻辑 ID/名称/分组方式、作品/音轨/版本、稳定在线引用、本地来源相对路径、收藏/分级、进度、书签、字幕关联/原译文选择/偏移、资料库内容过滤偏好和已保存队列的稳定版本 ID 顺序。排除媒体与字幕正文、下载文件/任务/缓存、设备绝对路径/URI、持久授权、Token/账号配置、签名链接、DSP/主题等设备设置，以及活动 A–B/睡眠截止。使用者应另行保留原媒体目录；下载附件和封面可用已有 AN006 接口重新下载。

| API | 调用与结果 |
| --- | --- |
| `exportBytes()` | suspend；从 Room 一致性事务读取资料，生成并验证格式后返回 ByteArray，最多 32 MiB / 100000 条记录。 |
| `exportTo(uri)` | suspend；URI 来自 `CreateDocument("application/json")`，写完并关闭后返回字节数。建议文件名 `asmr-library-日期.json`；不申请目录扫描权限。提供者不保证原子更名，失败/取消时不能把目标文件报告为完整备份。 |
| `prepare(uri)` / `prepare(bytes)` | suspend；URI 来自 `OpenDocument`。只读验证 UTF-8、格式版本、字段/类型/大小、重复 ID、相对路径和跨表引用；返回 `PreparedImport`，不写库。 |
| `PreparedImport.preview` | version/createdMs/counts（每种记录数量）/existingRecords/directoryIds/queueSize。directoryIds 是包中所有目录，不等于当前设备全部都需重新授权。 |
| `apply(prepared, policy=KEEP_EXISTING)` | suspend；整包事务合并，返回 inserted/updated/kept、directoryIds 和 queueVersionIds。默认同 ID 保留现有记录；REPLACE_USER_DATA 显式替换入库/喜欢/分级、进度、书签内容、字幕选择/偏移和内容过滤偏好，不改已有物理来源绑定。 |

预览对象保存已验证的不可变内容，apply 不重新读用户文件；应用时重新核对现有库，ID 指向其他作品/版本/来源时整笔回滚。新记录按稳定 ID 插入，重复导入不复制条目；导入不删除未包含记录，不做模糊标题合并，也不会将已授权目录重置为无权限。未知版本、损坏/截断文件、无效引用或身份冲突应显示导入失败；格式/参数异常、IO/数据库异常由 ViewModel 转为对应反馈，CancellationException 继续抛出。

导出前若需要最新播放位置，可先在主线程调用 `LibraryPlayback.save(controller)` 并等待成功。导入不控制播放器，也不覆盖播放服务持有的会话；导入期间仍在播放的版本会继续由服务保存真实进度，后续保存可能更新导入的同版本进度。若用户选择替换进度，UI 应先暂停并保存。`queueVersionIds` 只是恢复候选；需用户明确操作后通过现有播放命令使用，不自动出声、不恢复旧睡眠截止。导入本地来源先设为 unavailable/unbound；在线引用保留，只有实际播放/下载时才请求来源，凭据仍需本机配置。

## 媒体目录重新授权（AN009 底层）

新恢复的目录沿用原 rootId；不要先调用 authorizeRoot 创建另一个目录 ID。以下方法不依赖 Activity；UI 负责系统选择器及结果反馈。

| API | 调用与结果 |
| --- | --- |
| `library.reauthorizationNeeds()` | suspend；返回 directoryIds、audioSourceIds、subtitleSourceIds。结合持久 READ 授权与已记录可用状态生成待处理列表；不逐个打开所有文件，外部移动/删除仍需扫描确认。在线待重下字幕不列为 SAF 文件。 |
| `library.reauthorizationItems()` | suspend；在待授权 ID 上补充目录、音频或字幕名称，供恢复页面展示；不向界面暴露 DAO。 |
| `library.previewReauthorization(rootId, treeUri)` | suspend；只读完整枚举，返回 rootId/directoryTitle/mode/matchedAudio/missingPaths/newAudio。按原音频相对路径对照，不读取整段音频做内容哈希。 |
| `library.reauthorizeRoot(rootId, treeUri, allowMissing=false)` | suspend；重新完整枚举并比对，成功才持久授权、替换根 URI 和事务提交扫描。零匹配拒绝；部分缺失默认拒绝，用户看过缺失列表后才传 true；缺失来源失效但历史保留。失败/取消保留旧绑定。 |
| `library.reauthorizeAudioSource(sourceId, uri)` | suspend；对原单文件导入来源使用 OpenDocument，验证可读后保持 sourceId/versionId 更新 URI；不能用于目录来源或下载来源。 |
| `subtitles.reauthorize(sourceId, uri)` | suspend；对单独导入的字幕使用 OpenDocument，验证旧格式时间轴后保持绑定 ID/偏移；目录字幕随目录扫描恢复，在线下载字幕走重新下载。 |

目录重新授权共用 `scanStates` 和 `cancelScan(rootId)`；AUTO 原目录根据既有作品分组固定 WORK/COLLECTION，避免目录改名导致版本 ID 改变。同 rootId + 相对路径保持收藏、进度、书签及自动/手动字幕关联；内容同名不等于内容相同，UI 必须让用户选择原逻辑目录，不把路径匹配表述为音频哈希验证。已归其他根 ID 的树拒绝重复绑定。

下载字幕在资料包中保存稳定附件引用，导入后为待下载关联；附件重新下载完成时恢复原字幕选择和偏移，不把旧设备的缓存路径当成可读文件。[Android SAF 与持久授权](https://developer.android.com/training/data-storage/shared/documents-files)。

## 当前边界与接线

### 耳机偏好与系统媒体（AN013 / AN016.A）

播放服务独占 `HeadphonePreferences`、实际 AudioTrack 路由监听和 DSP 快照。仅同应用控制器可调用 `HeadphoneCommands.GET/EDIT`。GET 和所有 DSP 命令成功结果均附带 `headphones` JSON 状态与 `config` 有效配置；EDIT 参数 `operation` 支持 `save`（name、可选 id）、`select`（可空 id 为通用）、`common`（当前配置存为通用）、`automatic`（enabled）、`bind`（id、route）、`unbind`（route）和 `remove`（id）。bind 必须携带 UI 看到的 route key，服务拒绝过期路由，未知 key 禁止绑定。自动切换与 UI 参数写入均更新同一 AtomicReference，不另建 DSP 或 Player。

ViewModel 串行处理音效编辑与状态查询，按媒体会话的变更通知刷新。前台单独采样播放进度，响度分析和运行中的睡眠定时器按需更新。Activity 停止时暂停界面采样，后台播放由原媒体服务继续。默认不自动切换，未知输出/自动模式进程恢复使用通用方案。完整语义和设备地址限制见 [兼容说明](../docs/releases/ANDROID_MEDIA_COMPATIBILITY.md)。这些播放偏好目前不包含在资料备份包内。

Media3 自动同步播放状态与媒体动作；`SessionArtwork` 仅加载当前作品封面，依据 `privateMetadata` 输出作品图片或中性应用图标，切歌和隐私更新取消旧请求。不把在线封面 URL、Token 或设备地址放入系统媒体元数据。专属厂商卡片尚未接入，不向 UI 暴露虚构的“灵动岛已支持”状态。

### 界面接线

Compose 已通过 `PlayerViewModel` 与其持有的 `LibraryTools` 接入目录预览/确认/扫描/取消/重新授权、字幕原译文选择/文件导入/作品附件绑定/偏移/时间轴、当前版本书签及 A–B/睡眠入口。播放扩展状态从服务查询，命令与查询串行同步；字幕仅在版本/来源/偏好变化或手动重试时读取。`OnlineTools` 已接入 AN006 在线发现/搜索/详情、来源设置、音频/字幕/图片下载和离线封面；播放依旧发送稳定版本到同一服务。下载字幕在 LibraryTools 识别在线附件命名空间后调用 attachDownloaded，保留同作品及版本校验。未改音频 DSP。suspend 异常由状态持有者转成反馈，CancellationException 继续抛出；离开在线页面取消未提交请求，已入队下载与播放器不依赖页面生命周期。

Room/KSP 固定为 2.8.4 / 2.2.20-2.0.4，沿用 FluxManga 已验证的版本组合与本地 JDK/SDK；未复制其 GPL 应用实现、漫画资源或双空间数据库。[Room](https://developer.android.com/training/data-storage/room)、[SAF](https://developer.android.com/training/data-storage/shared/documents-files)、[Media3 会话](https://developer.android.com/media/media3/session/background-playback)。


## 应用更新（AN009.U 底层）

入口 `AndroidBackend.get(context).updates`。UI 收集 `state: StateFlow<UpdateSnapshot>`，包括 phase、release（版本、说明、发布时间、大小）、downloadedBytes、checkedAt、autoCheck、skippedVersion 和 error。状态为 IDLE / CHECKING / UP_TO_DATE / AVAILABLE / QUEUED / DOWNLOADING / VERIFYING / READY / ERROR，不能根据下载百分比推测可安装。

- `suspend check(manual = true)`：显式检查。自动检查遵守开关和跳过版本；手动检查仍能发现被跳过版本。拒绝同版、降级、不兼容系统、草稿/预发布；清单没有 Android 条目视为没有 Android 更新。与下载/安装准备互斥，已有 READY 包优先保留。
- `setAutoCheck(enabled)` / `skipVersion(versionOrNull)`：保存偏好。正式版启动即安排一次联网检查，每 24 小时安排后续检查；系统可以延迟任务。调试版不在启动时联网，保留手动检查。自动检查只更新状态，UI 决定提示方式。
- `startDownload()`：用户点击后进入 QUEUED 并排入唯一 WorkManager 下载，等待网络与系统调度；Activity 重建不取消。`cancelDownload()` 取消并删除临时文件；失败可再次调用下载。下载不会自动启动安装。进程终止或系统中止后，未完成包重新下载；本接口不承诺 APK 断点续传。完成包写入私有 files/updates 并保存元数据，进程重建重新验签；升级后旧包不再符合版本要求并被清除。
- `suspend prepareInstall(controller)`：只能接到用户“安装”操作。先重新校验包；Android 8+ 未获安装来源授权时返回 `UpdateInstallAction.PermissionRequired(intent)`，UI 打开授权页，回来后让用户重试安装。已有授权时，通过当前 MediaController 的 `LibraryPlayback.prepareUpdate` 等待播放服务暂停、队列/位置入库和 WAL checkpoint 成功，再返回 `UpdateInstallAction.Installer(intent)`。UI 用 Activity 启动返回的 Intent，系统仍要求用户确认。接口本身不启动 Activity、不退出进程、不静默安装。捕获异常并显示错误；授权失败不会暂停播放，保存失败不返回安装 Intent。
- 系统安装器取消后 READY 包仍可重试；不把“返回了 Intent”作为安装成功。安装成功由新进程读取当前包版本确认，Room 使用既有增量迁移与会话恢复，保留本地库和凭据。安装准备失败进入 ERROR，先重新检查/下载再试。

共用 PC 的 GitHub `latest.json` 与 Tauri 公钥；Android 读取 `platforms.android-universal`，版本名与顶层 version 一致，versionCode 必须递增。依次校验 HTTPS 发布地址、长度、SHA-256、Minisign ED（BLAKE2b-512 + Ed25519）、受签名保护的版本、APK 包名/版本/最低系统及当前安装证书。APK 的 Android 安装签名与 Tauri 文件签名是两层不同签名；不接受证书轮换或调试/正式包互换。APK 安装证书沿用首个正式发行版的 keystore。

发布操作见 [统一更新清单与 Android 签名发布](../docs/releases/UPDATING.md#android-与-pc-共用更新清单)。设置 → 应用更新已接入检查、首页提示、等待调度/下载进度、错误、自动检查与忽略版本、安装来源授权跳转和用户确认安装。系统授权返回后需再次点击安装；取消安装不伪报成功。

## 分类与维护页面（AN007.I4 / AN009.UI）

`CollectionTools` 收集分类、版本收藏、历史与跨作品书签，查询切换清除旧结果；界面按生命周期订阅。队列编辑携带界面看到的完整版本顺序，变更前重新比对，过期确认不移除错误音轨。`LibraryPlayback.play` 的可选 `startPositionMs` 覆盖续播位置，用于跨作品书签播放；负数拒绝，未传时保留既有续播规则。

`MaintenanceTools` 在 ViewModel 中协调互斥的导出、导入预览/冲突策略、授权预览与提交、存储查询/封面清理。导出前等待服务保存会话，预览不写库，确认导入才事务合并；文件选择器、确认弹窗和安装 Intent 始终由 Compose/Activity 发起。目录重新授权从目录页与资料恢复页进入同一预览流程，不重新创建根 ID。

## 播放与目录筛选扩展（AN007.R6）

- Room schema 5 添加 `catalog_metadata`，通过 `MIGRATION_4_5` 增量迁移。在线导入/显式刷新写入社团、声优、标签、简介、发行日期和字幕标记；`PlayerScreenState.metadata` 提供生命周期内的缓存快照。该表属于可重新抓取的元数据缓存，不改变资料包 v2，不将该缓存表加入资料包；恢复旧资料后可从详情刷新。
- `LibraryQuery` 增加 tags / voices（AND、忽略大小写）、circle、subtitleOnly、offlineOnly、rating 和 favorite（0 不限、1 已喜欢、2 未喜欢）。关键词也匹配社团和声优；offlineOnly 仅匹配当前标记可用的本地/已下载来源，点击播放仍再次验证文件。字幕包括已知远端字幕标记或本地可用字幕。收藏页同时保持 favoritesOnly，不绕过全局内容过滤。
- `BrowseFilters` 构造来源支持的 `$tag:` / `$-tag:` / `$va:` / `$circle:` / `$age:` 查询，并独立传 subtitle 参数。热门无条件查询使用 `/api/recommender/popular` POST，网络失败回落销量排序；带条件或全年龄限制时采用来源搜索排序。标签建议来自当前来源 `/api/tags/`，失败时保留当前结果中的标签建议并允许手动输入条件。
- `LibraryPlayback.ENQUEUE` 自定义命令传 `versionIds` 与 `next`，在服务中解析稳定身份并追加，不替换当前音轨、位置或播放/暂停状态。加入前后检查队列上限 500，迟到解析不能覆盖后续请求。普通循环模式使用 MediaController.repeatMode，由服务持久化，恢复不会自动播放。
- `PlayerViewModel.playGroup(ids, fromStart, startVersion)` 按选中分组播放，继续收听保持完整组队列并定位最近未完成的版本。`OnlineTools.act` 支持 play/restart/next/enqueue/favorite-track 及原有保存/下载操作，成功回调在导入与命令提交后触发，用于页面跳转；离开详情仍取消未提交任务。
- `DetailTools` 管理详情附件、字幕预览、显式关联、远端信息刷新及下载反馈，不持有播放器。纯台词 UI 只消费当前稳定版本的字幕与服务位置；切换版本时不展示上一轨台词。

筛选集合、台词字号/模式是本机 UI 偏好，不写入媒体资料包。自动识别、翻译和模型功能仍不实现。在线台词仅显示已有字幕文件。

## AN018 跨设备同步

`AndroidBackend.sync` 提供 select、status、syncAuthorized、sync、logout、setAutoPolicy 和 resolve。SyncProvider 隔离绑定、有效令牌获取及快照下载/上传；仓库序列化手动与自动同步，按提供方保存状态。OneDrive 使用 AppAuth 和 PKCE，GitHub Gist 支持设备授权与访问令牌；WebDAV 通过 HTTPS 和条件 PUT 合并更新。Supabase 使用随 APK 打包的官方 JavaScript SDK 登录邮箱账号，通过 user_sync 表和 RLS 按账号同步。

Room schema 6 的 `device_sync_state` 保留设备身份、合并日志、基线和指纹缓存。在一次数据库事务中应用合并结果并保存日志。PC 的 `device_sync_command` 与本端读写同一格式。见 [存储配置与同步说明](../docs/releases/DEVICE_SYNC.md)。

`SyncScheduler` 保存独立的 foreground / periodic / intervalMinutes 偏好，`setAutoPolicy` 从设置接线；MainActivity 前台触发和 WorkManager 唯一周期任务共用 `SyncRepository.sync` 的执行互斥。后台调度受系统约束，失败不清空本地数据。偏好与连接保存在本机，不同步给其他设备。

每台设备先读取选中存储中的资料快照，再发布以本机设备 ID 命名的快照。退出时清除本机同步账号。

## AN010 / AN011 / AN012 扩展接口

- `PlaybackWidget` 只展示 PlaybackService 发布的已处理隐私的元数据；点击经 Activity 连接 MediaController，等待会话恢复再发送既有控制。小组件自身不维护队列或播放进度，不定期轮询服务。
- `IncomingTools.state` 提供分享预览、文件名、目标作品 ID、进度与错误；`receive(Intent)` 只读取元信息，`importFiles(versionId?, translation)` 经用户确认复制并入库，`dismiss()` 取消未提交复制。字幕与选用偏好在同一 Room 事务提交。已导入副本通过 `${applicationId}.imports` FileProvider 读取；LibraryRepository 仅对该内部 authority 跳过持久授权申请。
- `LibraryPlayback.setSleep(controller, deadlineEpochMs?, endOfTrack, fadeMs?)` 新增可选淡出时长（0–300000 ms），省略时保留偏好。`extendSleep(controller, minutes)` 支持 1–60 分钟，等价自定义命令 SET_SLEEP 的 `extendMinutes`；以现有截止与当前时间较晚者为基准，切换为分钟截止模式。原有取消/截止命令兼容。
- GET_STATE 新增 `fadeMs` 与 `sleepGain`（0–1）；渐弱仅乘在播放器原音量上，取消/暂停/结束恢复原音量，不改变系统音量和 DSP 参数。服务 100 ms 检查，纯音量事件不触发逐帧会话落库或小组件更新。

## AN010.R4 / R5 桌面组件透明材质与封面背景

`PlaybackWidget.publish(context, player)` 在 Media3 所在线程读取已经脱敏的元数据和真实进度，离屏材质在后台生成并按配置/尺寸缓存；无第二播放器或单独播放时钟。播放中且已放置组件时每 15 秒更新实际位置，暂停/服务退出停止。R8 的底部操作使用显式不可变 Broadcast PendingIntent → 非导出 PlaybackWidget → WidgetActions 临时 MediaController → LibraryPlayback.WIDGET_ACTION；不启动 Activity。连接/命令最长等待 8 秒，结束释放 controller 和 goAsync。空白处用 OPEN_PLAYER Activity PendingIntent，外观按钮保持专用设置 Activity。

`WIDGET_ACTION` 仅向同包 controller 开放；参数 operation 为 play/pause/previous/next/favorite/library，携带 versionId，favorite 另带明确布尔目标。LibrarySessionCoordinator 等会话恢复后核对当前版本，再控制唯一播放器或写库；旧切歌点击同样拒绝，play/pause 和喜欢目标避免重复点击反转。喜欢走 `library.collections.setVersionFavorite`，入库走 `library.setMembership(workId, true)`；重复入库只反馈。旧 Activity 入口只打开播放器，不重放旧命令。`LibraryCollectionsDao.observeWidgetCollection(versionId)` 只返回版本/作品身份与两个布尔状态，不读取或公开未脱敏标题/封面；切轨取消旧观察，Room 更新直接刷新已放置组件，未知版本禁用两个资料按钮。服务退出保留暂停的展示快照，下一次按钮通过服务恢复会话；不单独持久化组件播放状态。

`WidgetGlassStore` 按组件 ID 保存深色/浅色、背景浓度、实色与版本号，不保存新壁纸图片或坐标。`removeLegacyImage` 只清理对应的旧私有副本及映射键，不处理用户原图。`WidgetGlass.render(widthDp,heightDp,config,cover?)` 使用可选当前封面生成居中裁切/模糊/遮罩背景；无封面则生成真实透明底。R6 共用单条渐隐高光与轻外部阴影，移除内圈及面状白光。这里不处理桌面壁纸，也没有桌面实时模糊/折射。

`SessionArtwork.hasCover(id)` 区分真实当前封面与应用图标占位，服务在脱敏后的 MediaMetadata extras 写入 `hasWorkArtwork`。组件只有该标记为真时才使用 artworkData 生成背景；隐私隐藏及失效版本均不泄漏旧封面。渲染缓存包含版本和封面内容，背景不按播放进度重复模糊。`privateMetadata` 首次默认 false，保留已存值，SET_PRIVACY 缺失目标值不改变当前选择。

`WidgetConfigurationActivity` 验证本应用组件身份，提供示意背景上的同布局预览，草稿仅在“应用到桌面”保存；取消保留原外观。预览不注册真实播放 PendingIntent。R3 的布局、喜欢/入库操作和媒体隐私保持不变。


## 保存的队列分组（AN007.R23）

Room v8 通过 `MIGRATION_7_8` 新增 `queue_groups(id,name,versionIds,sortOrder)`，不改运行会话表。`backend.queueGroups.groups` 提供排序后的 Flow；`create/rename/add/remove/move/moveGroup/delete/restore` 均以事务写入。名称 1–80 字符，音轨使用稳定 version ID，最多 500 首，加入时去重并检查版本存在；重排传入预期顺序以拒绝过期拖动。删除仅移除分组记录。

UI 选择播放时仍调用既有 `PlayerViewModel.playGroup` / `LibraryPlayback.play`；浏览分组、添加和移出不会改变当前服务队列、播放位置、喜欢或入库状态。分组不拥有独立播放器或播放游标。

资料包当前版本为 v4，增加 queue_groups，并校验组内版本引用。v1/v2 按既有规则升级后，v3 补空分组升级到 v4；KEEP_EXISTING 保留本机同 ID 分组，REPLACE_USER_DATA 显式覆盖名称、顺序与音轨列表。此版本没有增加跨设备同步分组协议。


### 歌单跨端同步与操作入口（2026-11-10）

`queue_groups` 与 PC 的独立 `playlists` 通过 schema 1 可选 `playlistRecords` 交换。记录以共享歌单 ID 标识，内容包含 name、最多 500 个便携音轨身份和 sortOrder；删除为 tombstone，沿用现有修订与冲突处理。匹配到本机稳定版本后写入原表，未知引用保留 `sync:<SHA-256>` 占位，不靠标题猜测音源；重新同步后可转成本地版本。在线音源描述、指纹和经验证的在线对应关系复用现有同步 Catalog。

`PlayerViewModel.choosePlaylist` 只打开全局选择歌单抽屉，`QueueGroupTools` 完成加入或新建；不发播放命令。作品长按通过 preferredPlaylistVersions 选择一个优先目录并按音轨优选格式，单曲传入单个版本。运行队列仍由唯一 MediaSession 服务持有。
