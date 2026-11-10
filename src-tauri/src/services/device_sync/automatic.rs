use super::ProviderId;
use super::{codec, fail, SyncService};
use crate::domain::AppResult;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicI64, Ordering};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoPolicy {
    pub foreground: bool,
    pub periodic: bool,
    pub interval_minutes: u32,
}
impl Default for AutoPolicy {
    fn default() -> Self {
        Self {
            foreground: false,
            periodic: false,
            interval_minutes: 30,
        }
    }
}
impl AutoPolicy {
    pub fn validate(&self) -> AppResult<()> {
        if ![15, 30, 60, 180, 360].contains(&self.interval_minutes) {
            return Err(fail("请选择 15、30、60、180 或 360 分钟"));
        }
        Ok(())
    }
}
impl SyncService {
    pub fn auto_policy(&self, id: ProviderId) -> AppResult<AutoPolicy> {
        Ok(self
            .db
            .get_setting(&id.setting("device_sync_auto"))?
            .and_then(|s| codec::from_str(&s).ok())
            .unwrap_or_default())
    }
    pub fn set_auto_policy(
        &self,
        p: AutoPolicy,
        expected: Option<ProviderId>,
    ) -> AppResult<super::Status> {
        let _guard = self
            .run_gate
            .try_lock()
            .ok_or_else(|| fail("同步或授权正在进行"))?;
        let id = self.check_selected(expected)?;
        p.validate()?;
        if p.foreground || p.periodic {
            if !id.provider().get_status(&self.entry(id)?)?.1 {
                return Err(fail("请先绑定当前存储"));
            }
        }
        self.db
            .set_setting(&id.setting("device_sync_auto"), &codec::to_string(&p)?)?;
        self.status()
    }
    pub fn start_automatic(&self) {
        let service = self.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(30));
            service.auto_tick(false);
        });
    }
    pub fn foreground(&self) {
        let service = self.clone();
        std::thread::spawn(move || {
            service.auto_tick(true);
        });
    }
    fn auto_tick(&self, foreground: bool) {
        // A removed provider must not implicitly start uploading to the new default.
        if !self
            .db
            .get_setting("device_sync_provider")
            .ok()
            .flatten()
            .is_some_and(|v| codec::from_str::<ProviderId>(&v).is_ok())
        {
            return;
        }

        let Ok(id) = self.selected() else { return };
        let Ok(policy) = self.auto_policy(id) else {
            return;
        };
        if !(if foreground {
            policy.foreground
        } else {
            policy.periodic
        }) {
            return;
        }
        if !self
            .entry(id)
            .and_then(|e| id.provider().get_status(&e))
            .is_ok_and(|v| v.1)
        {
            return;
        }
        let now = chrono::Utc::now().timestamp();
        let last = self.auto_attempt.load(Ordering::Relaxed);
        let delay = if foreground {
            60
        } else {
            policy.interval_minutes as i64 * 60
        };
        if now - last < delay
            || self
                .auto_attempt
                .compare_exchange(last, now, Ordering::SeqCst, Ordering::Relaxed)
                .is_err()
        {
            return;
        }
        let result = self.run(Some(id));
        let message = if result.is_ok() {
            ""
        } else {
            "自动同步未完成，将在下次触发时重试；可手动同步查看原因"
        };
        let _ = self
            .db
            .set_setting(&id.setting("device_sync_auto_error"), message);
    }
}
pub(super) fn clock() -> AtomicI64 {
    AtomicI64::new(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_sync_is_opt_in_and_intervals_are_bounded() {
        let mut policy = AutoPolicy::default();
        assert!(!policy.foreground && !policy.periodic);
        policy.validate().unwrap();
        policy.interval_minutes = 1;
        assert!(policy.validate().is_err());
        policy.interval_minutes = 360;
        policy.validate().unwrap();
    }
}
