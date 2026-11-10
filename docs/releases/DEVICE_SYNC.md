# 跨设备同步与存储提供方

## 在电脑和手机上使用

在两端进入「设置 → 跨设备同步」，选择同一种存储，并绑定同一账号。首次绑定立即拉取并上传资料。随后可以点击「立即同步」，或由后台周期任务执行同步。

| 存储方式 | 绑定步骤 | 云端位置 |
| --- | --- | --- |
| GitHub Gist | 点击「登录 GitHub 并同步」，在 GitHub 官方设备授权页输入播放器显示的短码，确认 gist 权限。也可选择「使用访问令牌」。 | 每台设备一个秘密 Gist，说明为 `ASMR Player sync v1`。知道链接的人可以读取其中的数据。 |
| WebDAV | 填写两端相同的已有 HTTPS 文件夹、用户名和密码或应用密码，点击「连接 WebDAV 并同步」。 | 文件夹内的 `asmr-player-sync-v1.json`，服务须支持强 ETag 与条件 PUT。 |
| OneDrive | 点击登录，使用系统浏览器选择 Microsoft 账号并授权。 | Microsoft Graph 的 `/me/drive/special/approot` 应用文件夹。 |
| Supabase | 输入邮箱及密码，点击「登录并同步」。首次使用可注册账号，并按邮件提示确认邮箱。 | `user_sync` 表按账号和设备保存 JSON 快照及服务器更新时间。 |

切换存储会保留其他方式的绑定、上次同步时间和自动同步偏好。只有当前选中的方式执行同步。退出只解除当前方式在本机的绑定。各方式使用同一份本机收藏、书签、歌单、进度和标记；切换后执行同步，将把这些本机资料与新选中的存储合并。

音频库和喜欢分别同步。在线作品会附带来源 ID、名称和分级，含收听记录或标记的在线音轨还会附带远端文件 ID、来源目录内的相对路径及文件大小。Android 收到后可直接建立对应的在线音轨并应用记录；只恢复听音历史的作品不会因此加入音频库或喜欢页。升级后先在有原始资料的设备同步，再让手机拉取，即可补齐旧快照。

歌单同步名称、音轨顺序、歌单顺序和删除操作，每个歌单最多 500 首。PC 在播放栏「控制与音效 → 歌单」打开右侧抽屉；Android 在歌单网格管理。PC 右键和 Android 长按作品或音轨可「加入歌单」：作品使用优先目录和版本，单曲只加入该首，重复音轨只保留一份。加入歌单不会启动播放，也不会改变作品的音频库或喜欢归属。

两端升级到支持歌单的版本后执行同步。在线音轨附带稳定来源引用，接收端可建立在线音源；本地音轨沿用文件指纹或已验证的在线对应关系。暂未找到的音轨保留在原位置并显示待匹配；可播放部分按原顺序播放。删除歌单保留文件和当前播放队列。并发修改同一歌单时，冲突页可选择完整歌单版本。运行队列不作为歌单上传。旧队列方案记录保留兼容，播放队列抽屉仅展示当前运行队列。协议使用 schema 1 的可选 `playlistRecords` 字段，旧客户端可继续读取原有资料，但不能管理同步歌单。

本地音频按 SHA-256 匹配。如果已核实本地版本对应具体的在线目录和音轨，同步可以通过原文件指纹恢复在线版及其进度。没有可靠对应关系的记录保留待匹配。首次同步较大的本地库可能需要读取音频计算指纹。媒体文件由各设备管理。

接收端收到在线作品身份后，使用本机设置的在线来源查询作品资料，在本地补全标题、作品编号、社团、CV、标签、封面、简介和发布日期。既有同步条目也会在升级后的同步中补查。资料查询不下载音频，不改变喜欢、音频库归属、进度或标记。来源暂不可用时保留已收到的用户资料，设置页显示待补全数量，下次同步重试；已成功补全的来源不在每次同步重复查询。

两台设备离线修改同一条记录时，「处理并发修改」可以选择保留的版本，随后同步即可让另一端收到结果。首次授权后，未设置过同步偏好的设备默认每 30 分钟后台同步一次。可在「自动同步」中关闭、调节间隔，或开启「回到应用时同步」。已有设备保留已保存的选择。Windows 在播放器运行时执行，Android 由 WorkManager 在联网时调度。

## 开发者配置

### Supabase

两端使用同一个项目的公开配置 `config/supabase.json` 和官方 `@supabase/supabase-js` SDK。Windows 使用主窗口的 WebView，Android 将 SDK 随 APK 打包，由同步任务在独立 WebView 中调用。构建 Android 前在项目根目录运行 `pnpm install --frozen-lockfile`。刷新令牌分别保存在 Windows 凭据管理器与 Android Keystore 加密的本机存储中。

