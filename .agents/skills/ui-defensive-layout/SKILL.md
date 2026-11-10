---
name: ui-defensive-layout
description: 检查长标题、长路径、双语字幕、动态计数与窄容器布局。
---

# ui-defensive-layout

使用长中文/日文、无空格路径、缺图、空值、未知总量及大数字检查目标容器。leading 与操作槽不收缩，正文列 min-width:0；换行优先于缩小字号硬塞。
标题可以省略但有完整阅读入口；正文、错误和字幕不可把唯一内容裁掉。图片遵守原 cover/contain 契约；缺图保持结构稳定。
时间与计数采用 tabular-nums，同时预留实际位数预算。状态变化不能推走播放、取消、关闭或恢复操作。
核对 Portal 位置、视口边缘、内部滚动及焦点环，不以 body overflow:hidden 掩盖布局错误。桌面窗口最小尺寸取 tauri.conf.json，浏览器缩放与 Windows DPI 分别记录。

## 项目适配边界

这是 2026-11-01 从 AI_Skills 提炼的 ASMR Player 适配版，不是上游原文。先遵守根 AGENTS.md 和 [M3 规范](../asmr-player-m3-ui-standards/SKILL.md)。源码与用户范围优先；不自动重启、发布或委派。按需加载同目录的相关技能，不全量加载。来源版本与取舍见 [来源清单](../SOURCES.md)。
