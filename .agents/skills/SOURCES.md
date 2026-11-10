# 项目技能来源与适配

核对日期：2026-11-02。仅合并到 ASMR Player 的 `.agents/skills/`，不修改全局技能，不提交或推送任何仓库。

| 上游 | 固定提交 | 纳入范围 |
| --- | --- | --- |
| [Mikuku-Kawaii/AI_Skills](https://github.com/Mikuku-Kawaii/AI_Skills) | `c986aca29fc608e35b1c09bd976d9abf07445328` | 10 个通用 UI 技能，改写为本项目适配版；已有 3 个 ASMR 技能与远端文本一致，原位保留 |
| [pbakaus/impeccable](https://github.com/pbakaus/impeccable) | `e89e3ada3e875a89ac2c5d33d11e65ebef6fe285` | 完整技能资源；入口改为 ASMR Player 审查与精修流程 |
| [Leonxlnx/taste-skill](https://github.com/Leonxlnx/taste-skill) | `12af471f627f866411b4bd785b19646d3672784c` | 完整技能资源，以 `design-taste-frontend` 名称安装；入口适配应用屏幕 |
| [Mikuku-Kawaii/AI_Skills](https://github.com/Mikuku-Kawaii/AI_Skills/tree/c986aca29fc608e35b1c09bd976d9abf07445328/skills/ui/liquid-glass-bottom-navigation) | `c986aca29fc608e35b1c09bd976d9abf07445328` | 2026-11-02 完整导入 `liquid-glass-bottom-navigation` 及 references，原样保留，仅作为 Android 可选底栏参考 |
| 本机 `~/.codex/skills/android-compose-development` | metadata 1.3.0 / updated-on 2026-10-25；2026-11-02 复制 | 完整导入 `android-compose-development` 的入口、agents、references、licenses；不是中央仓库内容，来源与固定上游提交见其 [sources.md](android-compose-development/references/sources.md) |

## 按任务加载

- 壳与导航：[ui-workspace-layout](ui-workspace-layout/SKILL.md)
- 设置与搜索：[ui-settings-design](ui-settings-design/SKILL.md)
- 任务与错误：[ui-state-feedback](ui-state-feedback/SKILL.md)
- 文字与长内容：[ui-text-presentation](ui-text-presentation/SKILL.md)、[ui-defensive-layout](ui-defensive-layout/SKILL.md)
- 共享样式：[ui-design-system](ui-design-system/SKILL.md)
- 键盘与可读性：[ui-accessibility](ui-accessibility/SKILL.md)
- 动效：[ui-motion](ui-motion/SKILL.md)
- 审查与验收：[ui-design-quality](ui-design-quality/SKILL.md)、[ui-visual-regression](ui-visual-regression/SKILL.md)
- 外部方法：[impeccable](impeccable/SKILL.md)、[design-taste-frontend](design-taste-frontend/SKILL.md)
- Android 架构、状态、导航与平台行为：[android-compose-development](android-compose-development/SKILL.md)；移植取舍见 [架构评估](../../docs/releases/ANDROID_PORT.md)
- Android 可选玻璃底栏：[liquid-glass-bottom-navigation](liquid-glass-bottom-navigation/SKILL.md)，仅在具体视觉任务要求时使用，不替换默认 M3 方向

## 适配决定

通用 UI 原技能含 LOCI 的 `apps/web`、Radix、冷铂灰蓝、知识图谱与专属测试命令。保留其任务优先、状态真实、信息分层、布局防溢出、可访问性和证据方法，删除这些项目专属假设，改为当前 `src/app`、`src/features`、`src/components` 与 M3 规范。没有引入新的 DESIGN_SYSTEM.md 作为第二份样式权威。

外部两个技能保留 references/reference、agents、assets、scripts 中原有配套文件；改写 SKILL.md 入口，按需使用参考资料。未执行其脚本、下载运行时、安装 hooks、启动 CDP 或注入运行中播放器。上游参考中的此类命令及子代理流程不自动成为本项目要求。M3、用户主题和明确任务范围优先于上游审美偏好。

2026-11-01 的 Windows 审查未纳入知识图谱、营销页、视频导出与两项 Compose 液态技能；2026-11-02 因 Android 规划补入 `liquid-glass-bottom-navigation`。继续不纳入 `ui-knowledge-evidence`、`landing-page-design`、`design-to-motion`、`liquid-world-transition`：当前移植没有知识工作区、宣传素材或双空间切换需求。

Android 使用 Compose 的生命周期、导航、系统边距与平台服务规则；现有项目 UI 适配入口中的 React 路径和 pnpm 验证命令仅适用于桌面。架构与 DSP 技能中的领域身份、并发结果隔离、声道与限幅原则继续适用，Windows 加载器、WASAPI、SMTC 和历史 sidecar/IPC 描述不作为 Android 实现要求。普通主题/玻璃视觉不增加账户空间、数据库隔离或悬浮窗权限，也不决定播放和下载时序。

## 许可与维护

- Impeccable：保留 [Apache-2.0 LICENSE](impeccable/LICENSE) 与 [NOTICE](impeccable/NOTICE.md)。修改范围为本地 SKILL.md 入口，其余资源保留安装版本。
- Taste：保留 [MIT LICENSE](design-taste-frontend/LICENSE)，修改范围同上。
- Android Compose：完整保留 [来源说明](android-compose-development/references/sources.md)、[Android Apache-2.0](android-compose-development/licenses/android-Apache-2.0.txt) 和 [Compose 上游许可](android-compose-development/licenses/compose-expert-LICENSE.txt)，本轮未修改技能正文或全局原件。
- 玻璃底栏：中央目录只有 SKILL.md 与 references/material-and-tabs.md，两者均完整保留；上游未附单独 LICENSE，不为其擅加统一许可。它是方法说明，不包含 FluxManga 或第三方 shader 源码。
- AI_Skills 没有为整个仓库统一声明 MIT，不能重新许可其全部内容。按用户仓库 README 的技能复制说明导入并保留来源；其中 Elaya 衍生内容保留 [MIT 许可](licenses/Elaya-MIT.txt) 和 [上游来源说明快照](licenses/AI_Skills-NOTICES.txt)。快照里的路径属于上游仓库，可在上述固定提交核对。
- 后续更新先比较已固定的上游版本与本地适配，不能直接覆盖入口。发布或 push 时遵守根 AGENTS 的中央技能同步规则，本轮未获授权进行 push。

## 2026-11-04 自适应液态玻璃

- 新增 [adaptive-liquid-glass](adaptive-liquid-glass/SKILL.md)：按用户指定的[枯白 010 液态玻璃帖子](https://x.com/Kubai087/status/2117782193108082923)与演示提炼；自编平台适配方法和参数摘要，不复制完整 Prompt 或附加未经确认的第三方许可。
- 适用于卡片、播放器和桌面组件材质；区别 Web / Compose 真背景采样与 RemoteViews 透明材质降级，不替代既有底栏技能。中央技能副本在下次获授权的项目上传流程中同步，本次仅维护本地。

- 2026-11-04 AN010.R2：按用户确认的 B 方案补入静态壁纸映射、独立无色玻璃、每组件校准/预览与真实像素折射路径；保留无图回退，纠正将透明描边视为参考效果已完成的验收方式。
