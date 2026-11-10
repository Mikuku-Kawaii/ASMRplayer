---
name: ui-settings-design
description: 设计或审查 ASMR Player 设置分类、搜索、字段和保存反馈；不用于纯后端配置。
---

# ui-settings-design

先读 SettingsView、RecognitionSettings、SubtitlePreferencesSettings、SettingsDisclosure 及实际保存接口。区分即时保存、草稿保存与下次任务生效；不能为了换控件改变提交时机。
分类下按任务分组，基础设置默认可见，模型管理、词库和高级参数渐进展开。折叠标题可显示当前值摘要；搜索应能定位字段、展开所属分组并聚焦，而不只显示整个分类。
设置行采用可收缩的说明域与控件域；空间不足整行上下重排。按钮按内容宽度，不因控件列宽而拉成巨型按钮。短说明直接可读，长原理复用 HelpHint；重要风险与校验不只放悬停。
保存型区域明确未保存、保存中、失败和已保存；切分类和折叠保留草稿，失败不清空输入。删除模型与恢复默认说明范围；不为普通读取新增确认。
复用 Select、SettingLabel、SettingsDisclosure 和现有按钮样式，给真实需要的区域增加 children/actions/footer，空槽不占位。

## 项目适配边界

这是 2026-11-01 从 AI_Skills 提炼的 ASMR Player 适配版，不是上游原文。先遵守根 AGENTS.md 和 [M3 规范](../asmr-player-m3-ui-standards/SKILL.md)。源码与用户范围优先；不自动重启、发布或委派。按需加载同目录的相关技能，不全量加载。来源版本与取舍见 [来源清单](../SOURCES.md)。
