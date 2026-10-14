use crate::{
    db::Database,
    domain::{AppError, AppResult, ErrorCode},
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::{Emitter, LogicalSize, Manager, PhysicalPosition};

const KEY: &str = "desktop_lyrics_v1";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LyricsSettings {
    pub enabled: bool,
    pub locked: bool,
    pub font_size: u32,
    pub font_family: String,
    pub background_enabled: bool,
    pub color: String,
    pub background_opacity: f64,
    pub show_translation: bool,
    pub revision: u64,
}
impl Default for LyricsSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            locked: false,
            font_size: 32,
            font_family: "system".into(),
            background_enabled: true,
            color: "lavender".into(),
            background_opacity: 0.25,
            show_translation: true,
            revision: 0,
        }
    }
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LyricsPatch {
    pub enabled: Option<bool>,
    pub locked: Option<bool>,
    pub font_size: Option<u32>,
    pub font_family: Option<String>,
    pub background_enabled: Option<bool>,
    pub color: Option<String>,
    pub background_opacity: Option<f64>,
    pub show_translation: Option<bool>,
}
impl LyricsSettings {
    fn patched(mut self, p: LyricsPatch) -> AppResult<Self> {
        if let Some(v) = p.enabled {
            self.enabled = v;
        }
        if let Some(v) = p.locked {
            self.locked = v;
        }
        if let Some(v) = p.font_size {
            self.font_size = v;
        }
        if let Some(v) = p.font_family {
            self.font_family = v;
        }
        if let Some(v) = p.background_enabled {
            self.background_enabled = v;
        }
        if let Some(v) = p.color {
            self.color = v;
        }
        if let Some(v) = p.background_opacity {
            self.background_opacity = v;
        }
        if let Some(v) = p.show_translation {
            self.show_translation = v;
        }
        if !["system", "yahei", "dengxian", "simsun", "kaiti", "meiryo"]
            .contains(&self.font_family.as_str())
            || !(20..=64).contains(&self.font_size)
            || !self.background_opacity.is_finite()
            || !(0.0..=0.8).contains(&self.background_opacity)
            || !["lavender", "white", "amber"].contains(&self.color.as_str())
        {
            return Err(error("桌面歌词参数超出范围"));
        }
        self.revision = self.revision.saturating_add(1);
        Ok(self)
    }
}
fn error(e: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::InternalError, format!("桌面歌词：{e}"))
}

