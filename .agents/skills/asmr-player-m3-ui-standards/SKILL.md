---
name: asmr-player-m3-ui-standards
description: "Google Material Design 3 (M3) frontend UI design system and UI copywriting/button text standards for ASMR Player. MUST USE whenever creating, styling, refactoring, or reviewing React components (src/features/*, src/components/*, src/app/*), CSS tokens (src/styles.css), button hierarchy, interactive states, layout surfaces, microcopy, button labels, badges, empty/error states, or accessibility."
---

# ASMR Player · Google M3 Frontend UI & Copywriting Standards

## When To Load This Skill

Load this skill whenever working on any frontend UI, styling, or user-facing text in ASMR Player, including:

- Creating or refactoring React views and components (`src/app/*`, `src/features/*`, `src/components/*`).
- Modifying CSS custom properties, color tokens, elevation, typography, spacing, or component styles in `src/styles.css`.
- Designing or updating buttons, icon buttons, chips, segmented controls, cards (`WorkCard`), tables, modals, side sheets (`RightDrawer`), or the bottom player bar (`PlayerBar`).
- Writing, reviewing, or localizing any user-facing UI text: button labels, tooltips (`title`), `aria-label`, status badges, form labels/placeholders, dialog actions, empty states, or error banners (`ErrorBanner`).
- Auditing keyboard navigation, focus rings (`:focus-visible`), tap/click target sizes, or contrast ratios.

---

## Core Design Philosophy (M3 Adapted for ASMR Desktop Listening)

1. **Tonal Surface Hierarchy Over Heavy Shadows (色阶容器层级取代生硬阴影)**
   - 采用 Google Material Design 3 (M3) 暗色主题的 **Surface Container 色阶递进体系**（从底层画布 `Surface` 到最高层浮层 `Surface Container Highest`）表达空间层级，辅以极克制的 `1px` `Outline Variant` 弱边框，避免在暗色界面使用厚重的黑色投影。
   - **夜间/助眠友好（Low-Glare Dark Mode）**：针对长时间与睡前 ASMR 收听场景，严禁使用刺眼的纯黑 `#000000`（易引发散光眩光）或高饱和荧光色块；主色调采用柔和的薰衣草紫（Tonal Primary `#a78bfa` / `#c4b5fd`）。

2. **Strict Action Hierarchy (一屏一主操作，五级按钮层次分明)**
   - 每个独立视图区域、卡片或模态框内，最多只允许出现 **1 个最高强调级按钮（M3 Filled Button）**，其余操作按重要性降级为 `Filled Tonal`、`Outlined`、`Text (Ghost)` 或 `Icon Button`。

3. **Explicit M3 State Layers & Keyboard Accessibility (标准状态层与全键盘可访问性)**
   - 所有可交互控件必须具备明确的 **Hover (`8%`)**、**Focus-Visible (`10%` + `2px` 焦点环)**、**Pressed (`12%`)** 与 **Disabled (`38%` 前景 / `12%` 容器)** 状态反馈。
   - 所有无文字图标按钮必须同时具备中文 `aria-label` 与 `title`；全局快捷键绝不能劫持输入框焦点。

4. **Action-Oriented, Unambiguous Microcopy (动词精准、术语唯一、安全感明确)**
   - 按钮文字必须采用**“具体动词 + 宾语”**，严禁使用含糊的 `确定 / 是 / 否 / 提交 / 操作`。
   - 严格区分**“非破坏性移除（移出队列/移除引用）”**与**“物理删除文件（删除离线文件）”**，时刻给用户明确的本地文件安全感。

---

## Part 1: Google Material Design 3 (M3) UI System Standards

### 1.1 M3 Dark Theme Color Tokens (`src/styles.css`)

在 `src/styles.css` 中维护或扩展样式时，必须遵循以下 M3 暗色角色令牌与项目变量的映射关系，严禁在组件内硬编码孤立的 Hex 颜色：

