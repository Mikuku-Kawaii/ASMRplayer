use super::{catalog, codec, SyncService};
use crate::domain::AppResult;
use futures_util::{stream, StreamExt};
use rusqlite::params;
use std::{collections::BTreeSet, time::Duration};

impl SyncService {
    pub(super) fn complete_source_information(&self) -> AppResult<()> {
        let base = self.source.api_base();
        let completed_key = format!(
            "device_sync_metadata_completed_{}",
            catalog::hash(base.as_bytes())
        );
        let mut completed: BTreeSet<String> = self
            .db
            .get_setting(&completed_key)?
            .and_then(|s| codec::from_str(&s).ok())
            .unwrap_or_default();
        let sources=self.db.with_conn(|c|{
            let mut statement=c.prepare("SELECT work_id,source_work_id FROM work_sources WHERE provider LIKE 'asmr_one%'")?;
            let rows=statement.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?
                .collect::<Result<Vec<_>,_>>()?;
            Ok(rows)
        })?.into_iter().filter(|(_,source)|!completed.contains(source)).collect::<Vec<_>>();
        self.db
            .set_setting("device_sync_metadata_pending", &sources.len().to_string())?;
        let results = tauri::async_runtime::block_on(async {
            stream::iter(sources)
                .map(|(work, source)| async move {
                    let result = tokio::time::timeout(
                        Duration::from_secs(45),
                        self.source
                            .sync_work_from_asmr_one(&work, Some(&source), false),
                    )
                    .await;
                    let error = match result {
                        Ok(Ok(_)) => None,
                        Ok(Err(e)) => Some(e.message),
                        Err(_) => Some("来源请求超时".into()),
                    };
                    (source, error)
                })
                .buffer_unordered(4)
                .collect::<Vec<_>>()
                .await
        });
        let mut errors = Vec::new();
        for (source, error) in results {
            if let Some(error) = error {
                errors.push(error)
            } else {
                completed.insert(source);
            }
        }
        // Persist the source completion separately from the shared user-data journal.
        self.db.with_conn(|c|{
            let tx=c.transaction()?;
            for(key,value) in [
                (completed_key.as_str(),codec::to_string(&completed)?),
                ("device_sync_metadata_pending",errors.len().to_string()),
                ("device_sync_metadata_error",errors.first().cloned().unwrap_or_default()),
            ] {
                tx.execute("INSERT INTO app_settings(key,value_json,updated_at) VALUES(?1,?2,?3) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,updated_at=excluded.updated_at",params![key,value,chrono::Utc::now().to_rfc3339()])?;
            }
            tx.commit()?;Ok(())
        })
    }
}