fn constrain_position(
    position: (i32, i32),
    min: (i32, i32),
    max: (i32, i32),
    preserve_anchor: bool,
) -> (i32, i32) {
    if preserve_anchor {
        position
    } else {
        (
            position.0.clamp(min.0, max.0),
            position.1.clamp(min.1, max.1),
        )
    }
}
#[derive(Clone)]
pub struct DesktopLyricsService {
    db: Database,
    app: tauri::AppHandle,
    gate: Arc<Mutex<()>>,
    ready: Arc<AtomicBool>,
}
impl DesktopLyricsService {
    pub fn new(db: Database, app: tauri::AppHandle) -> Self {
        Self {
            db,
            app,
            gate: Arc::new(Mutex::new(())),
            ready: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn settings(&self) -> AppResult<LyricsSettings> {
        self.db
            .get_setting(KEY)?
            .map(|s| serde_json::from_str(&s).map_err(error))
            .unwrap_or_else(|| Ok(LyricsSettings::default()))
    }
    pub fn update(&self, patch: LyricsPatch) -> AppResult<LyricsSettings> {
        let _guard = self.gate.lock();
        let before = self.settings()?;
        let next = before.clone().patched(patch)?;
        // Capture the live position before any setting change, especially locking.
        if self.ready.load(Ordering::SeqCst) {
            self.persist_position()?;
        }
        self.apply(&next)?;
        if let Err(e) = self
            .db
            .set_setting(KEY, &serde_json::to_string(&next).map_err(error)?)
        {
            let _ = self.apply(&before);
            return Err(e);
        }
        let _ = self.app.emit("desktop-lyrics:settings", &next);
        Ok(next)
    }
    pub fn ready(&self) -> AppResult<()> {
        let _guard = self.gate.lock();
        if !self.ready.load(Ordering::SeqCst) {
            self.position(&self.settings()?, false)?;
        }
        self.ready.store(true, Ordering::SeqCst);
        self.apply(&self.settings()?)
    }
    fn apply(&self, s: &LyricsSettings) -> AppResult<()> {
        let win = self
            .app
            .get_webview_window("desktop-lyrics")
            .ok_or_else(|| error("窗口尚未创建"))?;
        if !s.enabled {
            win.hide().map_err(error)?;
            self.close_menu()?;
        }
        self.position(s, false)?;
        win.set_ignore_cursor_events(false).map_err(error)?;
        if s.enabled && self.ready.load(Ordering::SeqCst) {
            win.show().map_err(error)?;
        }
        Ok(())
    }
    pub fn save_position(&self) -> AppResult<()> {
        let _guard = self.gate.lock();
        self.persist_position()
    }
    fn persist_position(&self) -> AppResult<()> {
        let win = self
            .app
            .get_webview_window("desktop-lyrics")
            .ok_or_else(|| error("窗口尚未创建"))?;
        let p = win.outer_position().map_err(error)?;
        self.db.set_setting(
            "desktop_lyrics_position",
            &serde_json::to_string(&(p.x, p.y)).map_err(error)?,
        )
    }
    pub fn reset_position(&self) -> AppResult<()> {
        let _guard = self.gate.lock();
        let settings = self.settings()?;
        if settings.locked {
            return Err(error("请先解锁桌面歌词"));
        }
        self.position(&settings, true)?;
        self.db.set_setting("desktop_lyrics_position", "null")
    }
    pub fn close_menu(&self) -> AppResult<()> {
        if let Some(menu) = self.app.get_webview_window("desktop-lyrics-menu") {
            menu.hide().map_err(error)?;
        }
        Ok(())
    }
    pub fn open_menu(&self, x: f64, y: f64) -> AppResult<()> {
        let _guard = self.gate.lock();
        if !x.is_finite() || !y.is_finite() {
            return Err(error("菜单坐标无效"));
        }
        if !self.settings()?.enabled {
            return Ok(());
        }
        let lyrics = self
            .app
            .get_webview_window("desktop-lyrics")
            .ok_or_else(|| error("窗口尚未创建"))?;
        let menu = self
            .app
            .get_webview_window("desktop-lyrics-menu")
            .ok_or_else(|| error("菜单尚未创建"))?;
        let origin = lyrics.outer_position().map_err(error)?;
        let scale = lyrics.scale_factor().map_err(error)?;
        let anchor = (
            origin.x + (x.clamp(0.0, 840.0) * scale) as i32,
            origin.y + (y.clamp(0.0, 288.0) * scale) as i32,
        );
        let monitors = lyrics.available_monitors().map_err(error)?;
        let monitor = monitors
            .into_iter()
            .find(|m| {
                anchor.0 >= m.position().x
                    && anchor.1 >= m.position().y
                    && anchor.0 < m.position().x + m.size().width as i32
                    && anchor.1 < m.position().y + m.size().height as i32
            })
            .or(lyrics.primary_monitor().map_err(error)?)
            .ok_or_else(|| error("没有可用屏幕"))?;
        let area = monitor.work_area();
        let width = (264.0 * monitor.scale_factor()) as u32;
        let height = (370.0 * monitor.scale_factor()) as u32;
        let width = width.min(area.size.width);
        let height = height.min(area.size.height);
        let position = constrain_position(
            anchor,
            (area.position.x, area.position.y),
            (
                area.position.x + (area.size.width - width) as i32,
                area.position.y + (area.size.height - height) as i32,
            ),
            false,
        );
        // This separate surface never resizes or moves the locked lyrics window.
        menu.set_position(PhysicalPosition::new(position.0, position.1))
            .map_err(error)?;
        menu.set_size(tauri::PhysicalSize::new(width, height))
            .map_err(error)?;
        menu.show().map_err(error)?;
        menu.set_focus().map_err(error)
    }
    fn position(&self, s: &LyricsSettings, reset: bool) -> AppResult<()> {
        let win = self
            .app
            .get_webview_window("desktop-lyrics")
            .ok_or_else(|| error("窗口尚未创建"))?;
        let monitors = win.available_monitors().map_err(error)?;
        let saved: Option<(i32, i32)> = if reset {
            None
        } else if self.ready.load(Ordering::SeqCst) {
            let p = win.outer_position().map_err(error)?;
            Some((p.x, p.y))
        } else {
            self.db
                .get_setting("desktop_lyrics_position")?
                .and_then(|x| serde_json::from_str(&x).ok())
        };
        let selected = saved
            .and_then(|(x, y)| {
                monitors.iter().find(|m| {
                    x >= m.position().x
                        && y >= m.position().y
                        && x < m.position().x + m.size().width as i32
                        && y < m.position().y + m.size().height as i32
                })
            })
            .cloned();
        let selected_exists = selected.is_some();
        let mon = selected
            .or(win.primary_monitor().map_err(error)?)
            .ok_or_else(|| error("没有可用屏幕"))?;
        let scale = mon.scale_factor();
        let width = 840_f64
            .min(mon.size().width as f64 / scale - 32.0)
            .max(200.0);
        let height = (s.font_size as f64 * 4.0 + 32.0).min(mon.size().height as f64 / scale - 32.0);
        let w = (width * scale) as i32;
        let h = (height * scale) as i32;
        let min_x = mon.position().x;
        let min_y = mon.position().y;
        let max_x = (min_x + mon.size().width as i32 - w).max(min_x);
        let max_y = (min_y + mon.size().height as i32 - h).max(min_y);
        let (x, y) = saved.unwrap_or((
            min_x + (mon.size().width as i32 - w) / 2,
            max_y - (64.0 * scale) as i32,
        ));
        win.set_size(LogicalSize::new(width, height))
            .map_err(error)?;
        // Locking never moves the anchor, including when changing font size.
        let (x, y) = constrain_position(
            (x, y),
            (min_x, min_y),
            (max_x, max_y),
            s.locked && selected_exists,
        );
        win.set_position(PhysicalPosition::new(x, y)).map_err(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn locked_anchor_survives_larger_font_near_monitor_edge() {
        // The current position remains fixed even when a larger frame exceeds the edge.
        assert_eq!(
            constrain_position((1000, 900), (0, 0), (1080, 792), true),
            (1000, 900)
        );
        // Unlocking or disconnecting that monitor brings the frame back into view.
        assert_eq!(
            constrain_position((1000, 900), (0, 0), (1080, 792), false),
            (1000, 792)
        );
        assert_eq!(
            constrain_position((-1600, 200), (0, 0), (1080, 920), false),
            (0, 200)
        );
    }
    #[test]
    fn older_preferences_gain_defaults_and_background_toggle_keeps_opacity() {
        let saved: LyricsSettings = serde_json::from_str(
            r#"{"enabled":true,"locked":true,"fontSize":40,"backgroundOpacity":0.4}"#,
        )
        .unwrap();
        assert_eq!(saved.font_family, "system");
        assert!(saved.background_enabled);
        let updated = saved
            .patched(LyricsPatch {
                font_family: Some("meiryo".into()),
                background_enabled: Some(false),
                ..Default::default()
            })
            .unwrap();
        assert!(updated.locked && updated.enabled);
        assert_eq!(updated.font_size, 40);
        assert_eq!(updated.background_opacity, 0.4);
        assert!(updated
            .patched(LyricsPatch {
                font_family: Some("unknown".into()),
                ..Default::default()
            })
            .is_err());
    }
    #[test]
    fn patches_preserve_other_preferences_and_validate_before_commit() {
        let a = LyricsSettings::default()
            .patched(LyricsPatch {
                enabled: Some(true),
                ..Default::default()
            })
            .unwrap();
        let b = a
            .patched(LyricsPatch {
                font_size: Some(48),
                ..Default::default()
            })
            .unwrap();
        assert!(b.enabled);
        assert_eq!(b.revision, 2);
        assert_eq!(b.font_size, 48);
        assert!(b
            .clone()
            .patched(LyricsPatch {
                font_size: Some(0),
                ..Default::default()
            })
            .is_err());
        assert!(b
            .clone()
            .patched(LyricsPatch {
                background_opacity: Some(f64::NAN),
                ..Default::default()
            })
            .is_err());
        assert!(b
            .patched(LyricsPatch {
                color: Some("invalid".into()),
                ..Default::default()
            })
            .is_err());
    }
}