| M3 Color Role | 推荐 M3 Token / 项目 CSS 变量 | 色值参考 | 适用场景与层级规范 |
| --- | --- | --- | --- |
| **Surface (Dim / Root)** | `--md-sys-color-surface` / `--bg-root` | `#0f1117` | 应用底层主画布背景、主内容滚动区底色 |
| **Surface Container Low** | `--md-sys-color-surface-container-low` / `--bg-surface` | `#161923` | 左侧导航栏 (`left-sidebar`)、作品卡片 (`work-card`)、设置分组卡片 |
| **Surface Container** | `--md-sys-color-surface-container` | `#1a1e2b` | 底部常驻播放栏 (`player-bar`)、右侧抽屉 (`right-drawer`)、表头 |
| **Surface Container High** | `--md-sys-color-surface-container-high` / `--bg-elevated` | `#1e2230` | 模态对话框 (`modal-card`)、输入框底色、卡片 Hover 态、下拉菜单 |
| **Surface Container Highest** | `--md-sys-color-surface-container-highest` / `--bg-hover` | `#262b3d` | 按钮/列表项 Hover 状态层、未激活 Chip 背景、进度条轨道底色 |
| **Primary** | `--md-sys-color-primary` / `--accent` | `#a78bfa` | 核心强调文字、激活图标、焦点环、进度条填充、选中态边框 |
| **Primary Strong (Filled)** | `--md-sys-color-primary-fixed-dim` / `--accent-strong` | `#8b5cf6` | Filled 主按钮背景填充（搭配 `#ffffff` 文字确保 $\ge 4.5:1$ 对比度） |
| **Primary Container** | `--md-sys-color-primary-container` / `--accent-bg` | `rgba(139, 92, 246, 0.16)` | 导航栏激活胶囊、Tonal 按钮（继续收听）、激活 Chip、RJ 徽章底色 |
| **On Surface** | `--md-sys-color-on-surface` / `--text-primary` | `#e2e8f0` | 一级正文、作品标题、音轨名、主要控件图标 |
| **On Surface Variant** | `--md-sys-color-on-surface-variant` / `--text-secondary` | `#94a3b8` | 二级元数据（社团、声优、时长、文件大小）、未激活导航项、次要按钮文字 |
| **Outline** | `--md-sys-color-outline` / `--text-muted` | `#64748b` | 辅助说明文字、占位符、禁用态提示、输入框聚焦前外框 |
| **Outline Variant** | `--md-sys-color-outline-variant` / `--border-subtle` | `#2a3042` | 区域分割线 (`1px`)、卡片/表格静默外边框 |
| **Semantic: Success** | `--green` / `--green-bg` | `#34d399` / `rgba(52, 211, 153, 0.14)` | `已下载离线`、`已完成`、连接验证通过 |
| **Semantic: Info** | `--blue` / `--blue-bg` | `#60a5fa` / `rgba(96, 165, 250, 0.14)` | `本地原件`、本地目录标识 |
| **Semantic: Warning** | `--amber` / `--amber-bg` | `#fbbf24` / `rgba(251, 191, 36, 0.15)` | `已暂停`、`来源缺失`、`元数据待刷新`、睡眠定时激活提醒 |
| **Semantic: Error** | `--danger` / `--danger-bg` | `#f87171` / `rgba(248, 113, 113, 0.15)` | 错误横幅 (`ErrorBanner`)、失败状态、破坏性删除按钮 |

### 1.2 M3 State Layer Opacity Standard (交互状态层不透明度)

M3 规定交互状态不通过随意更换色相实现，而是通过在容器上叠加状态层（State Layer）实现：

- **Hover（悬停）**：叠加 `8%` `On-Surface` 或切换到高一级的 `Surface Container`（如 `--bg-surface` → `--bg-elevated`）。
- **Focus-Visible（键盘聚焦）**：必须显示 `2px solid var(--accent)` 外轮廓 + `outline-offset: 2px`，绝不能写 `outline: none` 抹除键盘焦点。
- **Pressed / Active（按下）**：叠加 `12%` 状态层或微缩过渡（`transform: scale(0.98)` 仅限独立小按钮，列表/大卡片禁用缩放）。
- **Disabled（禁用）**：容器或整体 `opacity: 0.42`，`cursor: not-allowed`，禁止触发 hover 高亮。

### 1.3 M3 Typography Scale & Tabular Numbers (字体阶梯与等宽数字铁律)

- **字体栈**：`"Segoe UI Variable Text", "Segoe UI", "PingFang SC", "Hiragino Sans", "Meiryo", system-ui, sans-serif`（兼顾中文、日文声优/作品名与拉丁字符渲染）。
- **M3 字号与行高规范**：

| M3 Type Scale | 字号 (`font-size`) | 行高 (`line-height`) | 字重 (`font-weight`) | 典型应用组件 |
| --- | --- | --- | --- | --- |
| **Headline Small** | `20px` | `28px` (`1.4`) | `700` | 作品详情页主标题 (`detail-hero-card h2`) |
| **Title Medium** | `15px–16px` | `22px` (`1.4`) | `600–700` | 侧边栏品牌标题、作品卡片标题 (`.work-card-title`)、模态框标题、抽屉标题 |
| **Title Small** | `14px` | `20px` (`1.45`) | `600` | 设置分组标题、播放栏当前音轨名 (`.player-track-title`)、导航栏激活项 |
| **Body Medium** | `13px–14px` | `20px` (`1.5`) | `400` | 默认界面正文、表格音轨行、作品简介、下拉选项 |
| **Label Large** | `13px` | `18px` | `500–600` | 标准按钮文字 (`.btn`)、Tab 标签 |
| **Label Medium** | `12px` | `16px` | `500` | 小号按钮 (`.btn-sm`)、继续收听胶囊 (`.btn-resume-pill`)、作品元数据行 |
| **Label Small** | `11px` | `14px` | `600` | 状态徽章 (`.badge`)、来源胶囊 (`.source-pill`)、标签胶囊 (`.tag-pill`)、时间戳 |

- **等宽数字铁律 (`font-variant-numeric: tabular-nums`)**：
  - **Rule**：凡是展示**播放进度时间 (`00:00 / 01:23:45`)、睡眠定时倒计时 (`29:59`)、音轨时长、A–B 循环起止点、下载字节数/速率 (`12.4 MB / 48.0 MB`)、百分比**的元素，必须声明 `font-variant-numeric: tabular-nums`，彻底杜绝数字跳动引起的布局抖动。

### 1.4 M3 Shape Scale (圆角规范)

遵循 M3 Shape Token 阶梯，保持圆角语义统一：

