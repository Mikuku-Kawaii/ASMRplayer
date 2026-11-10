# Android 耳机偏好与系统媒体兼容

实现范围核对：2026-11-02。设备测试结果单独记录，不能将标准接口接入解释为所有厂商胶囊均已通过。

## 耳机音效偏好（AN013）

入口：右上角三横 → 设置 → 耳机音效偏好，或音效与均衡器页面。

- 保存最多 32 个命名方案，包含完整 DSP 参数与原声旁路。可手动应用、用当前音效更新、删除；通用音效可单独保存与应用。
- 播放服务监听 Media3 实际创建的 AudioTrack 输出，用户明确关联设备后才自动切换。切换只更新已有 DSP 快照，不重新打开音频、重建播放器或移动进度。
- 自动切换默认关闭。开启后，未关联、未知或已断开的输出使用通用音效；进程重建先恢复通用音效，实际路由确认后再应用关联方案。
- 扬声器/听筒/模拟有线耳机以接口关联，不能辨认同一插孔上更换的耳机。蓝牙/USB 等使用系统提供的地址生成本机散列键；空值和通用隐私占位地址不能绑定。系统地址变化后需重新关联，不靠设备名称猜测音效或 AutoEQ。
- 暂停时系统可能不返回当前路由，页面明确显示“最近确认的输出”。可随时手动切换；自动模式下手动调整会保持到下一次输出变化。
- 方案与关联保存在本机播放偏好中，尚未加入 AN009 资料备份格式。

实际输出与首选输出不同，路由查询需以正在播放的 AudioTrack 为准：[Android AudioRouting](https://developer.android.com/reference/android/media/AudioRouting)。

## 通用媒体接口（AN016.A）

保留唯一 Media3 MediaSessionService 和 ExoPlayer，提供标准 MediaStyle 通知、系统媒体会话、播放状态/进度、当前可用操作与通知点击入口。新建“音频播放”通知渠道与单色通知图标。系统媒体页提供通知状态、渠道状态、隐私开关及系统设置入口；从系统设置返回后重新读取状态。

当前作品封面在后台有界加载，复用播放页原图选择器，本地附件优先于同名远端副本；无明确命名的附件可与小封面抽样比对，远程封面不附带来源 Token。会话只保留当前封面的图片字节，按原比例缩到最长边 1536px，小图不放大；编码上限 768KiB，不向系统元数据暴露封面 URL。隐藏开关默认关闭；开启后标题使用 ASMR Audio Track、封面使用应用图标，小组件背景回退为中性底色。升级保留用户明确保存的选择。切歌/隐私切换会取消旧请求并检查代次，避免过期封面回写。

Android 13 及以上系统媒体界面以会话状态和 MediaMetadata 为基础；较旧系统使用 MediaStyle 通知动作。媒体卡片不是应用自绘的悬浮窗。参见 [Media3 后台播放](https://developer.android.com/media/media3/session/background-playback)、[Android 媒体控制](https://developer.android.com/media/implement/surfaces/mobile)。

## 厂商范围与真实限制

| 平台 | 本次提供 | 尚不能保证的部分 |
|---|---|---|
| OPPO / 一加 ColorOS | 标准媒体会话、通知、封面与控制；设置页提供流体云查找说明 | 流体云是否收录本应用及音乐场景；未接入厂商专属卡片接口 |
| 小米 / REDMI HyperOS | 同一标准媒体接口 | 超级岛专属卡片需应用身份、签名配置、场景申请与审核；本次没有办理准入 |
| 三星 One UI | 标准活动媒体状态与控制，可供系统音乐入口消费 | Now bar 的机型/版本/显示策略及本应用具体表现未真机验证 |
| vivo / iQOO OriginOS | 标准媒体接口 | 原子岛/原子随身听是否采纳本应用由系统决定；没有伪造私有参数 |
| 荣耀 MagicOS | 标准媒体接口 | YOYO 专属场景涉及厂商申请与审核，尚未集成 |
| 其他 Android、仍支持 APK 的系统 | 标准媒体接口，按系统提供的通知和锁屏能力展示 | 系统修改、后台策略与第三方胶囊行为；原生 HarmonyOS 应用不属于 Android APK 移植范围 |

官方依据：[ColorOS 流体云说明及设置入口](https://www.coloros.com/article/A00000075/)、[小米超级岛接入流程](https://dev.mi.com/xiaomihyperos/ability/XiaomiHyperIsland)、[三星 Now bar 音乐控制](https://www.samsung.com/ie/support/mobile-devices/how-to-use-the-now-bar-on-the-lock-screen-of-your-samsung-galaxy-device/)、[荣耀 YOYO 场景申请](https://developer.honor.com/cn/doc/guides/101455)。

Android Live Updates 有指定通知样式与使用场景，MediaStyle 不在其合格样式列表中。本项目使用正确的媒体通道，不另外发布假导航/通话/配送进度来占用系统胶囊：[Android Live Updates](https://developer.android.com/develop/ui/views/notifications/live-update)。

本轮可在标准 Android 模拟器验证媒体契约和页面；ColorOS、HyperOS、One UI、OriginOS、MagicOS 的实际胶囊呈现、蓝牙/USB 实体切换均单独保留设备验收缺口。厂商专属接口是真正未实现的能力，和“已实现但没有真机验证”分开维护。用户当前无需提供设备测试。
