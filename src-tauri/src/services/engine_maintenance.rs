use crate::domain::{AppError, AppResult, ErrorCode};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

pub const SETTINGS_KEY: &str = "engine_maintenance";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EngineMaintenanceSettings {
    pub idle_minutes: Option<u32>,
    pub completed_tracks: Option<u32>,
}

impl EngineMaintenanceSettings {
    pub fn validate(&self) -> AppResult<()> {
        if self.idle_minutes.is_some_and(|v| !(1..=1440).contains(&v))
            || self.completed_tracks.is_some_and(|v| !(1..=1000).contains(&v)) {
            return Err(AppError::new(ErrorCode::InvalidInput, "未播放间隔须为 1–1440 分钟，音轨间隔须为 1–1000 首"));
        }
        Ok(())
    }
}

pub struct MaintenanceClock {
    pub settings: EngineMaintenanceSettings,
    idle_since: Option<Instant>,
    completed: u32,
    last_completed_generation: Option<u64>,
}

impl MaintenanceClock {
    pub fn new(settings: EngineMaintenanceSettings) -> Self {
        Self { settings, idle_since: None, completed: 0, last_completed_generation: None }
    }
    pub fn active(&mut self) { self.idle_since = None; }
    pub fn finished(&mut self, generation: u64) {
        if self.last_completed_generation != Some(generation) {
            self.last_completed_generation = Some(generation);
            self.completed = self.completed.saturating_add(1);
        }
    }
    pub fn due(&mut self, now: Instant, inactive: bool, track_gap: bool) -> bool {
        if track_gap {
            return self.settings.completed_tracks.is_some_and(|limit| self.completed >= limit);
        }
        if !inactive { self.active(); return false; }
        let since = *self.idle_since.get_or_insert(now);
        self.settings.idle_minutes.is_some_and(|minutes|
            now.saturating_duration_since(since) >= Duration::from_secs(minutes as u64 * 60))
    }
    pub fn restarted(&mut self, now: Instant) {
        self.completed = 0;
        self.idle_since = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maintenance_is_opt_in_counts_only_distinct_eof_and_never_mid_track() {
        let now = Instant::now();
        let mut clock = MaintenanceClock::new(EngineMaintenanceSettings::default());
        clock.finished(1);
        assert!(!clock.due(now, true, true));
        assert!(!clock.due(now + Duration::from_secs(86400), true, false));
        clock = MaintenanceClock::new(EngineMaintenanceSettings { idle_minutes: Some(2), completed_tracks: Some(2) });
        assert!(!clock.due(now, true, false));
        assert!(!clock.due(now + Duration::from_secs(119), true, false));
        assert!(clock.due(now + Duration::from_secs(120), true, false));
        clock.restarted(now);
        clock.finished(1); clock.finished(1);
        assert!(!clock.due(now, false, true));
        clock.finished(2);
        assert!(!clock.due(now, false, false));
        assert!(clock.due(now, false, true));
        clock.restarted(now);
        assert!(!clock.due(now, true, true));
        clock.due(now, true, false);
        clock.active();
        assert!(!clock.due(now + Duration::from_secs(120), true, false));
    }
    #[test]
    fn maintenance_settings_reject_zero_and_unbounded_intervals() {
        for value in [0, 1441, u32::MAX] {
            assert!(EngineMaintenanceSettings { idle_minutes: Some(value), completed_tracks: None }.validate().is_err());
        }
        assert!(EngineMaintenanceSettings { idle_minutes: None, completed_tracks: Some(1001) }.validate().is_err());
        assert!(EngineMaintenanceSettings { idle_minutes: Some(1), completed_tracks: Some(1) }.validate().is_ok());
    }
}