- **Extra Small (`4px–5px`)**：内联技术标识、紧凑状态徽章 (`.badge`)。
- **Small (`6px–8px`)**：文本输入框 (`.input-text`)、紧凑动作按钮 (`.btn`, `.btn-sm`)。表单下拉选择触发器为 `9px`，工具栏胶囊变体为 `9999px`，浮层为 `12px`，详见“统一下拉选择控件规范”。
- **Medium (`10px–12px`)**：作品卡片 (`.work-card`)、设置卡片 (`.settings-card`)、音轨数据表外框 (`.tracks-table`)。
- **Large (`14px–16px`)**：作品详情头图卡片 (`.detail-hero-card`)、模态弹窗卡片 (`.modal-card`)。
- **Full / Pill (`9999px`)**：播放栏主控圆按钮 (`.btn-play-main`)、筛选/功能切换胶囊 (`.btn-chip`)、作品标签 (`.tag-pill`)。

### 1.4a 统一下拉选择控件规范

**唯一实现**：`src/components/Select.tsx`，样式集中在 `src/styles.css` 的 `.app-select*`。所有新增和改造的单选下拉使用此组件，禁止重新引入 `<select>` / `<option>` 系统原生弹出界面，也不允许逐页复制浮层样式。标签批量筛选复用 `src/components/TagFilterPopover.tsx`，由 `OnlineFilters` / `LibraryFilters` 提供数据；保留最多五项、草稿确认提交的多选按钮语义，不能替换成单选。音频库入口位于搜索框左侧。

**受控 API**：`value: string`、`options: readonly { value: string; label: string; disabled?: boolean }[]`、`onValueChange(value)` 为必需参数；可选 `id`、`disabled`、`size="regular" | "compact"`（默认 regular）、`className`（仅布局）、`placeholder`（未匹配值时显示）及 ARIA 属性。选项 value 必须唯一。只有确认与当前值不同的可用选项才调用回调；方向键和字符定位仅改变导航高亮。保存时机、失败回退、加载禁用由业务层保持原有语义。

```tsx
import { Select } from "../../components/Select";

<label htmlFor="sort-direction">排序方向</label>
<Select id="sort-direction" size="compact" variant="chip" value={sort}
  options={[{ value: "desc", label: "降序" }, { value: "asc", label: "升序" }]}
  onValueChange={setSort} />

// 动作型选择不保存选中值：执行后仍显示占位文案，允许重复选择同一动作。
<Select aria-label="镜像预设" value="" size="compact"
  options={[{ value: "", label: "镜像预设..." }, ...mirrorOptions]}
  onValueChange={value => { if (value) void applyMirror(value); }} />
```

**尺寸与状态**：常规触发器最小高 `36px`、`14px` 字号，紧凑触发器最小高 `32px`、`13px` 字号；均为 `400` 字重、`9px` 圆角、`--bg-elevated` 色阶、`1px --border-subtle` 描边、lucide `ChevronDown`。浮层与触发器间隔 `6px`，`12px` 圆角、`6px` 内边距、弱描边与由 `--bg-root` 混合的轻阴影。菜单项最小高 `36px`（长文案自动增高），`14px / 20px`、`400` 字重、`8px 10px` 内边距与 `7px` 圆角。悬停用 `--bg-hover`；已选用 `--accent-bg`、`--accent` 和右侧 `Check`，不使用复选框或系统灰色选中条。键盘导航另加内侧焦点环，不改变 `aria-selected`；鼠标导航不显示该环。触发器仅 `:focus-visible` 显示焦点环，禁用透明度 `.38`。

**浮层与内容**：React Portal 挂在最近的 `[role="dialog"]` / `dialog` / `[aria-modal="true"]`，无弹窗则挂在 body；通过手动 Popover API 进入浏览器顶层，避开 overflow、transform、z-index 与原生 dialog 层级限制，保留弹窗 DOM 包含关系。固定定位基于触发器 `getBoundingClientRect()`，监听祖先滚动、ResizeObserver、窗口和 visualViewport 的缩放/尺寸/滚动；下方不足时自动向上展开，水平方向约束在视口 `8px` 安全边距。菜单宽度至少 `200px` 或触发器宽度，最大不超过视口；最大高 `300px` 且受可用空间限制，内部滚动并阻断滚动穿透。导航只滚动菜单，不滚动页面。触发器单行省略、title 提供完整文本；菜单内完整展示并合理换行，包括无空格长路径。

**工具栏变体**：可选 API `variant="field" | "chip"`，默认 `field`，遵循上面的表单尺寸。与筛选等胶囊按钮并排时使用 `size="compact" variant="chip"`，直接复用 `.btn-chip`：高 `30px`、`9999px` 圆角、字号 `12px`、字重 `500`、内边距 `4px 10px`，颜色和悬停态与相邻按钮一致；浮层、选项和键盘行为仍使用同一实现。禁止页面单独覆盖触发器圆角与字号。

**动效**：展开和关闭均为 `180ms` 淡入/淡出和 `4px` 轻微位移，向上展开时反转位移方向。关闭阶段立刻撤销交互和无障碍暴露，动画结束卸载。`prefers-reduced-motion: reduce` 关闭位移动画并立即卸载，不拖延交互。

**键盘与无障碍**：触发器是可关联 label 的 button，使用 `role="combobox"`、`aria-expanded`、`aria-controls`、`aria-activedescendant` 和 `aria-disabled`；浮层为 listbox，选项为 option，正确声明选中与禁用。优先 `label htmlFor` + `id`，无可见标签时提供准确的中文 `aria-label`。焦点保留在触发器：方向键导航（跳过禁用项）、Home/End 首尾、输入字符前缀定位（700ms 缓冲，重复字符轮换）；Enter/Space 展开或确认，Escape 关闭并保留焦点，Tab 正常离开且关闭。鼠标确认后回到触发器，点击或聚焦外部关闭且不抢焦点。快捷键必须隔离全局播放/快进；弹窗的捕获阶段 Escape 处理必须先让已展开 Select 消费，Tab 仍由弹窗原有焦点管理处理。相关实现见 `UpdateDialog.tsx`。

