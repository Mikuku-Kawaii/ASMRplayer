use super::*;
use chrono::{DateTime, Local, Timelike};
use serde::{Deserialize, Serialize};
use std::time::Duration;

const POLICY_KEY: &str = "download_policy_v1";
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DownloadPolicy {
    pub concurrency: usize,
    pub speed_limit_kib: u32,
    pub schedule_enabled: bool,
    pub start_minute: u32,
    pub end_minute: u32,
    pub auto_resume_network: bool,
    pub resume_on_startup: bool,
}
impl Default for DownloadPolicy {
    fn default() -> Self {
        Self {
            concurrency: 2,
            speed_limit_kib: 0,
            schedule_enabled: false,
            start_minute: 0,
            end_minute: 420,
            auto_resume_network: false,
            resume_on_startup: false,
        }
    }
}
impl DownloadPolicy {
    fn validate(&self) -> AppResult<()> {
        if !(1..=6).contains(&self.concurrency)
            || self.speed_limit_kib > 102_400
            || (self.speed_limit_kib != 0 && self.speed_limit_kib < 16)
            || self.start_minute >= 1440
            || self.end_minute >= 1440
            || (self.schedule_enabled && self.start_minute == self.end_minute)
        {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "下载策略无效：并发 1–6，限速 0 或 16–102400 KiB/s，定时起止时间不能相同",
            ));
        }
        Ok(())
    }
    pub(super) fn allows(&self, minute: u32) -> bool {
        !self.schedule_enabled
            || if self.start_minute < self.end_minute {
                minute >= self.start_minute && minute < self.end_minute
            } else {
                minute >= self.start_minute || minute < self.end_minute
            }
    }
}
pub(super) fn local_minute() -> u32 {
    let now = Local::now();
    now.hour() * 60 + now.minute()
}
pub(super) fn load_policy(db: &Database) -> AppResult<DownloadPolicy> {
    let policy = db
        .get_setting(POLICY_KEY)?
        .map(|raw| {
            serde_json::from_str::<DownloadPolicy>(&raw).map_err(|_| {
                AppError::new(
                    ErrorCode::InvalidInput,
                    "下载策略损坏，调度已停止；请重新保存策略",
                )
            })
        })
        .transpose()?
        .unwrap_or_default();
    policy.validate()?;
    Ok(policy)
}
pub(super) struct RateBudget {
    tokens: f64,
    updated: Instant,
    limit: u32,
}
impl Default for RateBudget {
    fn default() -> Self {
        Self {
            tokens: 0.,
            updated: Instant::now(),
            limit: 0,
        }
    }
}
impl RateBudget {
    pub(super) fn take(&mut self, bytes: usize, limit: u32, now: Instant) -> bool {
        if limit == 0 {
            self.limit = 0;
            self.updated = now;
            return true;
        }
        if limit != self.limit {
            self.tokens = 0.;
            self.limit = limit;
            self.updated = now;
        }
        let rate = limit as f64 * 1024.;
        self.tokens = (self.tokens + now.duration_since(self.updated).as_secs_f64() * rate)
            .min(rate.max(16384.));
        self.updated = now;
        if self.tokens >= bytes as f64 {
            self.tokens -= bytes as f64;
            true
        } else {
            false
        }
    }
}
impl DownloadService {
    pub fn policy_status(&self) -> AppResult<serde_json::Value> {
        let policy = self.get_policy()?;
        Ok(
            serde_json::json!({"scheduleOpen":policy.allows(local_minute()),"policy":policy,"activeTasks":self.cancel_flags.lock().len()}),
        )
    }
    pub fn get_policy(&self) -> AppResult<DownloadPolicy> {
        self.policy.lock().clone().ok_or_else(|| {
            AppError::new(
                ErrorCode::InvalidInput,
                "下载策略读取失败，调度已停止；请重新保存策略",
            )
        })
    }
    pub fn set_policy(&self, policy: DownloadPolicy) -> AppResult<DownloadPolicy> {
        policy.validate()?;
        {
            let _operation = self.operation.lock();
            self.db.set_setting(
                POLICY_KEY,
                &serde_json::to_string(&policy)
                    .map_err(|e| AppError::new(ErrorCode::InvalidInput, e.to_string()))?,
            )?;
            *self.policy.lock() = Some(policy.clone());
        }
        if let Err(e) = self.dispatch() {
            self.db.append_log("WARN", &e.message);
        }
        self.emit_tasks_updated();
        Ok(policy)
    }
    pub fn set_priority(&self, task_id: &str, priority: i32) -> AppResult<Vec<DownloadTaskDto>> {
        if !(-1..=1).contains(&priority) {
            return Err(AppError::new(ErrorCode::InvalidInput, "下载优先级无效"));
        }
        {
            let _operation = self.operation.lock();
            self.db.with_conn(|c| {
                let n=c.execute("UPDATE downloads SET priority=?1 WHERE id=?2 AND status NOT IN ('completed','canceled')",params![priority,task_id])?;
                if n==0{return Err(AppError::new(ErrorCode::InvalidInput,"任务已结束或不存在"))} Ok(())
            })?;
        }
        if let Err(e) = self.dispatch() {
            self.db.append_log("WARN", &e.message);
        }
        self.emit_tasks_updated();
        self.list_tasks()
    }
    pub fn start_scheduler(&self) {
        let this = self.clone();
        spawn_bg_task(async move {
            let mut last_error = None;
            loop {
                match this.dispatch() {
                    Ok(()) => last_error = None,
                    Err(e) => {
                        if last_error.as_ref() != Some(&e.message) {
                            this.db
                                .append_log("WARN", &format!("下载调度：{}", e.message));
                        }
                        last_error = Some(e.message);
                    }
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
    }
    pub(super) fn claim_ready(
        &self,
        now: DateTime<Utc>,
        minute: u32,
    ) -> AppResult<Vec<(String, Arc<AtomicBool>)>> {
        let _operation = self.operation.lock();
        let policy = self.get_policy()?;
        let mut workers = self.cancel_flags.lock();
        if !policy.allows(minute) {
            for (id, flag) in workers.iter() {
                self.db.with_conn(|c|{c.execute("UPDATE downloads SET status='queued' WHERE id=?1 AND status IN ('resolving','downloading','verifying')",[id])?;Ok(())})?;
                flag.store(true, Ordering::SeqCst);
            }
            return Ok(vec![]);
        }
        let ids:Vec<String>=self.db.with_conn(|c|{
            let tx=c.transaction()?;
            let ids={
                let mut stmt=tx.prepare("SELECT id FROM downloads WHERE status='queued' OR (status='failed' AND next_retry_at IS NOT NULL AND next_retry_at<=?1 AND ?2=1) ORDER BY priority DESC,created_at ASC,rowid ASC")?;
                let rows=stmt.query_map(params![now.to_rfc3339(),policy.auto_resume_network],|r|r.get::<_,String>(0))?;
                rows.collect::<Result<Vec<_>,_>>()?.into_iter().filter(|id|!workers.contains_key(id)).take(policy.concurrency.saturating_sub(workers.len())).collect::<Vec<_>>()
            };
            for id in &ids {tx.execute("UPDATE downloads SET status='queued',next_retry_at=NULL,error_code=NULL,error_message=NULL WHERE id=?1",[id])?;}
            tx.commit()?;Ok(ids)
        })?;
        let mut ready = vec![];
        for id in ids {
            let flag = Arc::new(AtomicBool::new(false));
            workers.insert(id.clone(), flag.clone());
            ready.push((id, flag));
        }
        Ok(ready)
    }
    pub(super) fn dispatch(&self) -> AppResult<()> {
        for (id, flag) in self.claim_ready(Utc::now(), local_minute())? {
            let this = self.clone();
            spawn_bg_task(async move {
                let _ = this.run_registered(&id, flag).await;
            });
        }
        Ok(())
    }
    pub(super) async fn wait_budget(&self, bytes: usize, flag: &AtomicBool) -> AppResult<()> {
        loop {
            if flag.load(Ordering::SeqCst) {
                return Ok(());
            }
            let limit = self.get_policy()?.speed_limit_kib;
            if self.rate.lock().take(bytes, limit, Instant::now()) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    pub(super) async fn cancelled(flag: Arc<AtomicBool>) {
        while !flag.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    pub(super) fn cleanup_canceled_part(&self, task_id: &str) -> AppResult<()> {
        let rel: Option<String> = self.db.with_conn(|c| {
            c.query_row(
                "SELECT temp_relative_path FROM downloads WHERE id=?1 AND status='canceled'",
                [task_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(AppError::from)
        })?;
        if let Some(rel) = rel.filter(|p| p.ends_with(".part")) {
            let path = ensure_safe_parent_dir(&self.get_download_root(), &rel)?;
            if path.is_file() {
                fs::remove_file(path)?;
            }
        }
        Ok(())
    }
}
