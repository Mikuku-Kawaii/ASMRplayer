---
name: ui-design-system
description: 统一 ASMR Player 组件、Token、按钮层级、字阶和密度。
---

# ui-design-system

先核对项目 M3 技能、src/styles.css 与 src/components，不引入其他项目的颜色、路径或组件 API。
同一角色使用同一 Token；正文、次要文字、装饰边线和禁用态分别定义，不用一个 muted 值兼任全部。保留封面主体与用户主题，不强制移除紫色或改字体。
同一任务组只突出主要动作，次要操作仍可发现；删除、移出、取消与重试各有准确文案。导航选中、按钮按下和异步忙碌保持不同语义。
优先修改已有共享原语与 CSS 角色，再迁移实际调用点；无需为一次修改建立组件框架。检查 default/hover/focus/pressed/disabled，以及真实支持的 pending/error/selected 状态。

## 项目适配边界

这是 2026-11-01 从 AI_Skills 提炼的 ASMR Player 适配版，不是上游原文。先遵守根 AGENTS.md 和 [M3 规范](../asmr-player-m3-ui-standards/SKILL.md)。源码与用户范围优先；不自动重启、发布或委派。按需加载同目录的相关技能，不全量加载。来源版本与取舍见 [来源清单](../SOURCES.md)。