**回归**：公共交互测试在 `src/components/Select.test.tsx`。替换业务控件时验证原值、默认值、禁用、保存/错误逻辑与动作型占位复位；实际检查短菜单、长文案、长列表、边缘、缩放、滚动和弹窗，不能仅根据 jsdom 宣称视觉验收。

### 1.5 M3 Five-Tier Button System (五级按钮规范)

在任何界面设计或重构中，必须按以下 5 级语义选用按钮类型：

| M3 按钮层级 | 项目类名 | 视觉特征 | 使用场景与约束 |
| --- | --- | --- | --- |
| **1. Filled Button（实心主按钮）** | `.btn.btn-primary` | `--accent-strong` 实心背景 + `#fff` 文字 | **最高强调**。用于当前视图/弹窗最核心的唯一正向召唤动作（如：`导入作品文件夹`、`确认导入`、`开始下载`、`播放当前目录`）。每个局部区域最多 1 个。 |
| **2. Filled Tonal Button（色调次主按钮）** | `.btn-resume-pill` / `.btn-chip.active` | `--accent-bg` 柔和底色 + `--accent` 文字 | **中高强调**。用于高频但非唯一的主入口，或表示已激活的持久状态（如卡片上的 `继续收听 · 第 2 轨 · 14:20`、播放栏已开启的 `单曲循环` / `睡眠定时`）。 |
| **3. Outlined / Elevated Button（描边/抬升次按钮）** | `.btn.btn-secondary` / `.source-pill` | `--bg-elevated` 底色 + `1px solid --border-subtle` | **中等强调**。用于常规并列操作（如：`导入作品库根目录`、`刷新元数据`、`编辑信息`、`关联与撤销`、`加入队列`）。 |
| **4. Text Button（文本/幽灵按钮）** | `.btn.btn-ghost` | 透明背景，Hover 时浮现 `--bg-hover` | **低强调**。用于返回导航（`返回列表`）、弹窗取消（`取消`）、表单重置（`恢复原值`）、表格内次要操作。 |
| **5. Icon Button（纯图标按钮）** | `.btn-icon` / `.btn-fav` | 无边框圆形/圆角矩形，仅含 `lucide-react` 图标 | **紧凑工具操作**。用于播放控制栏（上一首/下一首）、收藏红心、关闭弹窗/抽屉。最小点击热区不得小于 `32x32px`（主控区推荐 `36x36px–42x42px`）。 |

### 1.6 Core Layout & Component Guardrails (核心布局与组件守则)

- **左侧导航栏 (`left-sidebar`, M3 Navigation Drawer)**：
  - 固定宽度 `210px`，包含顶部品牌标识与垂直导航列表。
  - 导航项使用左对齐 `图标 (17px–18px) + 4字中文标签`，激活态使用 Tonal 胶囊高亮（`background: var(--accent-bg); color: var(--accent); font-weight: 600`）。
- **底部播放栏 (`player-bar`, Persistent Bottom Player)**：
  - 必须常驻于应用壳底部，切页、打开详情或打开弹窗时绝不卸载。
  - 严格三栏结构：**左栏**（当前音轨名、所属作品跳转链接、当前来源徽章与同版本来源切换器）、**中栏**（传输控制按钮行 + 带时间轴的拖动进度条）、**右栏**（A–B 循环、睡眠定时、音量控制、队列/书签/字幕抽屉开关）。
  - **防挤压铁律**：左栏音轨名与作品名必须设置 `overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0`，无论日文/中文标题多长，绝不能挤压中间的播放/暂停按钮或进度条宽度。
- **右侧抽屉 (`right-drawer`, M3 Standard Side Sheet)**：
  - 固定宽度 `340px`，与主内容区并排推开而非全屏遮罩，方便边看作品详情边管理播放队列、书签或实时字幕。
- **紧凑小矩形迷你模式 (`mini-player-shell`, NetEase Cloud Style Floating Mini-Player)**：
  - **尺寸与容器**：收起态推荐固定紧凑矩形 `340px × 88px`（展开内嵌字幕/精简队列态 `340px × 210px`），背景采用 `--md-sys-color-surface-container-high` (`#1e2230`) 搭配 `1px solid var(--border-subtle)` 与圆角 `12px`。
  - **拖拽与窗口控制**：空白背景区声明 `data-tauri-drag-region` 支持任意拖动位置，所有交互按钮显式声明 `app-region: no-drag`。
  - **常态 vs 悬停态（Hover Progressive Disclosure）**：
    - **左侧**：`56px × 56px` 圆角正方形封面（悬停浮现半透明遮罩与 `展开完整窗口` 图标）。
    - **右侧常态**：第 1 行展示当前音轨标题（单行省略），第 2 行高亮展示**实时同步单行字幕**（无字幕时回退展示所属作品名与 `tabular-nums` 播放时间 `12:34 / 45:00`）。
    - **右侧悬停（Hover）态**：平滑过渡浮现紧凑图标按钮组：`上一首`、`播放/暂停`、`下一首`、`切换字幕显示`、`置顶窗口 (Pin)`、`展开完整窗口`。
    - **底边进度条**：贴底 `3px` 连续进度条，鼠标悬停在窗口上时平滑加粗至 `6px` 供快速点按/拖拽跳转。
