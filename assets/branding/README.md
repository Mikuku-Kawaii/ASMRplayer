# 应用图标

桌面与 Android 的启动图标使用用户提供的猫耳耳机全图，矢量源为 `app-icon.svg`。桌面侧栏左上角及 Android 底栏圆形开关使用另一张透明、无 Z 的原图，源为 `public/brand-mark.svg`。界面图的取景框贴合角色轮廓，PC 图案的方块主体与右侧两行文字等高，手机底栏直接显示透明图案。

在 Windows 安装项目依赖后运行 `powershell -NoProfile -File scripts/generate-icons.ps1`。脚本使用项目自带的 Tauri CLI 栅格化两张 SVG，生成桌面 PNG / 多尺寸 ICO、Android 各密度启动图标、自适应图层和界面位图。桌面 favicon、Android 媒体默认封面保留完整插画；自适应图标的最终轮廓由启动器决定。
