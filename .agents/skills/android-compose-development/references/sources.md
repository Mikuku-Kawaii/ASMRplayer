# 来源与合并说明

合并日期：2026-10-15。本地定制版，非上游官方发行版，不自动同步更新。

## 实际合并来源

- Google LLC，[android/skills](https://github.com/android/skills/tree/5bda39f154fb859a08d45e2392f3568e049f83fb)，Apache-2.0：
  - [navigation-3](https://github.com/android/skills/blob/5bda39f154fb859a08d45e2392f3568e049f83fb/navigation/navigation-3/SKILL.md)
  - [edge-to-edge](https://github.com/android/skills/blob/5bda39f154fb859a08d45e2392f3568e049f83fb/system/edge-to-edge/SKILL.md)
  - 保留完整 [Apache 许可证](../licenses/android-Apache-2.0.txt)。
- Adit Lal，[compose-expert](https://github.com/aldefy/compose-skill/blob/954ef54ea32288fbc90745f012d09d7b791f0d8a/skills/compose-expert/SKILL.md)：
  - 原技能说明采用 MIT；保留完整 [上游许可证](../licenses/compose-expert-LICENSE.txt)。
  - 上游许可证另说明其 AndroidX 源码摘录为 Apache-2.0；本技能未打包那些源码摘录。

## 编辑与取舍

所有指令文件均为本地重新组织、删减和补充后的版本，未原样复制上游 SKILL.md。

合并重叠的状态、布局、性能与导航路由，保留平台限制。删除上游插件安装宣传、自动 session 初始化、PR URL 自动改变任务模式、组件固定分类流程和与 Android 无关的平台清单。将版本升级与导航迁移改为按用户需求和现有工程决定。

多空间异步身份、Rust / UniFFI 验证和范围控制为本地补充；不视为每个 Android 项目的必需架构。没有复制上游脚本、模板工程或未读取的参考文件。

## API 核对入口

具体实现以当前依赖为准，按需查询：
- [Compose 状态](https://developer.android.com/develop/ui/compose/state)
- [Compose 副作用](https://developer.android.com/develop/ui/compose/side-effects)
- [Navigation 3](https://developer.android.com/guide/navigation/navigation-3)
- [Compose Window Insets](https://developer.android.com/develop/ui/compose/system/insets)
- [Compose 性能](https://developer.android.com/develop/ui/compose/performance)

这些是核对入口，不意味着本地合并版已编译验证所有 API 组合。

## 2026-10-20：AI_Skills 增量合并

用户指定来源：Mikuku-Kawaii/AI_Skills，固定提交 `57e3bcdf765cab84896a3e4d82e56c02a7708e20`。采用以下技能的适用规则，重新编写为本技能参考；不是完整上游同步：

- [ui-accessibility](https://github.com/Mikuku-Kawaii/AI_Skills/blob/57e3bcdf765cab84896a3e4d82e56c02a7708e20/skills/ui/ui-accessibility/SKILL.md)
- [ui-defensive-layout](https://github.com/Mikuku-Kawaii/AI_Skills/blob/57e3bcdf765cab84896a3e4d82e56c02a7708e20/skills/ui/ui-defensive-layout/SKILL.md)
- [ui-state-feedback](https://github.com/Mikuku-Kawaii/AI_Skills/blob/57e3bcdf765cab84896a3e4d82e56c02a7708e20/skills/ui/ui-state-feedback/SKILL.md)
- [ui-workspace-layout](https://github.com/Mikuku-Kawaii/AI_Skills/blob/57e3bcdf765cab84896a3e4d82e56c02a7708e20/skills/ui/ui-workspace-layout/SKILL.md)

通用原则经 Android / 液态玻璃语境整理后写入 `ui-quality.md`；原有来源、许可证与平台分工保留。性能优先与移植前评估来自用户要求，不声称上游已验证本项目性能。

排除原项目的 `apps/web`、设计文档依赖、Radix / CSS 命令、固定桌面尺寸、冷铂灰蓝主题、ASMR / 音频 / Material 3 专项与知识证据流程；不引入自动委派、上传或同步。原仓库此提交未提供根 LICENSE，不将其标为 MIT / Apache 或授权再分发的代码包；此处为按用户指定来源整理的本地规则，未复制实现代码。

## 2026-10-20：第二次增量更新

固定提交 `e2484473c5c72e13b4e7eea28226e7e48c8ba891`，相比首次合并新增设置设计与文字呈现技能；本地两个专项已安装为 `ui-settings-design` 和 `ui-text-presentation` 的适配版，按需配合，不重复复制其完整内容。

- [ui-defensive-layout 增量](https://github.com/Mikuku-Kawaii/AI_Skills/blob/e2484473c5c72e13b4e7eea28226e7e48c8ba891/skills/ui/ui-defensive-layout/SKILL.md)：说明 / 控件空间预算、空槽、保存区遮挡。
- [ui-design-system 增量](https://github.com/Mikuku-Kawaii/AI_Skills/blob/e2484473c5c72e13b4e7eea28226e7e48c8ba891/skills/ui/ui-design-system/SKILL.md)：控件依据提交语义选择、短按钮与行内操作。
- [ui-state-feedback 增量](https://github.com/Mikuku-Kawaii/AI_Skills/blob/e2484473c5c72e13b4e7eea28226e7e48c8ba891/skills/ui/ui-state-feedback/SKILL.md)：草稿、即时 / 保存型配置与未知回执。

适用规则合入 `ui-quality.md`，原有 Android / Liquid Glass 分工、性能优先规则和许可文件保留。未引入来源项目的 Web 文档路径、固定侧栏、全局按钮样式，也未导入仓库 workflow.md 或更改项目 AGENTS.md。

## 2026-10-25：UI 设计方法增量吸收

固定来源：[Mikuku-Kawaii/AI_Skills](https://github.com/Mikuku-Kawaii/AI_Skills/tree/1ea43045c99ed9f8f862e7bc3e08abde7b8bccf4)，提交 `1ea43045c99ed9f8f862e7bc3e08abde7b8bccf4`；以该快照提炼适用于现有技能的判断规则，不原样同步仓库。

- [ui-design-system](https://github.com/Mikuku-Kawaii/AI_Skills/blob/1ea43045c99ed9f8f862e7bc3e08abde7b8bccf4/skills/ui/ui-design-system/SKILL.md)
- [ui-workspace-layout](https://github.com/Mikuku-Kawaii/AI_Skills/blob/1ea43045c99ed9f8f862e7bc3e08abde7b8bccf4/skills/ui/ui-workspace-layout/SKILL.md)
- [ui-state-feedback](https://github.com/Mikuku-Kawaii/AI_Skills/blob/1ea43045c99ed9f8f862e7bc3e08abde7b8bccf4/skills/ui/ui-state-feedback/SKILL.md)
- [ui-design-quality](https://github.com/Mikuku-Kawaii/AI_Skills/blob/1ea43045c99ed9f8f862e7bc3e08abde7b8bccf4/skills/ui/ui-design-quality/SKILL.md)

吸收主任务与信息收纳、角色化排版 / 留白、实际状态反馈、产品动效与展示叙事的区分；八维视觉判断只用于当前相关范围。保留既有 Android / 液态玻璃、参考图、配色与提交协议，不导入 Web 专属路径 / 原语、固定桌面尺寸、营销页结构、ASMR / 音频或知识证据流程，不新增上传 / 委派权限。

上游部分方法注明来自 Elaya `38bbe5765600b9178b5395a823c9f6c8d2b8e730`；本次未复制其落地页 / 动画导出技能、素材、脚本或模板。下载快照未见根 LICENSE，不为整个仓库自行声明 MIT；原有本地许可文件不变。用户当前测试与打包约定优先：暂停时不执行或新增测试任务，源码检查不等同于视觉 / 性能通过。