- **下载二级菜单紧凑居中与背景滚动锁定 (`DownloadTaskModal` Compact Viewport Centering & Scroll Lock, T1114)**：
  - **小巧紧凑尺寸**：下载二级菜单宽度控制在 `min(440px, 86vw)`，总高控制在 `min(480px, 74vh)` 以内，精简内边距、格式胶囊与文件树行高。
  - **严格居中于整个软件窗口**：使用全局遮罩（`position: fixed; inset: 0; display: flex; align-items: center; justify-content: center`）脱离局部文档流，确保无论主界面处于什么滚动位置，弹窗始终严格居中于软件窗口正中央。
  - **主界面滚动条锁定（Scroll Lock）**：弹窗打开期间强制锁定主界面滚动容器（`.main-content { overflow: hidden; }`）并阻断滚轮穿透（`overscroll-behavior: contain`），使主界面滚动条不能滚动，仅允许弹窗内文件树独立滚动；关闭后无损恢复。
- **全屏于软件窗口的沉浸式播放页 + 底部播放栏沉浸态形态重构与联动动效 (`immersive-now-playing-view`, T1115)**：
  - **全屏覆盖整个软件窗口（Full-Window Immersive Screen）**：展开沉浸式播放页时，不再局限于右侧 `.main-content` 子区域，而是作为覆盖整个软件窗口（盖住左侧导航栏与主内容区，铺满窗口顶栏下方全窗口空间）的全屏级视图层。
  - **中央巨幕同步字幕舞台（Center Multi-Line Synchronized Subtitle Stage）**：
    - **深邃氛围背景**：取当前作品封面做大半径高斯模糊（`blur(72px)`）加极光缓慢漂移动画（`m3AuroraDrift`）与暗色护眼遮罩。
    - **居中双层/多行滚动字幕**：纵向居中排布多行台词，当前句高亮提亮放大，点击任意一行台词直接 `seek` 跳转至该句对应毫秒时间戳。
  - **底部播放栏（`PlayerBar`）沉浸态专属形态变化与过渡动效（Immersive Bottom PlayerBar Morphing & Motion）**：
    - 当点开全屏沉浸式播放页时，下方播放栏同步发生**专属形态变化与动效**：
      1. **半透明毛玻璃流光融合**：底栏从常规实心深色容器平滑过渡为融入全屏封面模糊背景的半透明悬浮晶莹控制舱；
      2. **大封面弹性上浮与左栏位移重组**：常规小缩略图收起，左下角圆角正方形大封面（`120px–136px`）通过弹簧缓动动画（`cubic-bezier(0.22, 1, 0.36, 1)`）平滑上浮展开，同时底栏曲目名、作品/声优信息与传输控制区平滑位移、尺度延展与排版重组；
      3. **进度条与主控动效升级**：进度/波形条平滑拉宽并带流光高光过渡，展开与收起全过程具备连贯的双向形变动画。
- **详情页返回历史与滚动位置保留 (`WorkDetailView` History & Scroll Preservation)**：
  - 从「在线浏览 (`OnlineView`)」（如切换为畅销榜/销量榜、翻到第 $x$ 页、带搜索词）或「我的音频库 / 喜欢 (`LibraryView`)」点进作品详情页 (`WorkDetailView`) 时，**底层列表视图必须保持挂载 (`display: none`) 而非卸载重建**。
  - 点击详情页左上角 `← 返回` 时，必须原样恢复上一页的筛选排序、分页页码、搜索词以及主滚动容器 (`main-content`) 的精确 `scrollTop` 位置。
- **详情页声优（CV）与标签（Tag）点击检索寻找功能 (`WorkDetailView` Clickable CV & Tag Search, T1116)**：
  - 详情页中的声优胶囊（`.cv-pill`）与标签胶囊（`.tag-pill`）均为可点击按钮（带悬停抬升、高亮描边与按压动效）。
  - 点击任意 `CV` 或 `Tag` 时，支持一键跳转并按该声优或标签寻找匹配作品（自动切入「在线浏览 (`OnlineView`)」或「我的音频库 (`LibraryView`)」并填入该关键词执行检索），且保留返回历史栈以便随时退回原详情页。
- **「喜欢」界面一键顺序播放与默认“有音效 + WAV”优选规范 (`Favorites` One-Click Play All, T1117)**：
  - 在「喜欢」界面（`LibraryView` 的 `filter === "favorites"`）顶部工具栏提供醒目的主操作按钮 `一键播放全部`（Filled Primary Button）。
  - 点击后按当前「喜欢」列表顺序依次提取各作品音轨排列进播放队列并立即开始播放。
  - **默认版本与音源优选策略**：面对多目录分组（如 `WAV` / `MP3`、`SEあり` / `SEなし`）或多格式音源时，默认优先选择**有音效（排除 `SEなし` / `无音效` / `NoSE` / `無し` 目录）且格式为 `WAV`** 的音轨版本（无 `WAV` 时按 `WAV > FLAC > MP3/M4A/OGG/OPUS` 及 `本地导入 > 已下载 > 在线流式` 顺序回退），严禁将同一作品的多个重复格式版本同时混入队列。
