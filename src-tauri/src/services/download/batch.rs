use super::*;

#[derive(Clone, Copy)]
enum BatchAction { Resume, Pause, Cancel }

impl DownloadService {
    /// T1540: Dismiss history without losing offline audio/attachment ownership.
    pub fn clear_completed_tasks(&self) -> AppResult<Vec<DownloadTaskDto>> {
        {
            let _operation = self.operation.lock();
            self.db.with_conn(|conn| {
                conn.execute(
                    "INSERT OR IGNORE INTO download_task_dismissals(task_id,cleared_at)
                     SELECT id,?1 FROM downloads WHERE status='completed'",
                    [Utc::now().to_rfc3339()],
                )?;
                Ok(())
            })?;
        }
        self.emit_tasks_updated();
        self.list_tasks()
    }

    pub fn resume_all_tasks(&self) -> AppResult<Vec<DownloadTaskDto>> { self.control_all_tasks(BatchAction::Resume) }
    pub fn pause_all_tasks(&self) -> AppResult<Vec<DownloadTaskDto>> { self.control_all_tasks(BatchAction::Pause) }
    pub fn cancel_all_tasks(&self) -> AppResult<Vec<DownloadTaskDto>> { self.control_all_tasks(BatchAction::Cancel) }

    fn control_all_tasks(&self, action: BatchAction) -> AppResult<Vec<DownloadTaskDto>> {
        let mut cleanup_error = None;
        {
            // Persist the whole operation before signaling any worker. The same
            // lock protects claims, individual controls and final registration.
            let _operation = self.operation.lock();
            let (filter, update) = match action {
                BatchAction::Resume => ("status IN ('paused','failed')",
                    "UPDATE downloads SET status='queued',next_retry_at=NULL,error_code=NULL,error_message=NULL,updated_at=?1 WHERE status IN ('paused','failed')"),
                BatchAction::Pause => ("status IN ('queued','resolving','downloading','verifying','failed')",
                    "UPDATE downloads SET status='paused',next_retry_at=NULL,updated_at=?1 WHERE status IN ('queued','resolving','downloading','verifying','failed')"),
                BatchAction::Cancel => ("status NOT IN ('completed','canceled')",
                    "UPDATE downloads SET status='canceled',next_retry_at=NULL,downloaded_bytes=0,updated_at=?1 WHERE status NOT IN ('completed','canceled')"),
            };
            let ids: Vec<String> = self.db.with_conn(|conn| {
                let tx = conn.transaction()?;
                let ids = {
                    let mut stmt = tx.prepare(&format!("SELECT id FROM downloads WHERE {filter}"))?;
                    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
                    rows.collect::<Result<Vec<_>,_>>()?
                };
                tx.execute(update, [Utc::now().to_rfc3339()])?;
                tx.commit()?;
                Ok(ids)
            })?;
            if !matches!(action, BatchAction::Resume) {
                let workers = self.cancel_flags.lock();
                for id in &ids {
                    if let Some(flag) = workers.get(id) {
                        // Keep registration until the transfer drops its file
                        // handles; run_registered then cleans canceled parts.
                        flag.store(true, Ordering::SeqCst);
                    } else if matches!(action, BatchAction::Cancel) {
                        if let Err(error) = self.cleanup_canceled_part(id) {
                            if cleanup_error.is_none() { cleanup_error = Some(error); }
                        }
                    }
                }
                let mut speeds = self.speeds.lock();
                for id in &ids { speeds.remove(id); }
            }
        }
        let dispatched = if matches!(action, BatchAction::Resume) { self.dispatch() } else { Ok(()) };
        self.emit_tasks_updated();
        if let Some(error) = cleanup_error { return Err(error); }
        dispatched?;
        self.list_tasks()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::{tempdir,TempDir};
    use wiremock::{Mock,MockServer,ResponseTemplate,matchers::method};

    fn fixture() -> (TempDir,DownloadService) {
        let dir=tempdir().unwrap();
        let db=Database::open_and_migrate(&dir.path().join("data"),&dir.path().join("cache")).unwrap();
        let service=DownloadService::new(db.clone(),AsmrOneProvider::new(db));
        service.set_download_root(&dir.path().join("downloads").to_string_lossy()).unwrap();
        let minute=scheduling::local_minute();
        let policy=DownloadPolicy {concurrency:1,schedule_enabled:true,start_minute:(minute+1)%1440,end_minute:(minute+2)%1440,..Default::default()};
        service.db.set_setting("download_policy_v1",&serde_json::to_string(&policy).unwrap()).unwrap();
        *service.policy.lock()=Some(policy);
        (dir,service)
    }
    fn add(s:&DownloadService,id:&str,status:&str,url:&str) {
        s.db.with_conn(|c| {
            c.execute("INSERT INTO works(id,title,created_at,updated_at) VALUES(?1,?1,'now','now')",[id])?;
            c.execute("INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES(?1,?1,?1,'1','now')",[id])?;
            c.execute("INSERT INTO track_versions(id,track_id,created_at) VALUES(?1,?1,'now')",[id])?;
            c.execute("INSERT INTO media_sources(id,version_id,source_kind,remote_file_id,format,created_at,updated_at) VALUES(?1,?1,'online_provider',?2,'mp3','now','now')",params![id,url])?;
            c.execute("INSERT INTO downloads(id,work_id,version_id,source_id,status,temp_relative_path,target_relative_path,downloaded_bytes,total_bytes,etag,created_at,updated_at) VALUES(?1,?1,?1,?1,?2,?1||'.mp3.part',?1||'.mp3',7,100,'validator','now','now')",params![id,status])?;
            Ok(())
        }).unwrap();
    }
    fn status(s:&DownloadService,id:&str)->String {
        s.list_tasks().unwrap().into_iter().find(|t|t.id==id).unwrap().status
    }
    #[test]
    fn t1540_clear_completed_persists_without_losing_files_sources_or_attachment_bookkeeping() {
        let (dir,s)=fixture();
        for state in ["queued","resolving","downloading","verifying","failed","paused","completed","canceled"] {
            add(&s,state,state,"http://127.0.0.1/unused");
        }
        let audio=s.get_download_root().join("completed.mp3");
        let document=s.get_download_root().join("notes.txt");
        let part=s.get_download_root().join("downloading.mp3.part");
        fs::write(&audio,b"finished audio").unwrap();
        fs::write(&document,b"finished document").unwrap();
        fs::write(&part,b"unfinished").unwrap();
        s.complete_task_in_db("completed","completed","completed","completed.mp3",&audio,14,None,"limited_size_and_header").unwrap();
        s.db.with_conn(|c| {
            c.execute("INSERT INTO attachments(id,work_id,attachment_type,title,relative_path,provider,remote_file_id,created_at) VALUES('doc','completed','text','notes.txt','notes.txt','asmr_one','document','now')",[])?;
            c.execute("INSERT INTO downloads(id,work_id,attachment_id,status,temp_relative_path,target_relative_path,created_at,updated_at) VALUES('doc-task','completed','doc','completed','notes.txt.part','notes.txt','now','now')",[])?;
            Ok(())
        }).unwrap();
        s.complete_task_in_db("doc-task","","","notes.txt",&document,17,None,"limited_size_and_header").unwrap();
        let flag=Arc::new(AtomicBool::new(false));
        s.cancel_flags.lock().insert("downloading".into(),flag.clone());
        s.speeds.lock().insert("downloading".into(),1024);
        let before=s.list_tasks().unwrap();
        let remaining=s.clear_completed_tasks().unwrap();
        assert_eq!(remaining.len(),before.len()-2);
        assert!(remaining.iter().all(|t|t.status!="completed"));
        for task in remaining {
            let original=before.iter().find(|t|t.id==task.id).unwrap();
            assert_eq!(serde_json::to_value(task).unwrap(),serde_json::to_value(original).unwrap());
        }
        s.clear_completed_tasks().unwrap();
        assert!(!flag.load(Ordering::SeqCst));
        assert_eq!(s.cancel_flags.lock().len(),1);
        assert_eq!(fs::read(&audio).unwrap(),b"finished audio");
        assert_eq!(fs::read(&document).unwrap(),b"finished document");
        assert_eq!(fs::read(&part).unwrap(),b"unfinished");
        let reopened=Database::open_and_migrate(&dir.path().join("data"),&dir.path().join("cache")).unwrap();
        let restored=DownloadService::new(reopened.clone(),AsmrOneProvider::new(reopened));
        assert_eq!(restored.list_tasks().unwrap().len(),7,"cleared tasks must stay absent after restart");
        let detail=restored.db.get_work_detail("completed").unwrap();
        assert!(detail.tracks[0].versions[0].sources.iter().any(|source|source.source_kind=="local_downloaded" && source.availability=="available"));
        let preview=restored.preview_download("completed",Some(vec!["doc".into()])).unwrap();
        assert!(preview.items.iter().any(|item|item.attachment_id.as_deref()==Some("doc") && item.already_downloaded));
        // An explicit later file deletion still finds dismissed audio and documents.
        restored.delete_downloaded_files_for_work("completed").unwrap();
        assert!(!audio.exists());assert!(!document.exists());assert!(part.exists());
        restored.db.with_conn(|c| {
            assert_eq!(c.query_row("SELECT COUNT(*) FROM download_task_dismissals",[],|r|r.get::<_,i64>(0))?,0);
            Ok(())
        }).unwrap();
    }
    #[test]
    fn bulk_controls_preserve_files_progress_priorities_and_schedule() {
        let (_dir,s)=fixture();
        let states=["queued","resolving","downloading","verifying","failed","paused","completed","canceled"];
        for state in states {
            add(&s,state,state,"http://127.0.0.1/unused");
            if !["completed","canceled"].contains(&state) {
                let part=ensure_safe_parent_dir(&s.get_download_root(),&format!("{state}.mp3.part")).unwrap();
                fs::write(part,b"partial").unwrap();
            }
        }
        let completed=ensure_safe_parent_dir(&s.get_download_root(),"completed.mp3").unwrap();
        fs::write(&completed,b"completed audio").unwrap();
        s.db.with_conn(|c| {
            c.execute("UPDATE downloads SET error_code='NETWORK_UNAVAILABLE',error_message='retry',next_retry_at='2099-01-01T00:00:00Z',priority=1 WHERE id='failed'",[])?;
            Ok(())
        }).unwrap();
        s.speeds.lock().insert("downloading".into(),1024);
        s.pause_all_tasks().unwrap();
        let reopened=DownloadService::new(s.db.clone(),s.provider.clone());
        for state in states.iter().take(6) {
            assert_eq!(status(&reopened,state),"paused");
            assert_eq!(fs::read(s.get_download_root().join(format!("{state}.mp3.part"))).unwrap(),b"partial");
        }
        assert!(s.speeds.lock().is_empty());
        assert!(s.list_tasks().unwrap().iter().all(|t|t.next_retry_at.is_none()));
        s.resume_all_tasks().unwrap();
        assert!(s.cancel_flags.lock().is_empty(),"resume must respect the closed download schedule");
        for state in states.iter().take(6) {assert_eq!(status(&s,state),"queued");}
        let failed=s.list_tasks().unwrap().into_iter().find(|t|t.id=="failed").unwrap();
        assert_eq!(failed.downloaded_bytes,7);assert_eq!(failed.priority,1);assert!(failed.error_code.is_none());
        let ready=s.claim_ready(Utc::now(),s.get_policy().unwrap().start_minute).unwrap();
        assert_eq!(ready.len(),1);assert_eq!(ready[0].0,"failed");
        // No worker was spawned for this manual claim.
        s.cancel_flags.lock().clear();
        s.cancel_all_tasks().unwrap();
        s.cancel_all_tasks().unwrap();
        for state in states.iter().take(6) {
            assert_eq!(status(&s,state),"canceled");
            assert!(!s.get_download_root().join(format!("{state}.mp3.part")).exists());
        }
        assert_eq!(status(&s,"completed"),"completed");assert_eq!(status(&s,"canceled"),"canceled");
        assert_eq!(fs::read(completed).unwrap(),b"completed audio");
    }
    #[test]
    fn failed_bulk_persistence_never_stops_workers_or_partially_updates_the_queue() {
        let (_dir,s)=fixture();
        add(&s,"a","queued","http://127.0.0.1/unused");add(&s,"b","downloading","http://127.0.0.1/unused");
        let flag=Arc::new(AtomicBool::new(false));s.cancel_flags.lock().insert("b".into(),flag.clone());
        s.db.with_conn(|c| {c.execute_batch("CREATE TRIGGER reject_batch BEFORE UPDATE ON downloads WHEN NEW.status='paused' AND OLD.id='b' BEGIN SELECT RAISE(ABORT,'test persistence failure'); END;")?;Ok(())}).unwrap();
        assert!(s.pause_all_tasks().is_err());
        assert_eq!(status(&s,"a"),"queued");assert_eq!(status(&s,"b"),"downloading");
        assert!(!flag.load(Ordering::SeqCst));
    }
    async fn wait_requests(server:&MockServer,count:usize) {
        tokio::time::timeout(Duration::from_secs(2),async {
            while server.received_requests().await.unwrap().len()<count {tokio::time::sleep(Duration::from_millis(5)).await;}
        }).await.unwrap();
    }
    #[tokio::test]
    async fn bulk_pause_resume_and_cancel_keep_worker_ownership_until_handles_close() {
        let server=MockServer::start().await;
        Mock::given(method("GET")).respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)).set_body_bytes(vec![42;100])).mount(&server).await;
        let (_dir,s)=fixture();
        for id in ["running","waiting"] {
            add(&s,id,"queued",&format!("{}/audio",server.uri()));
            let part=ensure_safe_parent_dir(&s.get_download_root(),&format!("{id}.mp3.part")).unwrap();
            fs::write(part,b"partial").unwrap();
        }
        let clone=s.clone();let worker=tokio::spawn(async move {clone.run_single_task("running").await});
        wait_requests(&server,1).await;
        s.pause_all_tasks().unwrap();s.resume_all_tasks().unwrap();
        assert_eq!(s.cancel_flags.lock().len(),1,"resuming cannot replace the worker while it still owns the file");
        tokio::time::timeout(Duration::from_secs(1),worker).await.unwrap().unwrap().unwrap();
        assert_eq!(status(&s,"running"),"queued");
        assert_eq!(fs::read(s.get_download_root().join("running.mp3.part")).unwrap(),b"partial");
        assert!(s.cancel_flags.lock().is_empty());
        let clone=s.clone();let worker=tokio::spawn(async move {clone.run_single_task("running").await});
        wait_requests(&server,2).await;
        s.cancel_all_tasks().unwrap();
        assert!(s.get_download_root().join("running.mp3.part").exists());
        assert!(!s.get_download_root().join("waiting.mp3.part").exists());
        tokio::time::timeout(Duration::from_secs(1),worker).await.unwrap().unwrap().unwrap();
        assert!(!s.get_download_root().join("running.mp3.part").exists());
        assert!(s.cancel_flags.lock().is_empty());
        assert!(s.list_tasks().unwrap().iter().all(|t|t.status=="canceled"));
    }
}
