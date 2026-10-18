use crate::{
    db::Database,
    domain::{
        is_valid_normal_window_bounds, resolve_window_restore_placement, MonitorDescriptorDto,
        WindowPlacementDto,
    },
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

const HISTORY_KEY: &str = "window_placement_history_json";
const LEGACY_KEY: &str = "window_placement_json";

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlacementHistory {
    last: Option<WindowPlacementDto>,
    monitors: Vec<WindowPlacementDto>,
}

fn same_monitor(saved: &WindowPlacementDto, monitor: &MonitorDescriptorDto) -> bool {
    match (&saved.monitor_name, &monitor.name) {
        (Some(a), Some(b)) => a == b,
        _ => saved.monitor_x == Some(monitor.x) && saved.monitor_y == Some(monitor.y),
    }
}

fn centered(
    monitor: &MonitorDescriptorDto,
    width: u32,
    height: u32,
    maximized: bool,
) -> WindowPlacementDto {
    let width = width.min(monitor.width);
    let height = height.min(monitor.height);
    WindowPlacementDto {
        x: monitor.x + ((monitor.width - width) / 2) as i32,
        y: monitor.y + ((monitor.height - height) / 2) as i32,
        width,
        height,
        maximized,
        monitor_name: monitor.name.clone(),
        monitor_x: Some(monitor.x),
        monitor_y: Some(monitor.y),
        monitor_width: Some(monitor.width),
        monitor_height: Some(monitor.height),
    }
}

impl PlacementHistory {
    fn record(&mut self, mut placement: WindowPlacementDto, monitor: &MonitorDescriptorDto) {
        let index = self
            .monitors
            .iter()
            .position(|saved| same_monitor(saved, monitor));
        if placement.maximized {
            // Maximized bounds are not the restore rectangle. Keep this screen's
            // last normal rectangle, including its position, instead of recentering it.
            placement = if let Some(index) = index {
                let mut normal = self.monitors[index].clone();
                normal.maximized = false;
                resolve_window_restore_placement(&normal, std::slice::from_ref(monitor))
            } else {
                centered(monitor, 1280, 820, false)
            };
            placement.maximized = true;
        }
        if let Some(index) = index {
            self.monitors[index] = placement.clone();
        } else {
            self.monitors.push(placement.clone());
        }
        self.last = Some(placement);
    }

    fn restore(&self, monitors: &[MonitorDescriptorDto]) -> Option<WindowPlacementDto> {
        let last = self.last.as_ref()?;
        if monitors.is_empty() || monitors.iter().any(|m| same_monitor(last, m)) {
            return Some(resolve_window_restore_placement(last, monitors));
        }
        // Last screen is disconnected. Reuse the available primary screen's own
        // layout without deleting the disconnected screen's saved rectangle.
        let primary = &monitors[0];
        Some(
            if let Some(saved) = self.monitors.iter().find(|p| same_monitor(p, primary)) {
                resolve_window_restore_placement(saved, std::slice::from_ref(primary))
            } else {
                centered(primary, last.width, last.height, last.maximized)
            },
        )
    }
}

struct Runtime {
    history: PlacementHistory,
    mini: bool,
    restoring: bool,
}

pub struct WindowPlacementService {
    db: Database,
    runtime: Mutex<Runtime>,
}

impl WindowPlacementService {
    pub fn new(db: Database) -> Self {
        let history = db
            .get_setting(HISTORY_KEY)
            .ok()
            .flatten()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_else(|| {
                let last = db
                    .get_setting(LEGACY_KEY)
                    .ok()
                    .flatten()
                    .and_then(|raw| serde_json::from_str::<WindowPlacementDto>(&raw).ok());
                PlacementHistory {
                    monitors: last.iter().cloned().collect(),
                    last,
                }
            });
        Self {
            db,
            runtime: Mutex::new(Runtime {
                history,
                mini: false,
                restoring: false,
            }),
        }
    }

    pub fn set_mini_mode(&self, mini: bool) {
        self.runtime.lock().mini = mini;
    }

    pub fn capture(&self, window: &tauri::WebviewWindow) {
        {
            let state = self.runtime.lock();
            if state.mini || state.restoring {
                return;
            }
        }
        if window.is_minimized().unwrap_or(true) {
            return;
        }
        let (Ok(pos), Ok(size), Ok(maximized), Ok(Some(monitor))) = (
            window.outer_position(),
            window.inner_size(),
            window.is_maximized(),
            window.current_monitor(),
        ) else {
            return;
        };
        if !maximized
            && !is_valid_normal_window_bounds(pos.x, pos.y, size.width, size.height, false)
        {
            return;
        }
        let monitor = describe(&monitor);
        let placement = WindowPlacementDto {
            x: pos.x,
            y: pos.y,
            width: size.width,
            height: size.height,
            maximized,
            monitor_name: monitor.name.clone(),
            monitor_x: Some(monitor.x),
            monitor_y: Some(monitor.y),
            monitor_width: Some(monitor.width),
            monitor_height: Some(monitor.height),
        };
        self.remember(placement, &monitor);
    }

    fn remember(&self, placement: WindowPlacementDto, monitor: &MonitorDescriptorDto) {
        if !placement.maximized
            && !is_valid_normal_window_bounds(
                placement.x,
                placement.y,
                placement.width,
                placement.height,
                false,
            )
        {
            return;
        }
        let mut state = self.runtime.lock();
        if state.mini || state.restoring {
            return;
        }
        state.history.record(placement, monitor);
        if let Ok(raw) = serde_json::to_string(&state.history) {
            if let Err(error) = self.db.set_setting(HISTORY_KEY, &raw) {
                self.db
                    .append_log("WARN", &format!("保存窗口布局失败: {error}"));
            }
        }
    }

    /// Returns false only when no saved layout is available.
    pub fn restore(&self, window: &tauri::WebviewWindow) -> bool {
        let mut monitors: Vec<_> = window
            .available_monitors()
            .unwrap_or_default()
            .iter()
            .map(describe)
            .collect();
        if let Ok(Some(primary)) = window.primary_monitor() {
            let primary = describe(&primary);
            if let Some(index) = monitors
                .iter()
                .position(|m| m.name == primary.name && m.x == primary.x && m.y == primary.y)
            {
                monitors.swap(0, index);
            }
        }
        let saved = {
            let mut state = self.runtime.lock();
            let Some(saved) = state.history.restore(&monitors) else {
                return false;
            };
            state.restoring = true;
            saved
        };
        // Never hold runtime across native calls: native move/size events can reenter capture.
        let _ = window.unmaximize();
        let _ = window.set_position(tauri::PhysicalPosition::new(saved.x, saved.y));
        let _ = window.set_size(tauri::PhysicalSize::new(saved.width, saved.height));
        if saved.maximized {
            let _ = window.maximize();
        }
        self.runtime.lock().restoring = false;
        true
    }
}

fn describe(monitor: &tauri::Monitor) -> MonitorDescriptorDto {
    MonitorDescriptorDto {
        name: monitor.name().cloned(),
        x: monitor.position().x,
        y: monitor.position().y,
        width: monitor.size().width,
        height: monitor.size().height,
        scale_factor: monitor.scale_factor(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn monitor(name: &str, x: i32) -> MonitorDescriptorDto {
        MonitorDescriptorDto {
            name: Some(name.into()),
            x,
            y: 0,
            width: 2560,
            height: 1440,
            scale_factor: 1.0,
        }
    }
    fn rect(
        m: &MonitorDescriptorDto,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> WindowPlacementDto {
        let mut p = centered(m, width, height, false);
        p.x = m.x + x;
        p.y = y;
        p
    }
    fn database() -> (tempfile::TempDir, Database) {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache"))
            .unwrap();
        (dir, db)
    }

    #[test]
    fn window_placement_restores_last_screen_and_keeps_each_screen_rectangle() {
        let (_dir, db) = database();
        let service = WindowPlacementService::new(db.clone());
        let a = monitor("A", 0);
        let b = monitor("B", -2560);
        let first = rect(&a, 100, 80, 1000, 700);
        let second = rect(&b, 220, 110, 1500, 900);
        service.remember(first.clone(), &a);
        service.remember(second.clone(), &b);
        let reopened = WindowPlacementService::new(db.clone());
        assert_eq!(
            reopened
                .runtime
                .lock()
                .history
                .restore(&[a.clone(), b.clone()]),
            Some(second.clone())
        );
        assert_eq!(reopened.runtime.lock().history.monitors.len(), 2);
        reopened.remember(first.clone(), &a);
        let reopened = WindowPlacementService::new(db);
        assert_eq!(
            reopened.runtime.lock().history.restore(&[a, b]),
            Some(first)
        );
        assert_eq!(reopened.runtime.lock().history.monitors[1], second);
    }

    #[test]
    fn window_placement_maximized_keeps_normal_position_and_ignores_mini_or_minimized() {
        let (_dir, db) = database();
        let service = WindowPlacementService::new(db.clone());
        let m = monitor("A", 0);
        let normal = rect(&m, 0, 70, 1000, 700);
        service.remember(normal.clone(), &m);
        let mut maximized = rect(&m, -8, -8, 2576, 1456);
        maximized.maximized = true;
        service.remember(maximized, &m);
        let mut expected = normal.clone();
        expected.maximized = true;
        assert_eq!(
            service
                .runtime
                .lock()
                .history
                .restore(std::slice::from_ref(&m)),
            Some(expected.clone())
        );
        service.remember(rect(&m, -32000, -32000, 1000, 700), &m);
        service.set_mini_mode(true);
        // Even a mini window on a high-DPI screen must not overwrite normal bounds.
        service.remember(rect(&m, 30, 30, 1020, 630), &m);
        service.set_mini_mode(false);
        service.runtime.lock().restoring = true;
        service.remember(rect(&m, 300, 150, 1200, 800), &m);
        assert_eq!(
            WindowPlacementService::new(db).runtime.lock().history.last,
            Some(expected)
        );
    }

    #[test]
    fn window_placement_falls_back_to_primary_saved_layout_without_forgetting_secondary() {
        let mut history = PlacementHistory::default();
        let a = monitor("A", 0);
        let b = monitor("B", 2560);
        let first = rect(&a, 50, 60, 1100, 750);
        let second = rect(&b, 300, 100, 1400, 900);
        history.record(first.clone(), &a);
        history.record(second.clone(), &b);
        assert_eq!(history.restore(std::slice::from_ref(&a)), Some(first));
        assert_eq!(history.restore(&[a, b]), Some(second));
        assert_eq!(history.monitors.len(), 2);
    }

    #[test]
    fn window_placement_tracks_monitor_identity_after_virtual_desktop_rearrangement() {
        let mut history = PlacementHistory::default();
        let b = monitor("B", 2560);
        history.record(rect(&b, 200, 120, 1400, 900), &b);
        let b_moved = monitor("B", -2560);
        let restored = history
            .restore(&[monitor("A", 2560), b_moved.clone()])
            .unwrap();
        assert_eq!(restored, rect(&b_moved, 200, 120, 1400, 900));
    }

    #[test]
    fn window_placement_migrates_legacy_single_screen_when_next_saved() {
        let (_dir, db) = database();
        let m = monitor("A", 0);
        let original = rect(&m, 150, 90, 1300, 850);
        db.set_setting(LEGACY_KEY, &serde_json::to_string(&original).unwrap())
            .unwrap();
        let service = WindowPlacementService::new(db.clone());
        assert_eq!(
            service
                .runtime
                .lock()
                .history
                .restore(std::slice::from_ref(&m)),
            Some(original.clone())
        );
        service.remember(original.clone(), &m);
        assert!(db.get_setting(HISTORY_KEY).unwrap().is_some());
        assert_eq!(
            WindowPlacementService::new(db).runtime.lock().history.last,
            Some(original)
        );
    }
}