- **底部播放栏「控制与音效」二级菜单内置 WASAPI 独占模式按钮 (`PlayerBar` Control & DSP Popover WASAPI Toggle, T1118)**：
  - 将 `WASAPI 独占输出模式 (Exclusive Mode)` 切换按钮直接放置在底部播放栏（`PlayerBar`）右侧弹出的 **「控制与音效」二级菜单（`.player-right-popover`）** 中，与硬件加速、左右声道互换（Swap L/R）、防炸耳限制器、ASMR 三段滤波及 10 段 PEQ 集中排布，方便用户在不离开当前页面的情况下一键热切换硬件独占直通模式。
- **双维度「收藏（进我的音频库）」与「喜欢（进喜欢）」分离铁律 (Collected vs. Favorite Separation)**：
  - **收藏 (`Bookmark` 图标，`isCollected` / `toggle_collected`)**：控制作品是否收入第一页 **「我的音频库」** (`filter === "all"`)。本地扫描导入的作品自动标记为已收藏；在线浏览点开查看详情的作品不会自动混入「我的音频库」，只有用户显式点击 **「收藏」** 后才进入「我的音频库」。
  - **喜欢 (`Heart` 图标，`isFavorite` / `toggle_favorite`)**：控制作品是否进入 **「喜欢」** 页面 (`filter === "favorites"`)。
  - **规范化详情页 (`WorkDetailView`) 主操作栏**：主操作区统一并列提供 `播放` / `继续收听`、`收藏 / 已收藏` (`Bookmark`)、`喜欢 / 已喜欢` (`Heart`)、`下载` 四个核心按钮；次要工具（`同步元数据`、`抓取字幕`、`编辑元数据`、`关联与撤销`）收纳于详情页顶栏右侧。
- **零冗余提示铁律 (Zero Noisy Prompts / Hints Rule)**：
  - 严禁在主页或在线页作品卡片底部放置 `左键打开详情 · 右键创建下载任务 →`、`正在拉取目录树并入库…` 等操作提示文字。
  - 严禁在列表加载或详情切换时渲染冗余的“正在加载音频/作品…”占位提示框或顶部绿色/蓝色通知横幅 (`info-banner`)。
  - 严禁在详情页目录栏与徽章区堆砌诸如 `目录分组与播放范围（避免 MP3 / WAV 自动混播）：`、`在线 ID: ... · 缓存已过期` 等解释性长句。

---

## Part 2: Frontend Button & UI Copywriting Standards (前端按钮与显示文字标准)

### 2.1 Button Copy Golden Rules (按钮文字黄金六法则)

#### 法则 1：使用“精准动词”或“动词 + 宾语”，全面禁用含糊词
按钮必须直接告诉用户**点击后会发生什么业务结果**。在任何对话框、表单或工具栏中，**严禁单独使用 `确定`、`是`、`否`、`提交`、`操作`、`OK`、`执行`**。

| 场景 | ❌ 错误/含糊文案 | ✅ 标准 M3 文案 |
| --- | --- | --- |
| 本地目录导入预览弹窗确认 | `确定` / `提交` | `确认导入` 或 `导入选中作品 (3)` |
| 离线下载预览弹窗确认 | `确定` / `好` | `开始下载` 或 `加入下载队列 (5)` |
| 作品元数据编辑弹窗保存 | `提交` / `完成` | `保存修改` |
| 在线作品按 RJ/ID 打开 | `确定` / `查询` | `打开作品` |
| 关联本地与在线作品 | `是` / `合入` | `关联在线作品` |
| 确认共享同一时间轴版本 | `合并` / `确定` | `合并版本并共享进度` |
| 放弃弹窗当前操作 | `否` / `关闭窗口` | `取消`（纯查看类弹窗用 `关闭`） |

#### 法则 2：严格区分“移除引用/移出列表”与“物理删除文件”（安全边界铁律）
ASMR 用户的本地音频文件体量大、整理成本高。界面上的每一个移除/清理类按钮，必须从字面上消除“会不会删掉我硬盘里的原文件”的恐惧：

| 业务动作实际影响 | ❌ 严禁使用的惊悚/歧义词 | ✅ 必须使用的标准按钮文案 | 配套提示说明要求 |
| --- | --- | --- | --- |
| 从当前播放队列拿掉某条音轨 | `删除` / `删掉` | `移出队列` | — |
| 清空当前播放队列所有项 | `全部删除` | `清空队列` | — |
| 解除本地音频库根目录挂载 | `删除目录` / `删除库` | `移除目录引用` | 必须注明：`仅从软件索引移除，不会删除磁盘上的原始音频文件` |
| 放弃未完成的下载任务（仅删 `.part`） | `删除文件` | `取消下载` | Tooltip/说明：`取消任务并清理未完成的 .part 临时文件` |
| 清理播放缓冲与过期接口缓存 | `清空所有数据` / `清理离线` | `清理播放缓存` | 必须注明：`不会删除已下载的离线文件与本地导入作品` |
| 删除软件已下载完成的离线文件 | `移除`（过于轻描淡写） | `删除离线文件` | 必须使用危险色样式，明确只删除下载目录内的副本 |

#### 法则 3：异步按钮的“四态文案联动”（Idle → Loading → Active → Error）
凡是触发异步 Tauri 命令（扫描、网络请求、下载预览、连接测试）的按钮，在请求未返回前必须处于 `disabled` 状态，并同步更新按钮文字，使用标准中文省略号 `…`（`U+2026`）或半角 `...` 保持统一（推荐统一为 `…`）：