数据库迁移位于 `supabase/migrations/202611100001_user_sync.sql`。每次同步使用 select 读取当前账号的设备快照，合并后以 `(user_id, device_id)` 为键 upsert 本设备的快照。RLS 的 select、insert 和 update 策略均校验登录用户 ID。时间戳由数据库写入。自部署时应同时更新两端 SDK 允许连接的项目域名。

项目启用了邮箱确认。内置邮件服务可用于项目团队邮箱的联测；面向普通用户开放自助注册前，发布者需要在 Authentication 中配置可向外部邮箱投递的 SMTP 服务。[Supabase 邮件服务说明](https://supabase.com/docs/guides/auth/auth-smtp)

### OneDrive

1. 在 Microsoft Entra 注册一个公共客户端应用，账号类型应覆盖计划支持的 Microsoft 账号。PC 与 Android 使用同一 Application (client) ID，从而访问同一应用文件夹。
2. 在「移动和桌面应用程序」平台登记桌面回调 `http://localhost` 和 Android 回调 `msal<小写 client-id>://auth`。桌面端在本机回环接口监听临时端口，Android 用 AppAuth 通过浏览器 Custom Tabs 接收自定义 Scheme 回调。
3. 为应用配置委托权限 `Files.ReadWrite.AppFolder` 和 `offline_access`。授权请求使用 PKCE S256；刷新令牌时沿用同一权限集合，服务返回新 Refresh Token 后先写入系统凭据存储再继续同步。
4. ASMR Player 已在 Microsoft Entra 注册，支持个人 Microsoft 账号和组织账号。Windows、Android 和 GitHub Actions 默认使用 `config/onedrive-oauth-client-id.txt` 中的公开客户端 ID。使用自行注册的应用构建时，可通过 `ASMR_ONEDRIVE_CLIENT_ID` 覆盖，两端需使用同一值。

GitHub 使用本项目注册的 OAuth App，公开 Client ID 位于 `config/github-oauth-client-id.txt`，由两端构建读取。设备流程仅申请 `gist`，设备码在本机内存中等待授权，授权结果存入系统凭据存储。启用令牌过期时，后续同步会用 Refresh Token 自动续期。用户也可以绑定细粒度的 Gists 读写令牌或仅含 gist 权限的经典令牌。[GitHub Gists API](https://docs.github.com/en/rest/gists/gists) · [Microsoft Graph 应用文件夹](https://learn.microsoft.com/en-us/graph/onedrive-sharepoint-appfolder) · [Microsoft 授权码流程](https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-auth-code-flow)

## SyncProvider 接口

桌面端的 Rust trait 和 Android 的 Kotlin interface 分别位于 `src-tauri/src/services/device_sync/providers.rs` 和 `android/app/src/main/kotlin/com/asmrplayer/android/sync/SyncProvider.kt`。设置页负责选择和发起操作，仓库负责选中方式、同步互斥和本机事务，提供方负责凭据及远端文件。

| 方法 | 职责 |
| --- | --- |
| login / logout / getStatus | 绑定、解除本机绑定和读取此提供方状态。界面只接收是否配置和是否绑定。 |
| accessToken | 为一次同步提供有效凭据。OAuth 提供方静默刷新并保存轮换令牌。 |
| downloadSyncData | 将该提供方所有匹配快照合并到本轮文档，返回更新本机快照所需的远端标识。 |
| uploadSyncData | 首次创建或覆盖本机的快照。其他设备的文件保留到各自下次同步。 |

OneDrive 的 Refresh Token、GitHub 令牌由 Windows 凭据管理器或 Android Keystore 加密存储，各提供方使用独立条目。GitHub 首次创建秘密 Gist，此后更新自己的 Gist。Gist 的 REST 下载路径限制单份快照为 10 MB，超出时可选择 OneDrive 或 WebDAV。OneDrive 从 `/me/drive/special/approot/children` 分页读取列表，通过临时下载地址读取文件，再 PUT 到 `/me/drive/special/approot:/asmr-sync-v1-<设备 ID>.json:/content`。请求临时下载地址时不附带账号令牌。

两端已有的资料匹配、冲突处理和数据库事务由同步仓库统一执行。新增提供方时实现上述接口、注册存储标识并添加设置选项，沿用相同的版本化 JSON 快照。远端快照损坏或拉取失败时，本轮不上传，成功时间只在云端写入及本机合并完成后更新。注销会保留本机资料和远端快照。

### WebDAV 并发与凭据

WebDAV 是独立的资料同步提供方。两端 GET 同一个版本化 JSON 文件并按现有修订规则合并。首次创建使用 `If-None-Match: *`，后续写入使用 GET 返回的强 ETag 和 `If-Match`。遇到 412 会重新拉取并合并，连续三次冲突时保留本地资料，提示稍后重试。已有文件缺少强 ETag 或格式无效时停止上传。请填写支持 HTTPS、Basic 认证和条件写入的已有文件夹；密码或应用密码由 Windows 凭据管理器或 Android Keystore 加密保存。
