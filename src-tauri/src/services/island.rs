//! Desktop island geometry is owned by the host: physical monitor origins, logical UI sizes.
use crate::{
    db::Database,
    domain::{AppError, AppResult, ErrorCode},
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

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
            Self::Compact => (180., 42.),
            Self::Expanded => (308., 192.),
            Self::Toast => (232., 38.),
            Self::Menu => (240., 184.),
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
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IslandLayout {
    pub revision: u64,
    pub form: IslandForm,
    pub scale: f64,
    pub width: f64,
    pub height: f64,
    pub radius: f64,
    pub animate: bool,
    pub native_glass: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IslandPreferences {
    pub size: IslandSize,
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
    pending: Option<(u64, PhysicalSize<u32>, PhysicalSize<u32>, i32)>,
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
                    CreateRoundRectRgn, DeleteObject, RedrawWindow, SetWindowRgn, RDW_ALLCHILDREN,
                    RDW_INVALIDATE, RDW_NOERASE,
                },
            };
            let region = shape.map(|(size, radius)| {
                let left = (canvas.width as i32 - size.width as i32) / 2;
                CreateRoundRectRgn(
                    left,
                    0,
                    left + size.width as i32 + 1,
                    size.height as i32 + 1,
                    radius * 2,
                    radius * 2,
                )
            });
            if region.is_some_and(|r| r.0.is_null()) {
                eprintln!("Island input region allocation failed");
                return;
            }
            // Keep the viewport fixed; the rounded region only clips idle input bounds.
            let hwnd = HWND(raw as *mut _);
            if SetWindowRgn(hwnd, region, true) == 0 {
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
    size: IslandSize,
    anchor: Option<&Anchor>,
) -> (PhysicalPosition<i32>, PhysicalSize<u32>) {
    let (position, largest, _) = geometry(mon, size, IslandForm::Expanded, anchor, 308.);
    let canvas = PhysicalSize::new(
        (largest.width as f64 * 1.06).ceil() as u32,
        (largest.height as f64 * 1.06).ceil() as u32,
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
    size: IslandSize,
    form: IslandForm,
    anchor: Option<&Anchor>,
    compact_width: f64,
) -> (PhysicalPosition<i32>, PhysicalSize<u32>, f64) {
    let dpi = mon.scale_factor.max(0.5);
    let user_scale: f64 = match size {
        IslandSize::ExtraSmall => 0.7,
        IslandSize::Small => 0.8,
        IslandSize::Medium => 0.9,
        IslandSize::Large => 1.,
        IslandSize::ExtraLarge => 1.15,
    };
    // Match the reference footprint at 1440p, compensating for Windows DPI.
    let display_scale = (mon.height as f64 / dpi / 1152.).clamp(0.5, 1.5);
    let scale = (user_scale * display_scale)
        .min(((mon.width as f64 / dpi - 24.) / 360.).max(0.1))
        .min(((mon.height as f64 / dpi - 24.) / 192.).max(0.1));
    let (base_w, h) = form.dimensions();
    let w = if matches!(form, IslandForm::Compact) {
        compact_width
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
                    revision: 0,
                    form: IslandForm::Compact,
                    scale: 1.,
                    width: 180.,
                    height: 42.,
                    radius: 21.,
                    animate: false,
                    native_glass: false,
                },
                monitors: vec![],
                position: PhysicalPosition::new(0, 0),
                moved: false,
                avoided: false,
                compact_width: 180.,
                pending: None,
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
                    id: name.clone(),
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
    fn size(&self) -> AppResult<IslandSize> {
        Ok(self
            .db
            .get_setting("island_size")?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default())
    }
    fn anchor(&self) -> AppResult<Option<Anchor>> {
        Ok(self
            .db
            .get_setting("island_anchor_v2")?
            .and_then(|s| serde_json::from_str(&s).ok()))
    }
    pub fn preferences(&self) -> AppResult<IslandPreferences> {
        let monitors = self.monitors()?;
        let monitor_id = self.db.get_setting("island_monitor")?.unwrap_or_default();
        let active = monitors
            .iter()
            .find(|m| m.id == monitor_id)
            .or_else(|| monitors.iter().find(|m| m.primary))
            .or(monitors.first());
        Ok(IslandPreferences {
            size: self.size()?,
            monitor_id,
            active_monitor_id: active.map(|m| m.id.clone()).unwrap_or_default(),
            monitors,
            layout: IslandLayout {
                native_glass: native_glass_ready(),
                ..self.state.lock().layout.clone()
            },
        })
    }
    pub fn set_preferences(
        &self,
        size: Option<IslandSize>,
        monitor_id: Option<String>,
    ) -> AppResult<IslandPreferences> {
        if let Some(id) = monitor_id {
            if !id.is_empty() && !self.monitors()?.iter().any(|m| m.id == id) {
                return Err(error("所选屏幕已断开，请刷新屏幕列表"));
            }
            self.db.set_setting("island_monitor", &id)?;
            self.db.set_setting("island_anchor_v2", "")?;
        }
        if let Some(size) = size {
            self.db
                .set_setting("island_size", &serde_json::to_string(&size).map_err(error)?)?;
        }
        let form = self.state.lock().layout.form;
        self.set_form(form)?;
        self.preferences()
    }
    pub fn set_form(&self, form: IslandForm) -> AppResult<IslandLayout> {
        self.layout_form(form, false, None)
    }
    pub fn layout_form(
        &self,
        form: IslandForm,
        animate: bool,
        compact_width: Option<f64>,
    ) -> AppResult<IslandLayout> {
        let _operation = self.operation.lock();
        if let Some(width) = compact_width {
            if !width.is_finite() {
                return Err(error("字幕宽度无效"));
            }
            self.state.lock().compact_width = width.clamp(148., 308.);
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
            geometry(mon, preferences.size, form, anchor.as_ref(), compact_width);
        let (position, canvas) = canvas_geometry(mon, preferences.size, anchor.as_ref());
        let requested_width = if matches!(form, IslandForm::Compact) {
            compact_width
        } else {
            form.dimensions().0
        };
        {
            let state = self.state.lock();
            if state.layout.form == form
                && state.layout.width == requested_width
                && state.layout.height == form.dimensions().1
                && (state.layout.scale - scale).abs() < 0.000001
                && state.position == position
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
            state.moved = false;
            state.layout = IslandLayout {
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
                    20.
                },
                animate: animated,
                native_glass: native_glass_ready(),
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
    pub fn moved(&self, position: PhysicalPosition<i32>) {
        let mut state = self.state.lock();
        if state.pending.is_none() && position != state.position {
            state.moved = true;
        }
    }
    pub fn save_position(&self) -> AppResult<()> {
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
        let name = current.name().cloned().unwrap_or_default();
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
        self.set_form(form)?;
        Ok(())
    }
    pub fn show(&self, open: bool) -> AppResult<bool> {
        let win = self.window()?;
        if open {
            self.set_form(IslandForm::Compact)?;
            win.set_always_on_top(
                self.db.get_setting("island_always_on_top")?.as_deref() != Some("false"),
            )
            .map_err(error)?;
            if !self.state.lock().avoided {
                win.show().map_err(error)?;
            }
        } else {
            win.hide().map_err(error)?;
        }
        self.db
            .set_setting("island_open", if open { "true" } else { "false" })?;
        let _ = self.app.emit("island:visibility", open);
        Ok(open)
    }
    pub fn initialize(&self) {
        if let Ok(win) = self.window() {
            // Whole-window Acrylic/Blur paints a rectangular backing surface on some GPUs.
            // Keep the native and WebView surfaces alpha-transparent; glass tint is in CSS.
            let _ = win.set_effects(None::<tauri::utils::config::WindowEffectsConfig>);
            let _ = win.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
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
        }
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
                let _ = win.show();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            assert_eq!(compact, PhysicalSize::new(203, 47));
            assert_eq!(expanded, PhysicalSize::new(347, 216));
            assert_eq!(compact_pos.y, expanded_pos.y);
            let (wide_pos, wide, _) =
                geometry(&m, IslandSize::Medium, IslandForm::Compact, None, 360.);
            assert_eq!(wide.width, 405);
            assert_eq!(wide_pos.x + wide.width as i32 / 2, 1280);
        }
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
                        for cue_width in [148., 180., 308.] {
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
}