- **目录扫描**：`扫描目录` → `正在扫描…`（旁配可点击的 `取消扫描` 按钮）
- **导入预览**：`预览并导入` → `正在解析目录…`
- **刷新线上信息**：`刷新元数据` → `正在刷新…`
- **打开在线作品**：`打开作品` → `正在加载…`
- **播放引擎故障恢复**：当 `snapshot.state === "error"` 时，播放栏展示显著的 `重启播放引擎` 或 `重试播放` 按钮。

#### 法则 4：状态切换按钮（Cycle / Toggle Buttons）遵循“显示当前态，提示动作态”
对于在多个状态间循环切换的胶囊按钮（如循环模式、睡眠定时、A–B 循环、收藏）：
- **按钮表面文字（Visible Label）**必须反映**当前生效的状态**，让用户一眼看清当前模式。
- **悬停提示（`title` / `aria-label`）**说明**点击后将执行的切换动作**。

| 控件 | 当前状态 | 按钮表面显示文字 | `title` / `aria-label` 属性 |
| --- | --- | --- | --- |
| **循环模式** | `none` | `顺序播放`（或紧凑态 `顺序`） | `切换循环模式（当前：顺序播放）` |
| **循环模式** | `single` | `单曲循环` | `切换循环模式（当前：单曲循环）` |
| **循环模式** | `queue` | `队列循环` | `切换循环模式（当前：队列循环）` |
| **睡眠定时** | `off` | `睡眠定时` | `设置睡眠定时（30 分钟 / 本轨结束）` |
| **睡眠定时** | `minutes` | `定时 29:45` | `切换睡眠定时（点击改为本轨结束暂停）` |
| **睡眠定时** | `end_of_track` | `本轨结束暂停` | `切换睡眠定时（点击关闭睡眠定时）` |
| **A–B 循环** | 未设置 | `A–B` | `设置 A–B 循环起点 (A)` |
| **A–B 循环** | 已设 A 点 | `已设 A · 设 B 点` | `在当前位置设置循环终点 (B)` |
| **A–B 循环** | 循环生效中 | `A–B 循环中 (清除)` | `点击清除当前 A–B 循环区间` |
| **收藏按钮** | 未收藏 / 已收藏 | `收藏` / `已收藏` | `加入收藏` / `取消收藏` |

#### 法则 5：按钮字数、计数格式与标点禁令
- **字数控制**：常规操作按钮保持 **2～6 个汉字**，避免按钮内文字换行（`.btn { white-space: nowrap; }`）。Select 触发器展示选中项，长度不受操作按钮字数限制；遵循单行省略、完整 title 和浮层内换行规则。
- **动态计数格式**：在按钮上附带数量时，统一使用半角括号且括号前留 1 个半角空格，例如：`开始下载 (4)`、`切换来源 (2)`、`播放队列 (12)`。
- **标点禁令**：按钮文字末尾**严禁出现句号（`。`）、感叹号（`！`）或冒号（`：`）**。
- **连接符规范**：A–B 循环与时间区间统一使用 en-dash `–`（如 `A–B 循环`、`01:20 – 03:45`）；多级状态组合使用中圆点 ` · `（前后各一空格，如 `继续收听 · 第 2 轨 · 14:20`）。

#### 法则 6：纯图标按钮（Icon-Only Buttons）的无障碍文案强制要求
任何没有可见文字的 `.btn-icon` 必须同时提供准确的中文 `aria-label` 和 `title`。若该操作绑定了全局键盘快捷键，必须在 `title` 中以括号注明快捷键：
- 播放/暂停：`aria-label="播放"`，`title="播放 (Space)"` / `aria-label="暂停"`，`title="暂停 (Space)"`
- 上一首/下一首：`aria-label="上一首"`，`title="上一首"` / `aria-label="下一首"`，`title="下一首"`
- 快退/快进：`title="快退 5 秒 (←)"` / `title="快进 5 秒 (→)"`

---

### 2.2 Domain Terminology Dictionary (全域统一术语表)

全项目所有组件、弹窗、设置项和错误提示必须严格使用同一套领域词汇，禁止在同一概念上混用不同叫法：

