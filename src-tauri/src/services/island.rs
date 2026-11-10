//! Desktop island geometry is owned by the host: physical monitor origins, logical UI sizes.
use crate::{
    db::Database,
    domain::{AppError, AppResult, ErrorCode},
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

const EXPANDED_WIDTH: f64 = 400.;
const EXPANDED_HEIGHT: f64 = 186.;
// Four 32px rows + three 4px gaps, 34px heading, 10px padding and 1px border per edge.
const MENU_HEIGHT: f64 = 196.;
const COMPACT_MIN_WIDTH: f64 = 200.;
const COMPACT_MAX_WIDTH: f64 = 640.;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum IslandSize {
    ExtraSmall,
    Small,
    #[default]
    Medium,
    Large,
    ExtraLarge,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IslandScales {
    pub compact_scale: f64,
    pub expanded_scale: f64,
}
impl Default for IslandScales {
    fn default() -> Self { Self { compact_scale: 1., expanded_scale: 1. } }
}
impl From<IslandSize> for IslandScales {
    fn from(size: IslandSize) -> Self {
        let ratio = match size {
            IslandSize::ExtraSmall => 0.7 / 0.8,
            IslandSize::Small => 1.,
            IslandSize::Medium => 0.9 / 0.8,
            IslandSize::Large => 1. / 0.8,
            IslandSize::ExtraLarge => 1.15 / 0.8,
        };
        Self { compact_scale: ratio, expanded_scale: ratio }
    }
}
impl IslandScales {
    fn for_form(self, form: IslandForm) -> f64 {
        if matches!(form, IslandForm::Compact | IslandForm::Toast) { self.compact_scale } else { self.expanded_scale }
    }
    fn with_changes(self, compact: Option<f64>, expanded: Option<f64>) -> AppResult<Self> {
        fn validate(value: f64) -> AppResult<f64> {
            if !value.is_finite() || !(0.5..=2.).contains(&value) {
                return Err(AppError::new(ErrorCode::InvalidInput, "灵动岛比例须在 0.5 至 2.0 之间"));
            }
            Ok((value * 10.).round() / 10.)
        }
        Ok(Self { compact_scale: validate(compact.unwrap_or(self.compact_scale))?, expanded_scale: validate(expanded.unwrap_or(self.expanded_scale))? })
    }
}
fn read_scales(db: &Database) -> AppResult<IslandScales> {
    if let Some(raw) = db.get_setting("island_scales")? {
        if let Ok(scales) = serde_json::from_str::<IslandScales>(&raw) {
            if let Ok(valid) = scales.with_changes(None, None) { return Ok(valid); }
        }
    }
    let legacy = db.get_setting("island_size")?.and_then(|s| serde_json::from_str::<IslandSize>(&s).ok());
    legacy.map(IslandScales::from).unwrap_or_default().with_changes(None, None)
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IslandTypography {
    pub font_size: u8,
    pub font_weight: u16,
}
impl Default for IslandTypography {
    fn default() -> Self { Self { font_size: 20, font_weight: 700 } }
}
impl IslandTypography {
    fn with_changes(self, size: Option<u8>, weight: Option<u16>) -> AppResult<Self> {
        let next = Self { font_size: size.unwrap_or(self.font_size), font_weight: weight.unwrap_or(self.font_weight) };
        if !(16..=24).contains(&next.font_size) || ![400, 500, 600, 700, 800].contains(&next.font_weight) {
            return Err(AppError::new(ErrorCode::InvalidInput, "灵动岛字号须为 16–24，字重须为 400、500、600、700 或 800"));
        }
        Ok(next)
    }
}
fn read_typography(db: &Database) -> AppResult<IslandTypography> {
    Ok(db.get_setting("island_typography")?
        .and_then(|raw| serde_json::from_str::<IslandTypography>(&raw).ok())
        .and_then(|value| value.with_changes(None, None).ok()).unwrap_or_default())
}

// T1518: keep disconnected displays in storage; global values are migration defaults only.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
struct DisplayAppearance {
    scales: IslandScales,
    typography: IslandTypography,
}
fn display_appearance(db: &Database, id: &str) -> AppResult<DisplayAppearance> {
    let key = format!("island_display_appearance:{id}");
    if let Some(raw) = db.get_setting(&key)? {
        if let Ok(value) = serde_json::from_str::<DisplayAppearance>(&raw) {
            if value.scales.with_changes(None, None).is_ok() && value.typography.with_changes(None, None).is_ok() {
                return Ok(value);
            }
        }
    }
    let value = DisplayAppearance { scales: read_scales(db)?, typography: read_typography(db)? };
    save_display_appearance(db, id, value)?;
    Ok(value)
}
fn save_display_appearance(db: &Database, id: &str, value: DisplayAppearance) -> AppResult<()> {
    db.set_setting(&format!("island_display_appearance:{id}"), &serde_json::to_string(&value).map_err(error)?)
}

// DISPLAY2 can refer to different monitors at work and at home. Use the device
// interface registered per monitor, independent of resolution, DPI and desktop position.
fn monitor_identity(name: &str) -> String {
    #[cfg(windows)]
    unsafe {
        use windows::{core::PCWSTR, Win32::{Graphics::Gdi::{EnumDisplayDevicesW, DISPLAY_DEVICEW}, UI::WindowsAndMessaging::EDD_GET_DEVICE_INTERFACE_NAME}};
        let name_wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let mut device = DISPLAY_DEVICEW { cb: size_of::<DISPLAY_DEVICEW>() as u32, ..Default::default() };
        if EnumDisplayDevicesW(PCWSTR(name_wide.as_ptr()), 0, &mut device, EDD_GET_DEVICE_INTERFACE_NAME).as_bool() {
            let len = device.DeviceID.iter().position(|c| *c == 0).unwrap_or(device.DeviceID.len());
            if len > 0 { return format!("device:{}", String::from_utf16_lossy(&device.DeviceID[..len]).to_lowercase()); }
        }
    }
    name.to_owned()
}

fn active_monitor<'a>(monitors: &'a [IslandMonitor], preferred: &str) -> Option<&'a IslandMonitor> {
    monitors.iter().find(|m| m.id == preferred || m.name == preferred)
        .or_else(|| monitors.iter().find(|m| m.primary)).or(monitors.first())
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum IslandForm {
    #[default]
    Compact,
    Expanded,
    Toast,
    Menu,
}
impl IslandForm {
    fn dimensions(self) -> (f64, f64) {
        match self {
            Self::Compact => (COMPACT_MIN_WIDTH, 42.),
            Self::Expanded => (EXPANDED_WIDTH, EXPANDED_HEIGHT),
            Self::Toast => (232., 38.),
            Self::Menu => (280., MENU_HEIGHT),
        }
    }
}
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IslandMonitor {
    pub id: String,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
    pub primary: bool,
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum IslandSubtitlePreference { Chinese, #[default] Original }

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum IslandExpandMode { Focus, #[default] Click }

fn read_expand_mode(db: &Database) -> AppResult<IslandExpandMode> {
    Ok(db.get_setting("island_expand_mode")?
        .and_then(|raw| serde_json::from_str(&raw).ok()).unwrap_or_default())
}

#[test]
fn island_expand_mode_defaults_and_persists() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let cache = dir.path().join("cache");
    let db = Database::open_and_migrate(&data, &cache).unwrap();
    assert_eq!(read_expand_mode(&db).unwrap(), IslandExpandMode::Click);
    for mode in [IslandExpandMode::Focus, IslandExpandMode::Click] {
        db.set_setting("island_expand_mode", &serde_json::to_string(&mode).unwrap()).unwrap();
        let reopened = Database::open_and_migrate(&data, &cache).unwrap();
        assert_eq!(read_expand_mode(&reopened).unwrap(), mode);
    }
    assert!(serde_json::from_str::<IslandExpandMode>("\"invalid\"").is_err());
    db.set_setting("island_expand_mode", "invalid").unwrap();
    assert_eq!(read_expand_mode(&db).unwrap(), IslandExpandMode::Click);
}

fn read_subtitle_preference(db: &Database) -> AppResult<IslandSubtitlePreference> {
    Ok(db.get_setting("island_subtitle_preference")?
        .and_then(|raw| serde_json::from_str(&raw).ok()).unwrap_or_default())
}

#[test]
fn island_subtitle_preference_defaults_and_persists() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let cache = dir.path().join("cache");
    let db = Database::open_and_migrate(&data, &cache).unwrap();
    assert_eq!(read_subtitle_preference(&db).unwrap(), IslandSubtitlePreference::Original);
    for mode in [IslandSubtitlePreference::Chinese, IslandSubtitlePreference::Original] {
        db.set_setting("island_subtitle_preference", &serde_json::to_string(&mode).unwrap()).unwrap();
        let reopened = Database::open_and_migrate(&data, &cache).unwrap();
        assert_eq!(read_subtitle_preference(&reopened).unwrap(), mode);
    }
    assert!(serde_json::from_str::<IslandSubtitlePreference>("\"bilingual\"").is_err());
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IslandLayout {
    pub expand_mode: IslandExpandMode,
    pub subtitle_preference: IslandSubtitlePreference,
    #[serde(flatten)]
    pub typography: IslandTypography,
    pub revision: u64,
    pub form: IslandForm,
    pub scale: f64,
    pub width: f64,
    pub height: f64,
    pub radius: f64,
    pub animate: bool,
    pub native_glass: bool,
    pub viewport: Option<IslandViewport>,
}
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IslandViewport {
    pub width: u32,
    pub height: u32,
    pub device_scale: f64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IslandPreferences {
    #[serde(flatten)]
    pub scales: IslandScales,
    pub monitor_id: String,
    pub active_monitor_id: String,
    pub monitors: Vec<IslandMonitor>,
    pub layout: IslandLayout,
}
#[derive(Clone, Serialize, Deserialize)]
struct Anchor {
    monitor_id: String,
    center_ratio: f64,
    top: f64,
}

struct State {
    layout: IslandLayout,
    monitors: Vec<IslandMonitor>,
    position: PhysicalPosition<i32>,
    moved: bool,
    avoided: bool,
    compact_width: f64,
    canvas: PhysicalSize<u32>,
    pending: Option<(u64, PhysicalSize<u32>, PhysicalSize<u32>, i32)>,
    ready_revision: Option<u64>,
    surface_ready: bool,
}
#[derive(Clone)]
pub struct IslandService {
    db: Database,
    app: AppHandle,
    state: Arc<Mutex<State>>,
    operation: Arc<Mutex<()>>,
}
fn native_glass_ready() -> bool {
    #[cfg(windows)]
    {
        super::island_glass::is_ready()
    }
    #[cfg(not(windows))]
    {
        false
    }
}
fn error(e: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::InternalError, format!("无法调整灵动岛：{e}"))
}

// One Win32 transaction preserves the screen-space center during resize/reposition.
fn apply_bounds(
    win: &WebviewWindow,
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
) -> AppResult<()> {
    #[cfg(windows)]
    {
        let raw = win.hwnd().map_err(error)?.0 as isize;
        win.run_on_main_thread(move || unsafe {
            use windows::Win32::{
                Foundation::HWND,
                UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER},
            };
            if let Err(e) = SetWindowPos(
                HWND(raw as *mut _),
                None,
                position.x,
                position.y,
                size.width as i32,
                size.height as i32,
                SWP_NOACTIVATE | SWP_NOZORDER,
            ) {
                eprintln!("Island window bounds: {e}");
            }
        })
        .map_err(error)?;
    }
    #[cfg(not(windows))]
    {
        win.set_size(size).map_err(error)?;
        win.set_position(position).map_err(error)?;
    }
    Ok(())
}

#[cfg(windows)]
fn rounded_input_region(
    canvas: PhysicalSize<u32>,
    size: PhysicalSize<u32>,
    radius: i32,
) -> windows::Win32::Graphics::Gdi::HRGN {
    // Keep native bounds on the capsule. Expanding this region exposes the
    // window's backing surface, producing a persistent rim outside the glass.
    let left = (canvas.width as i32 - size.width as i32) / 2;
    unsafe {
        windows::Win32::Graphics::Gdi::CreateRoundRectRgn(
            left,
            0,
            left + size.width as i32 + 1,
            size.height as i32 + 1,
            radius * 2,
            radius * 2,
        )
    }
}

fn apply_region(
    win: &WebviewWindow,
    canvas: PhysicalSize<u32>,
    shape: Option<(PhysicalSize<u32>, i32)>,
) -> AppResult<()> {
    #[cfg(windows)]
    {
        let raw = win.hwnd().map_err(error)?.0 as isize;
        win.run_on_main_thread(move || unsafe {
            use windows::Win32::{
                Foundation::HWND,
                Graphics::Gdi::{
                    DeleteObject, RedrawWindow, SetWindowRgn, RDW_ALLCHILDREN,
                    RDW_INVALIDATE, RDW_NOERASE,
                },
            };
            let region = shape.map(|(size, radius)| rounded_input_region(canvas, size, radius));
            if region.is_some_and(|r| r.0.is_null()) {
                eprintln!("Island input region allocation failed");
                return;
            }
            // Keep the viewport fixed; the region bounds idle input without touching the visible curve.
            let hwnd = HWND(raw as *mut _);
            // Avoid SetWindowRgn's synchronous frame/background repaint, which
            // exposes a white backing surface before WebView/DComp supplies a frame.
            if SetWindowRgn(hwnd, region, false) == 0 {
                if let Some(region) = region {
                    let _ = DeleteObject(region.into());
                }
                eprintln!("Island input region update failed");
                return;
            }
            // A changed HWND region needs a fresh WebView frame, including newly
            // exposed pixels. Validating this area without painting leaves stale
            // fragments at the capsule edge. Keep repaint asynchronous and avoid
            // requesting an opaque GDI background erase over the composition layer.
            let _ = RedrawWindow(
                Some(hwnd),
                None,
                None,
                RDW_INVALIDATE | RDW_NOERASE | RDW_ALLCHILDREN,
            );
        })
        .map_err(error)?;
    }
    #[cfg(not(windows))]
    let _ = (win, canvas, shape);
    Ok(())
}

fn canvas_geometry(
    mon: &IslandMonitor,
    scales: impl Into<IslandScales>,
    anchor: Option<&Anchor>,
) -> (PhysicalPosition<i32>, PhysicalSize<u32>) {
    let scales = scales.into();
    let (position, expanded, _) = geometry(mon, scales, IslandForm::Expanded, anchor, EXPANDED_WIDTH);
    let (_, widest, _) = geometry(mon, scales, IslandForm::Compact, anchor, COMPACT_MAX_WIDTH);
    let (_, menu, _) = geometry(mon, scales, IslandForm::Menu, anchor, EXPANDED_WIDTH);
    let canvas = PhysicalSize::new(
        (widest.width.max(expanded.width).max(menu.width) as f64 * 1.06).ceil() as u32,
        (expanded.height.max(widest.height).max(menu.height) as f64 * 1.06).ceil() as u32,
    );
    let margin = (2. * mon.scale_factor).round() as i32;
    let center = anchor.map_or(0.5, |a| a.center_ratio.clamp(0., 1.));
    let x = mon.x + (mon.width as f64 * center - canvas.width as f64 / 2.).round() as i32;
    let max_x = (mon.x + mon.width as i32 - canvas.width as i32 - margin).max(mon.x + margin);
    let max_y = (mon.y + mon.height as i32 - canvas.height as i32 - margin).max(mon.y + margin);
    (
        PhysicalPosition::new(
            x.clamp(mon.x + margin, max_x),
            position.y.clamp(mon.y + margin, max_y),
        ),
        canvas,
    )
}

// Origins stay in physical pixels, even on a negative-coordinate, mixed-DPI monitor.
fn geometry(
    mon: &IslandMonitor,
    scales: impl Into<IslandScales>,
    form: IslandForm,
    anchor: Option<&Anchor>,
    compact_width: f64,
) -> (PhysicalPosition<i32>, PhysicalSize<u32>, f64) {
    let dpi = mon.scale_factor.max(0.5);
    // 1.0 is exactly the former Small preset, before display/DPI fitting.
    let user_scale = 0.8 * scales.into().for_form(form);
    // Match the reference footprint at 1440p, compensating for Windows DPI.
    let display_scale = (mon.height as f64 / dpi / 1152.).clamp(0.5, 1.5);
    let scale = (user_scale * display_scale)
        .min(((mon.width as f64 / dpi - 24.) / (COMPACT_MAX_WIDTH * 1.06)).max(0.1))
        .min(((mon.height as f64 / dpi - 24.) / (EXPANDED_HEIGHT.max(MENU_HEIGHT) * 1.06)).max(0.1));
    let (base_w, h) = form.dimensions();
    let w = if matches!(form, IslandForm::Compact) {
        compact_width.clamp(COMPACT_MIN_WIDTH, COMPACT_MAX_WIDTH)
    } else {
        base_w
    };
    let width = (w * scale * dpi).round().max(1.) as u32;
    let height = (h * scale * dpi).round().max(1.) as u32;
    let margin = (8. * dpi).round() as i32;
    let center = anchor.map_or(0.5, |a| a.center_ratio.clamp(0., 1.));
    let top = anchor.map_or(2., |a| if a.top <= 8. { 2. } else { a.top });
    let x = mon.x + (mon.width as f64 * center - width as f64 / 2.).round() as i32;
    let y = mon.y + (top * dpi).round() as i32;
    let max_x = (mon.x + mon.width as i32 - width as i32 - margin).max(mon.x + margin);
    let max_y = (mon.y + mon.height as i32 - height as i32 - margin).max(mon.y + margin);
    (
        PhysicalPosition::new(
            x.clamp(mon.x + margin, max_x),
            y.clamp(mon.y + (2. * dpi).round() as i32, max_y),
        ),
        PhysicalSize::new(width, height),
        scale,
    )
}
impl IslandService {
    pub fn new(db: Database, app: AppHandle) -> Self {
        Self {
            db,
            app,
            operation: Arc::new(Mutex::new(())),
            state: Arc::new(Mutex::new(State {
                layout: IslandLayout {
                    expand_mode: IslandExpandMode::default(),
                    subtitle_preference: IslandSubtitlePreference::default(),
                    typography: IslandTypography::default(),
                    revision: 0,
                    form: IslandForm::Compact,
                    scale: 1.,
                    width: COMPACT_MIN_WIDTH,
                    height: 42.,
                    radius: 21.,
                    animate: false,
                    native_glass: false,
                    viewport: None,
                },
                monitors: vec![],
                position: PhysicalPosition::new(0, 0),
                moved: false,
                avoided: false,
                compact_width: COMPACT_MIN_WIDTH,
                canvas: PhysicalSize::new(0, 0),
                pending: None,
                ready_revision: None,
                surface_ready: false,
            })),
        }
    }
    fn window(&self) -> AppResult<WebviewWindow> {
        self.app
            .get_webview_window("island")
            .ok_or_else(|| error("未找到窗口"))
    }
    fn monitors(&self) -> AppResult<Vec<IslandMonitor>> {
        let win = self.window()?;
        let primary = win.primary_monitor().map_err(error)?;
        Ok(win
            .available_monitors()
            .map_err(error)?
            .into_iter()
            .map(|m| {
                let name = m
                    .name()
                    .cloned()
                    .unwrap_or_else(|| format!("屏幕 {},{}", m.position().x, m.position().y));
                IslandMonitor {
                    id: monitor_identity(&name),
                    name,
                    x: m.position().x,
                    y: m.position().y,
                    width: m.size().width,
                    height: m.size().height,
                    scale_factor: m.scale_factor(),
                    primary: primary
                        .as_ref()
                        .is_some_and(|p| p.position() == m.position()),
                }
            })
            .collect())
    }
    fn anchor(&self) -> AppResult<Option<Anchor>> {
        Ok(self
            .db
            .get_setting("island_anchor_v2")?
            .and_then(|s| serde_json::from_str(&s).ok()))
    }
    pub fn preferences(&self) -> AppResult<IslandPreferences> {
        let monitors = self.monitors()?;
        let mut monitor_id = self.db.get_setting("island_monitor")?.unwrap_or_default();
        // Bind an old DISPLAYn preference once, then never resolve it by ordinal again.
        if let Some(mon) = monitors.iter().find(|m| m.name == monitor_id && m.id != monitor_id) {
            monitor_id = mon.id.clone();
            self.db.set_setting("island_monitor", &monitor_id)?;
            if let Some(mut anchor) = self.anchor()?.filter(|a| a.monitor_id == mon.name) {
                anchor.monitor_id = mon.id.clone();
                self.db.set_setting("island_anchor_v2", &serde_json::to_string(&anchor).map_err(error)?)?;
            }
        }
        let active = active_monitor(&monitors, &monitor_id).ok_or_else(|| error("没有可用屏幕"))?;
        let appearance = display_appearance(&self.db, &active.id)?;
        Ok(IslandPreferences {
            scales: appearance.scales,
            monitor_id,
            active_monitor_id: active.id.clone(),
            monitors,
            layout: IslandLayout {
                expand_mode: read_expand_mode(&self.db)?,
                subtitle_preference: read_subtitle_preference(&self.db)?,
                typography: appearance.typography,
                native_glass: native_glass_ready(),
                ..self.state.lock().layout.clone()
            },
        })
    }
    pub fn set_preferences(
        &self,
        compact_scale: Option<f64>,
        expanded_scale: Option<f64>,
        monitor_id: Option<String>,
        font_size: Option<u8>,
        font_weight: Option<u16>,
        subtitle_preference: Option<IslandSubtitlePreference>,
        expand_mode: Option<IslandExpandMode>,
    ) -> AppResult<IslandPreferences> {
        let _operation = self.operation.lock();
        let current = self.preferences()?;
        let selected = monitor_id.as_deref().unwrap_or(&current.monitor_id);
        if !selected.is_empty() && monitor_id.is_some() && !current.monitors.iter().any(|m| m.id == selected) {
            return Err(error("所选屏幕已断开，请刷新屏幕列表"));
        }
        let active = active_monitor(&current.monitors, selected).ok_or_else(|| error("没有可用屏幕"))?;
        let appearance = display_appearance(&self.db, &active.id)?;
        let next = DisplayAppearance {
            scales: appearance.scales.with_changes(compact_scale, expanded_scale)?,
            typography: appearance.typography.with_changes(font_size, font_weight)?,
        };
        if let Some(preference) = subtitle_preference {
            self.db.set_setting("island_subtitle_preference", &serde_json::to_string(&preference).map_err(error)?)?;
        }
        if let Some(mode) = expand_mode {
            self.db.set_setting("island_expand_mode", &serde_json::to_string(&mode).map_err(error)?)?;
        }
        if let Some(id) = monitor_id {
            self.db.set_setting("island_monitor", &id)?;
            self.db.set_setting("island_anchor_v2", "")?;
        }
        if next != appearance {
            save_display_appearance(&self.db, &active.id, next)?;
        }
        let form = self.state.lock().layout.form;
        self.layout_form_locked(form, false, None, false)?;
        self.preferences()
    }
    pub fn set_form(&self, form: IslandForm) -> AppResult<IslandLayout> {
        self.layout_form(form, false, None)
    }
    pub fn reset_position(&self) -> AppResult<IslandLayout> {
        let _operation = self.operation.lock();
        self.db.set_setting("island_anchor_v2", "")?;
        let form = self.state.lock().layout.form;
        self.layout_form_locked(form, false, None, false)
    }
    pub fn layout_form(
        &self,
        form: IslandForm,
        animate: bool,
        compact_width: Option<f64>,
    ) -> AppResult<IslandLayout> {
        let _operation = self.operation.lock();
        self.layout_form_locked(form, animate, compact_width, false)
    }
    fn layout_form_locked(
        &self,
        form: IslandForm,
        animate: bool,
        compact_width: Option<f64>,
        refresh_viewport: bool,
    ) -> AppResult<IslandLayout> {
        if let Some(width) = compact_width {
            if !width.is_finite() {
                return Err(error("字幕宽度无效"));
            }
            self.state.lock().compact_width = width.clamp(COMPACT_MIN_WIDTH, COMPACT_MAX_WIDTH);
        }
        let preferences = self.preferences()?;
        let mon = preferences
            .monitors
            .iter()
            .find(|m| m.id == preferences.active_monitor_id)
            .ok_or_else(|| error("没有可用屏幕"))?;
        let dpi = mon.scale_factor;
        let anchor = self.anchor()?.filter(|a| a.monitor_id == mon.id);
        let compact_width = self.state.lock().compact_width;
        let (_, physical_size, scale) =
            geometry(mon, preferences.scales, form, anchor.as_ref(), compact_width);
        let (position, canvas) = canvas_geometry(mon, preferences.scales, anchor.as_ref());
        let requested_width = if matches!(form, IslandForm::Compact) {
            compact_width
        } else {
            form.dimensions().0
        };
        {
            let state = self.state.lock();
            if !refresh_viewport && state.layout.form == form
                && state.layout.typography == preferences.layout.typography
                && state.layout.subtitle_preference == preferences.layout.subtitle_preference
                && state.layout.expand_mode == preferences.layout.expand_mode
                && state.layout.width == requested_width
                && state.layout.height == form.dimensions().1
                && (state.layout.scale - scale).abs() < 0.000001
                && state.position == position
                && state.canvas == canvas
                && state.monitors == preferences.monitors
                && (animate || state.pending.is_none())
            {
                return Ok(IslandLayout {
                    native_glass: native_glass_ready(),
                    ..state.layout.clone()
                });
            }
        }
        let win = self.window()?;
        let animated = animate && win.is_visible().unwrap_or(false);
        let layout = {
            let mut state = self.state.lock();
            state.position = position;
            state.canvas = canvas;
            state.moved = false;
            state.layout = IslandLayout {
                expand_mode: preferences.layout.expand_mode,
                subtitle_preference: preferences.layout.subtitle_preference,
                typography: preferences.layout.typography,
                revision: state.layout.revision + 1,
                form,
                scale,
                width: if matches!(form, IslandForm::Compact) {
                    compact_width
                } else {
                    form.dimensions().0
                },
                height: form.dimensions().1,
                radius: if matches!(form, IslandForm::Compact | IslandForm::Toast) {
                    21.
                } else {
                    28.
                },
                animate: animated,
                native_glass: native_glass_ready(),
                viewport: Some(IslandViewport { width: canvas.width, height: canvas.height, device_scale: dpi }),
            };
            state.pending = if animated {
                Some((
                    state.layout.revision,
                    canvas,
                    physical_size,
                    (state.layout.radius * scale * dpi).round() as i32,
                ))
            } else {
                None
            };
            state.monitors = preferences.monitors;
            state.layout.clone()
        };
        if win.outer_position().map_err(error)? != position
            || win.outer_size().map_err(error)? != canvas
        {
            apply_bounds(&win, position, canvas)?;
        }
        // SetWindowPos and a monitor DPI change can leave the child WebView on
        // its old bounds until a later resize message. Queue explicit physical
        // bounds after the host move, before asking React to acknowledge it.
        let webview: &tauri::Webview = win.as_ref();
        webview.set_bounds(tauri::Rect {
            position: PhysicalPosition::new(0, 0).into(),
            size: canvas.into(),
        }).map_err(error)?;
        if refresh_viewport { win.set_zoom(1.).map_err(error)?; }
        apply_region(
            &win,
            canvas,
            if animated {
                None
            } else {
                Some((physical_size, (layout.radius * scale * dpi).round() as i32))
            },
        )?;
        #[cfg(windows)]
        super::island_glass::update(
            &win,
            (layout.width * scale * dpi) as f32,
            (layout.height * scale * dpi) as f32,
            (layout.radius * scale * dpi) as f32,
            animated,
        );
        let _ = self.app.emit("island:layout", &layout);
        Ok(layout)
    }
    pub fn settle(&self, revision: u64) -> AppResult<()> {
        let _operation = self.operation.lock();
        let target = {
            let mut state = self.state.lock();
            match state.pending {
                Some((current, canvas, size, radius)) if current == revision => {
                    state.pending = None;
                    state.moved = false;
                    Some((canvas, size, radius, state.layout.clone()))
                }
                _ => None,
            }
        };
        if let Some((canvas, size, radius, layout)) = target {
            let win = self.window()?;
            // Commit the final composition geometry before applying the idle
            // region. Completed keyframe animations must not retain old base sizes.
            #[cfg(windows)]
            {
                let dpi = win.scale_factor().map_err(error)?;
                super::island_glass::update(
                    &win,
                    (layout.width * layout.scale * dpi) as f32,
                    (layout.height * layout.scale * dpi) as f32,
                    (layout.radius * layout.scale * dpi) as f32,
                    false,
                );
            }
            #[cfg(not(windows))]
            let _ = layout;
            apply_region(&win, canvas, Some((size, radius)))?;
        }
        Ok(())
    }
    pub fn refresh_displays(&self) -> AppResult<()> {
        let monitors = self.monitors()?;
        let (changed, form) = {
            let state = self.state.lock();
            (monitors != state.monitors, state.layout.form)
        };
        if changed {
            self.set_form(form)?;
        } else {
            let moved = self.state.lock().moved;
            if moved {
                self.save_position()?;
            }
        }
        Ok(())
    }
    pub fn refresh_viewport(&self) -> AppResult<()> {
        // A drag can change DPI before the periodic position capture. Preserve
        // that destination instead of moving the island back to its old monitor.
        self.save_position()?;
        let _operation = self.operation.lock();
        let form = self.state.lock().layout.form;
        self.layout_form_locked(form, false, None, true)?;
        Ok(())
    }
    pub fn moved(&self, position: PhysicalPosition<i32>) {
        let mut state = self.state.lock();
        if state.pending.is_none() && position != state.position {
            state.moved = true;
        }
    }
    pub fn save_position(&self) -> AppResult<()> {
        let _operation = self.operation.lock();
        // Windows may relocate the window before our display poll after unplugging.
        // That system move must not replace the user's preferred display with the fallback.
        let monitors = self.monitors()?;
        let topology_changed = {
            let state = self.state.lock();
            !state.monitors.is_empty() && state.monitors != monitors
        };
        if topology_changed {
            let form = self.state.lock().layout.form;
            self.layout_form_locked(form, false, None, false)?;
            return Ok(());
        }
        let win = self.window()?;
        let Some(current) = win.current_monitor().map_err(error)? else {
            return Ok(());
        };
        let pos = win.outer_position().map_err(error)?;
        {
            let mut state = self.state.lock();
            state.moved = false;
            if state.pending.is_some() || pos == state.position {
                return Ok(());
            }
        }
        let size = win.outer_size().map_err(error)?;
        let name = monitor_identity(current.name().map(String::as_str).unwrap_or_default());
        let mut top = (pos.y - current.position().y) as f64 / current.scale_factor();
        if self.db.get_setting("island_edge_snap")?.as_deref() != Some("false") && top < 30. {
            top = 2.;
        }
        let anchor = Anchor {
            monitor_id: name.clone(),
            center_ratio: ((pos.x - current.position().x) as f64 + size.width as f64 / 2.)
                / current.size().width as f64,
            top,
        };
        self.db.set_setting(
            "island_anchor_v2",
            &serde_json::to_string(&anchor).map_err(error)?,
        )?;
        self.db.set_setting("island_monitor", &name)?;
        let form = self.state.lock().layout.form;
        self.layout_form_locked(form, false, None, false)?;
        Ok(())
    }
    pub fn show(&self, open: bool) -> AppResult<bool> {
        let win = self.window()?;
        self.db.set_setting("island_open", if open { "true" } else { "false" })?;
        if open {
            self.set_form(IslandForm::Compact)?;
            win.set_always_on_top(
                self.db.get_setting("island_always_on_top")?.as_deref() != Some("false"),
            )
            .map_err(error)?;
            self.show_if_ready()?;
        } else {
            win.hide().map_err(error)?;
        }
        let _ = self.app.emit("island:visibility", open);
        Ok(open)
    }
    // T1416: only reveal a layout that the WebView has committed. Obsolete
    // acknowledgments and rapid close/open cannot reveal the wrong geometry.
    pub fn layout_ready(&self, revision: u64) -> AppResult<()> {
        {
            let mut state = self.state.lock();
            if revision != state.layout.revision { return Ok(()); }
            state.ready_revision = Some(revision);
        }
        self.show_if_ready()
    }
    fn show_if_ready(&self) -> AppResult<()> {
        let service = self.clone();
        self.app.run_on_main_thread(move || {
            let state = service.state.lock();
            if state.surface_ready && state.ready_revision == Some(state.layout.revision) && !state.avoided && service.is_open() {
                if let Ok(win) = service.window() { let _ = win.show(); }
            }
        }).map_err(error)
    }
    // Called on the setup/UI thread immediately after creation, before any reveal.
    // Do not acquire the layout-operation lock or query monitors here.
    pub fn prepare_window(&self, win: &WebviewWindow) -> AppResult<()> {
        #[cfg(windows)]
        {
            let app = self.app.clone();
            super::island_window::prepare(win.hwnd().map_err(error)?, move |action| {
                if let Some(state) = app.try_state::<crate::commands::AppState>() {
                    let playback = state.playback.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = playback.handle_smtc_action(action, None).await;
                    });
                }
            }).map_err(error)?;
        }
        // Native clipping uses monitor DPI × island scale; browser zoom must
        // stay neutral rather than adding a third, independently restored scale.
        if let Err(e) = win.set_zoom(1.) {
            eprintln!("Island WebView zoom: {e}");
        }
        // Tao's native background brush ignores alpha. Only WebView gets an
        // RGBA background; the host must not erase transparent pixels with GDI.
        // Keep the transparent HWND's initial DWM setup instead of clearing
        // effects that were never installed on this window.
        let webview: &tauri::Webview = win.as_ref();
        webview.window().set_background_color(None).map_err(error)?;
        webview.set_background_color(Some(tauri::window::Color(0, 0, 0, 0))).map_err(error)?;
        #[cfg(windows)]
        if let Ok(hwnd) = win.hwnd() {
            let raw = hwnd.0 as isize;
            let _ = win.run_on_main_thread(move || unsafe {
                use windows::Win32::{
                    Foundation::HWND,
                    Graphics::Dwm::{
                        DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE,
                        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
                    },
                };
                // DComp owns capsule rounding; Windows must not add its own
                // focus border/corners to the changing input region. Older
                // Windows versions may not support these optional attributes.
                let border = DWMWA_COLOR_NONE;
                let corners = DWMWCP_DONOTROUND.0;
                let _ = DwmSetWindowAttribute(
                    HWND(raw as *mut _),
                    DWMWA_BORDER_COLOR,
                    (&border as *const u32).cast(),
                    size_of_val(&border) as u32,
                );
                let _ = DwmSetWindowAttribute(
                    HWND(raw as *mut _),
                    DWMWA_WINDOW_CORNER_PREFERENCE,
                    (&corners as *const i32).cast(),
                    size_of_val(&corners) as u32,
                );
            });
        }
        self.state.lock().surface_ready = true;
        Ok(())
    }
    pub fn initialize(&self) {
        let _ = self.set_form(IslandForm::Compact);
        if self.is_open() {
            let _ = self.show(true);
        }
    }
    pub fn is_open(&self) -> bool {
        self.db.get_setting("island_open").ok().flatten().as_deref() == Some("true")
    }
    pub fn avoid_fullscreen(&self, avoided: bool) {
        self.state.lock().avoided = avoided;
        if let Ok(win) = self.window() {
            if avoided {
                let _ = win.hide();
            } else if self.is_open() {
                let _ = self.show_if_ready();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t1518_three_displays_restore_independent_appearance_after_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("d");
        let cache = dir.path().join("c");
        let db = Database::open_and_migrate(&db_path, &cache).unwrap();
        db.set_setting("island_scales", r#"{"compactScale":0.8,"expandedScale":1.4}"#).unwrap();
        db.set_setting("island_typography", r#"{"fontSize":22,"fontWeight":600}"#).unwrap();
        let legacy = display_appearance(&db, "laptop").unwrap();
        assert_eq!(legacy.scales.compact_scale, 0.8);
        assert_eq!(legacy.typography.font_size, 22);
        let office = DisplayAppearance {
            scales: IslandScales { compact_scale: 1.3, expanded_scale: 1.6 },
            typography: IslandTypography { font_size: 24, font_weight: 800 },
        };
        let dorm = DisplayAppearance {
            scales: IslandScales { compact_scale: 0.7, expanded_scale: 0.9 },
            typography: IslandTypography { font_size: 18, font_weight: 400 },
        };
        assert_eq!(display_appearance(&db, "office").unwrap(), legacy);
        save_display_appearance(&db, "office", office).unwrap();
        // A new screen inherits the old settings, not the last adjusted display.
        assert_eq!(display_appearance(&db, "dorm").unwrap(), legacy);
        save_display_appearance(&db, "dorm", dorm).unwrap();
        drop(db);
        let db = Database::open_and_migrate(&db_path, &cache).unwrap();
        for _ in 0..3 {
            assert_eq!(display_appearance(&db, "laptop").unwrap(), legacy);
            assert_eq!(display_appearance(&db, "office").unwrap(), office);
            assert_eq!(display_appearance(&db, "dorm").unwrap(), dorm);
        }
    }

    #[test]
    fn t1518_display_identity_survives_reordering_and_unplugging() {
        let laptop = IslandMonitor { id: "device:laptop".into(), name: "DISPLAY1".into(), primary: true, ..monitor() };
        let office = IslandMonitor { id: "device:office".into(), name: "DISPLAY2".into(), ..monitor() };
        let dorm = IslandMonitor { id: "device:dorm".into(), name: "DISPLAY2".into(), ..monitor() };
        let preferred = office.id.clone();
        let at_work = vec![laptop.clone(), office.clone()];
        assert_eq!(active_monitor(&at_work, &preferred).unwrap().id, office.id);
        let at_home = vec![laptop.clone(), dorm];
        assert_eq!(active_monitor(&at_home, &preferred).unwrap().id, laptop.id);
        assert_eq!(active_monitor(&at_home, "").unwrap().id, laptop.id);
        let reconnected = vec![IslandMonitor { name: "DISPLAY3".into(), x: 1920, scale_factor: 2., ..office }, laptop];
        assert_eq!(active_monitor(&reconnected, &preferred).unwrap().id, preferred);
    }

    fn monitor() -> IslandMonitor {
        IslandMonitor {
            id: "secondary".into(),
            name: "secondary".into(),
            x: -2560,
            y: -300,
            width: 2560,
            height: 1440,
            scale_factor: 1.5,
            primary: false,
        }
    }
    #[test]
    fn negative_monitor_dpi_keeps_center_across_forms() {
        let m = monitor();
        for form in [
            IslandForm::Compact,
            IslandForm::Expanded,
            IslandForm::Toast,
            IslandForm::Menu,
        ] {
            let (p, s, scale) = geometry(&m, IslandSize::Medium, form, None, 180.);
            assert_eq!(p.x + s.width as i32 / 2, -1280);
            assert_eq!(p.y, -297);
            assert!((scale - 0.75).abs() < 0.001);
        }
    }
    #[test]
    fn reference_footprint_survives_windows_dpi_and_subtitle_changes() {
        for dpi in [1., 1.25, 1.5, 2.] {
            let m = IslandMonitor {
                x: 0,
                y: 0,
                width: 2560,
                height: 1440,
                scale_factor: dpi,
                ..monitor()
            };
            let (compact_pos, compact, _) =
                geometry(&m, IslandSize::Medium, IslandForm::Compact, None, 180.);
            let (expanded_pos, expanded, _) =
                geometry(&m, IslandSize::Medium, IslandForm::Expanded, None, 180.);
            assert_eq!(compact, PhysicalSize::new(225, 47));
            assert_eq!(expanded, PhysicalSize::new(450, 209));
            assert_eq!(compact_pos.y, expanded_pos.y);
            let (wide_pos, wide, _) =
                geometry(&m, IslandSize::Medium, IslandForm::Compact, None, 360.);
            assert_eq!(wide.width, 405);
            assert_eq!(wide_pos.x + wide.width as i32 / 2, 1280);
            let (_, capped, _) =
                geometry(&m, IslandSize::Medium, IslandForm::Compact, None, 10000.);
            assert_eq!(capped.width, 720);
        }
    }

    #[test]
    fn independent_ratios_preserve_small_baseline_and_fit_canvas() {
        let mon = monitor();
        let base = IslandScales::default();
        assert_eq!(geometry(&mon, base, IslandForm::Compact, None, 400.), geometry(&mon, IslandSize::Small, IslandForm::Compact, None, 400.));
        let larger = base.with_changes(None, Some(1.1)).unwrap();
        assert_eq!(geometry(&mon, base, IslandForm::Compact, None, 400.), geometry(&mon, larger, IslandForm::Compact, None, 400.));
        assert!(geometry(&mon, larger, IslandForm::Expanded, None, 400.).1.width > geometry(&mon, base, IslandForm::Expanded, None, 400.).1.width);
        for compact_scale in [0.5, 1., 2.] {
            for expanded_scale in [0.5, 1., 2.] {
                let scales = IslandScales { compact_scale, expanded_scale };
                let (_, canvas) = canvas_geometry(&mon, scales, None);
                for form in [IslandForm::Compact, IslandForm::Expanded, IslandForm::Toast, IslandForm::Menu] {
                    let (_, shape, _) = geometry(&mon, scales, form, None, COMPACT_MAX_WIDTH);
                    assert!(shape.width < canvas.width && shape.height < canvas.height);
                }
            }
        }
        for invalid in [f64::NAN, f64::INFINITY, 0.4, 2.1] { assert!(base.with_changes(Some(invalid), None).is_err()); }
    }

    #[test]
    fn ratios_migrate_legacy_and_persist_independently() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        assert_eq!(read_scales(&db).unwrap(), IslandScales::default());
        db.set_setting("island_size", "\"small\"").unwrap();
        assert_eq!(read_scales(&db).unwrap(), IslandScales::default());
        db.set_setting("island_size", "\"large\"").unwrap();
        assert_eq!(read_scales(&db).unwrap().compact_scale, 1.3);
        let scales = IslandScales::default().with_changes(Some(0.8), Some(1.4)).unwrap();
        db.set_setting("island_scales", &serde_json::to_string(&scales).unwrap()).unwrap();
        drop(db);
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        assert_eq!(read_scales(&db).unwrap(), scales);
    }

    #[test]
    fn typography_defaults_validates_and_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        assert_eq!(read_typography(&db).unwrap(), IslandTypography::default());
        let typography = IslandTypography::default().with_changes(Some(24), Some(500)).unwrap();
        assert_eq!(typography.with_changes(Some(16), None).unwrap().font_weight, 500);
        for size in [0, 15, 25, 255] { assert!(typography.with_changes(Some(size), None).is_err()); }
        for weight in [0, 450, 900] { assert!(typography.with_changes(None, Some(weight)).is_err()); }
        db.set_setting("island_typography", &serde_json::to_string(&typography).unwrap()).unwrap();
        drop(db);
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        assert_eq!(read_typography(&db).unwrap(), typography);
        db.set_setting("island_typography", "{}").unwrap();
        assert_eq!(read_typography(&db).unwrap(), IslandTypography::default());
    }

    #[test]
    fn sizes_and_resolution_fit_are_independent_of_physical_origin() {
        let m = monitor();
        let widths: Vec<_> = [
            IslandSize::ExtraSmall,
            IslandSize::Small,
            IslandSize::Medium,
            IslandSize::Large,
            IslandSize::ExtraLarge,
        ]
        .into_iter()
        .map(|v| geometry(&m, v, IslandForm::Compact, None, 180.).1.width)
        .collect();
        assert!(widths.windows(2).all(|pair| pair[0] < pair[1]));
        let tiny = IslandMonitor {
            width: 480,
            height: 320,
            scale_factor: 2.,
            ..m
        };
        let anchor = Anchor {
            monitor_id: tiny.id.clone(),
            center_ratio: 1.5,
            top: 900.,
        };
        let (p, s, scale) = geometry(
            &tiny,
            IslandSize::ExtraLarge,
            IslandForm::Menu,
            Some(&anchor),
            180.,
        );
        assert!(scale < 1.);
        assert!(p.x >= tiny.x && p.x + s.width as i32 <= tiny.x + tiny.width as i32);
        assert!(p.y >= tiny.y && p.y + s.height as i32 <= tiny.y + tiny.height as i32);
    }

    #[test]
    fn fixed_canvas_contains_every_form_and_keeps_dragged_center_on_restore() {
        for dpi in [1., 1.25, 1.5, 2.] {
            let mon = IslandMonitor {
                scale_factor: dpi,
                ..monitor()
            };
            for size in [
                IslandSize::ExtraSmall,
                IslandSize::Small,
                IslandSize::Medium,
                IslandSize::Large,
                IslandSize::ExtraLarge,
            ] {
                for ratio in [0., 0.5, 0.73, 1.] {
                    let anchor = Anchor {
                        monitor_id: mon.id.clone(),
                        center_ratio: ratio,
                        top: 2.,
                    };
                    let (origin, canvas) = canvas_geometry(&mon, size, Some(&anchor));
                    for form in [
                        IslandForm::Compact,
                        IslandForm::Expanded,
                        IslandForm::Menu,
                        IslandForm::Toast,
                    ] {
                        for cue_width in [100., 200., 400., 640., 10000.] {
                            let (_, shape, _) =
                                geometry(&mon, size, form, Some(&anchor), cue_width);
                            assert!(canvas.width as f64 >= shape.width as f64 * 1.05);
                            assert!(canvas.height as f64 >= shape.height as f64 * 1.05);
                        }
                    }
                    let saved = Anchor {
                        center_ratio: ((origin.x - mon.x) as f64 + canvas.width as f64 / 2.)
                            / mon.width as f64,
                        ..anchor
                    };
                    assert_eq!(canvas_geometry(&mon, size, Some(&saved)), (origin, canvas));
                    assert!(
                        origin.x >= mon.x
                            && origin.x + canvas.width as i32 <= mon.x + mon.width as i32
                    );
                }
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn native_input_region_never_exposes_a_rim_outside_the_capsule() {
        use windows::Win32::Graphics::Gdi::{DeleteObject, PtInRegion};
        for dpi in [1., 1.25, 1.5, 2., 2.25] {
            for (width, height, radius) in [(200., 42., 21.), (400., 186., 28.), (280., 196., 28.)] {
                let size = PhysicalSize::new((width * dpi) as u32, (height * dpi) as u32);
                let canvas = PhysicalSize::new(size.width + 101, size.height + 30);
                let r: f64 = radius * dpi;
                let region = rounded_input_region(canvas, size, r.round() as i32);
                assert!(!region.0.is_null());
                let left = ((canvas.width - size.width) / 2) as i32;
                let center = left + size.width as i32 / 2;
                let middle = size.height as i32 / 2;
                assert!(unsafe { PtInRegion(region, center, middle) }.as_bool());
                for (x, y) in [(left - 1, middle), (left + size.width as i32 + 1, middle),
                    (center, -1), (center, size.height as i32 + 1), (left, 0)] {
                    assert!(!unsafe { PtInRegion(region, x, y) }.as_bool());
                }
                unsafe { let _ = DeleteObject(region.into()); }
            }
        }
    }
    #[test]
    fn cleared_anchor_returns_menu_to_selected_monitor_top_center() {
        for dpi in [1., 1.25, 1.5, 2.] {
            let mon = IslandMonitor { scale_factor: dpi, ..monitor() };
            let anchor = Anchor { monitor_id: mon.id.clone(), center_ratio: 0.73, top: 180. };
            let (dragged, _) = canvas_geometry(&mon, IslandSize::Medium, Some(&anchor));
            let (reset, canvas) = canvas_geometry(&mon, IslandSize::Medium, None);
            assert_ne!(dragged, reset);
            assert!((reset.x + canvas.width as i32 / 2 - (mon.x + mon.width as i32 / 2)).abs() <= 1);
            assert_eq!(reset.y, mon.y + (2. * dpi).round() as i32);
            let (_, menu, _) = geometry(&mon, IslandSize::Medium, IslandForm::Menu, None, 200.);
            assert!(canvas.height as f64 >= menu.height as f64 * 1.05);
        }
    }
}
