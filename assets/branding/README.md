# 应用图标

Windows 应用图标、favicon 和 Android 启动图标共用 `launcher-icon.svg` 中的粉紫底兜帽角色。PC 左上角和 Android 空音频库欢迎页共用 `public/desktop-brand-mark.svg` 中的透明底角色。Android 默认媒体封面使用 `app-icon.svg` 中的雾紫猫耳耳机。

在 Windows 安装项目依赖后运行 `powershell -NoProfile -File scripts/generate-icons.ps1`。仅生成桌面资源时添加 `-DesktopOnly`。脚本使用项目自带的 Tauri CLI 栅格化 SVG，生成桌面 PNG / 多尺寸 ICO 和 Android 各密度圆角矩形启动图标及品牌位图。