| 核心概念 | 唯一标准中文术语 | 对应代码模型 / 字段 | ❌ 严禁混用的同义词 |
| --- | --- | --- | --- |
| Work | **作品** | `WorkSummary` / `WorkDetailDto` | 专辑、节目、音声包、同人音声、项目 |
| Library | **我的音频库** / **作品库** | `LibraryView` | 媒体库、本地曲库、音乐架 |
| Track | **音轨** | `TrackItemDto` | 歌曲、单曲、曲目、章节、音频文件 |
| Track Version | **版本** | `TrackVersionDto` (`versionId`) | 变体、分P、时间线版本 |
| Media Source | **来源** | `MediaSourceDto` (`sourceKind`) | 线路、节点、文件源、播放源 |
| `local_imported` | **本地原件** / **本地导入** | `sourceKind === "local_imported"` | 本地文件、硬盘源、外部源 |
| `local_downloaded` | **已下载离线** | `sourceKind === "local_downloaded"` | 已缓存、离线包、本地缓存 |
| `online_provider` | **在线流** | `sourceKind === "online_provider"` | 网络流、云端播放、在线试听 |
| Directory Group | **目录分组** | `DirectoryGroupOption` (`groupPath`) | 格式文件夹、子歌单、分类目录 |
| Playback Queue | **播放队列** | `QueueItemDto[]` (`queue`) | 播放列表、歌单、待播清单 |
| Resume Playback | **继续收听** | `lastVersionId` + `lastPositionMs` | 恢复播放、接着听、断点续播（按钮文案统一用“继续收听”） |
| Bookmark | **书签** | `BookmarkDto` | 标记点、时间锚点、打点 |
| A–B Loop | **A–B 循环** | `AbLoopDto` | 区间复读、AB段循环 |
| Sleep Timer | **睡眠定时** | `SleepTimerDto` | 定时关机、助眠倒计时、自动停止 |
| Subtitle | **字幕** | `AttachmentDto` (`subtitle`) | 歌词、LRC、台词轴 |
| Cloud/Local Subtitle Fetch | **抓取云端字幕** / **匹配本地字幕** | `AttachmentDto` | 搜索歌词、下载LRC |
| Mini-Player Mode | **迷你模式**（展开按钮用 **展开完整窗口**） | `isMiniMode` | 小窗模式、精简条、悬浮球 |
| Immersive Now-Playing Stage | **沉浸播放页**（按钮用 **展开播放页** / **收起播放页**） | `isImmersiveOpen` | 歌词大屏、全屏播放器、写真页 |
| Windows SMTC | **系统媒体控制 (SMTC)**（含 `完整展示` / `隐私脱敏模式` / `关闭`） | `smtcMode` | 锁屏歌词、桌面媒体条、耳机线控开关 |

---

### 2.3 Badges, Numbers, Empty States & Error Copy Standards

1. **中英文与数字混排间距**：
   - 普通说明文本与按钮中，汉字与拉丁字母、阿拉伯数字之间保留 **1 个半角空格**（如：`共 18 部作品`、`快进 5 秒`、`最大并发 2 个任务`）。
   - **例外**：作品编号（`RJ01234567`）、紧凑徽章内部文字（`MP3`、`FLAC`）内部不加空格。

2. **徽章（Badges）文案规范**：
   - **RJ 编号徽章 (`.badge-rj`)**：固定全大写 `RJ` 前缀（如 `RJ01000001`）。
   - **音频格式展示**：扩展名/编码格式统一转为全大写（`WAV`、`FLAC`、`MP3`、`M4A`、`OGG`、`OPUS`）。
   - **下载任务状态徽章**：
     - `queued` → `排队中`
     - `resolving` → `正在解析地址`
     - `downloading` → `下载中`
     - `verifying` → `正在校验完整性`
     - `paused` → `已暂停`
     - `completed` → `已完成`
     - `failed` → `下载失败`
     - `canceled` → `已取消`

3. **空状态（Empty States）三段式标准**：
   任何列表为空（作品库为空、无搜索结果、播放队列为空、无书签、无字幕、无下载任务）时，禁止只写冷冰冰的“暂无数据”四个字，必须包含：
   - **状态结论 + 简短引导说明**（说明为什么为空，以及如何开始）。
   - 示例：
     - 作品库为空：`音频库尚无作品 — 点击上方“导入作品文件夹”或“导入作品库根目录”即可扫描本地音频，软件不会改动你的原始文件。`
     - 队列为空：`播放队列为空 — 在作品详情中点击“播放”或“加入队列”即可添加音轨。`
     - 书签为空：`当前音轨版本尚无书签 — 播放时点击下方可记录喜欢的时间点，同一版本的在线流与离线文件共享书签。`

4. **错误提示（Error Banner & Inline Errors）可操作原则**：
   - 错误文案必须遵循 **`[明确故障现象] + [可选的恢复建议/按钮]`**。
   - 当 `AppError.hint` 存在时，必须在错误横幅或状态区一同展示。
   - 严禁直接将未格式化的 Rust 结构体调试输出、SQL 语句或带有签名 Token 的完整 URL 渲染到前端界面。

---

## Part 3: Frontend UI & Copy Verification Checklist

在完成任何前端组件或样式修改后，按以下清单逐项核验：

1. **静态与单元测试验证**：
   - 运行 `pnpm typecheck`（0 错误）。
   - 运行 `pnpm lint`（0 警告/错误）。
   - 运行 `pnpm test`（确保 `App.test.tsx` 等交互与快捷键隔离测试全部通过）。
2. **M3 视觉与布局自检**：
   - 是否使用了 `src/styles.css` 中的设计变量而非硬编码颜色？
   - 同一局部区域是否最多只有 1 个 `.btn-primary`（Filled Button）？
   - 所有时间、时长、倒计时、字节数是否具有 `font-variant-numeric: tabular-nums`？
   - 超长中文/日文音轨名是否能正常省略（`text-overflow: ellipsis`）而不撑破 `PlayerBar` 或卡片网格？
3. **按钮与文案自检**：
   - 是否消灭了所有孤立的 `确定 / 是 / 否 / 提交` 按钮，替换为具体动词？
   - 移除队列、移除库引用、清理缓存等非破坏性操作是否与“删除离线文件”在用词和颜色上严格区分？
   - 所有纯图标按钮（`.btn-icon`）是否都具备准确的中文 `aria-label` 与 `title`？
   - 全页面术语是否严格符合 **2.2 全域统一术语表**（作品 / 音轨 / 版本 / 来源 / 播放队列 / 继续收听）？
