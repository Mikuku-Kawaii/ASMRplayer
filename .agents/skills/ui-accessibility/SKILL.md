---
name: ui-accessibility
description: 检查控件、菜单、弹窗的键盘、焦点、名称及对比；不用于后台数据服务。
---

# ui-accessibility

复用 Select、ContextMenu、ContentDialog、HelpHint；不假定本项目安装 Radix。检查 Enter/Space、方向键、逐层 Escape、关闭后回焦及模态边界。
图标按钮具有中文名称和提示；可访问名称包含可见文字。状态不只靠颜色。长标题有键盘可达的完整阅读入口；遵守本项目文字不可拖选规则，必要复制用显式复制按钮。
输入、textarea、contenteditable 和 IME 组合期间不触发全局播放快捷键。悬停操作也须在聚焦时可用。
正文与辅助正文按实际前景/背景检查对比，边框色不能直接当正文色。分别验证玻璃背景、forced-colors、文字放大及焦点环；源码色值计算不等同原生窗口实测。

## 项目适配边界

这是 2026-11-01 从 AI_Skills 提炼的 ASMR Player 适配版，不是上游原文。先遵守根 AGENTS.md 和 [M3 规范](../asmr-player-m3-ui-standards/SKILL.md)。源码与用户范围优先；不自动重启、发布或委派。按需加载同目录的相关技能，不全量加载。来源版本与取舍见 [来源清单](../SOURCES.md)。
