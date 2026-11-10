---
name: impeccable
description: 审查或优化本项目界面的层级、布局、文字、反馈与可访问性；不用于后端任务。
---

# Impeccable — ASMR Player 适配入口

基于上游技能的本地适配（2026-11-01），参考资料与脚本完整保留，入口重写以服从项目范围。先读根 AGENTS.md 和 [M3 规范](../asmr-player-m3-ui-standards/SKILL.md)。本项目播放器/设置采用 Operate 模式，字幕阅读采用 Read；宣传页才使用 Persuade。

## 采用的方法

- 应用界面先读 [Operate](reference/operate.md)，借鉴稳定网格、克制层级和熟悉的控件。它的骨架、字体、尺寸等偏好必须依据当前场景，不是覆盖 M3 的强制命令。
- 审查按 [critique](reference/critique.md) 的可用性维度和 [audit](reference/audit.md) 的技术检查项提取问题；这些文件是参考资料，不是本项目执行流程。按“证据→影响→优先级→方案→验收”汇报。
- 布局、文案、响应式、精修分别按需读取 [layout](reference/layout.md)、[clarify](reference/clarify.md)、[adapt](reference/adapt.md)、[polish](reference/polish.md)。实际编辑前核对现有视觉真相，不默认重新设计整个应用。

## 项目执行方式

直接读取已有代码、项目规范和授权截图，不要求先生成 PRODUCT.md/DESIGN.md。不自动执行下载二进制的 context、注入 overlay、开启 CDP、安装 hooks、启动 live-server 或子代理。若用户另行明确要求这些工具，先检查实现及运行权限；工具不可用不阻塞源码/截图审查。用户要求不重启时不为截图重启原生程序。

保留主题紫、系统中日文字体、M3 组件、封面取色和后台播放状态。不把上游的风格反例当作一律删除列表。不用检测器数量或审美评分替代真实任务完成率，也不宣称未运行的检测通过。

来源、版本、许可和适配范围见 [来源清单](../SOURCES.md)。
