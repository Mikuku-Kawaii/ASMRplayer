use super::*;
use crate::services::immersion::{fade_gain, ImmersionSettings, SETTINGS_KEY};

impl PlaybackService {
    pub fn immersion_settings(&self) -> ImmersionSettings {
        self.inner.lock().immersion.clone()
    }
    pub async fn update_immersion_settings(
        &self,
        settings: ImmersionSettings,
    ) -> AppResult<ImmersionSettings> {
        settings.validate()?;
        let _load = self.load_operation.lock().await;
        let previous = self.immersion_settings();
        self.mpv
            .set_immersion(settings.reverse_stereo, settings.gapless_playback)
            .await?;
        if let Err(error) = self.db.set_setting(
            SETTINGS_KEY,
            &serde_json::to_string(&settings)
                .map_err(|_| AppError::new(ErrorCode::InternalError, "沉浸设置无法保存"))?,
        ) {
            let _ = self
                .mpv
                .set_immersion(previous.reverse_stereo, previous.gapless_playback)
                .await;
            return Err(error);
        }
        let cancel_fade = previous.fade_enabled && !settings.fade_enabled;
        let paused = {
            let mut state = self.inner.lock();
            state.immersion = settings.clone();
            if cancel_fade {
                state.transition_serial += 1;
                state.fade_in_pending = false;
            }
            state.user_paused_intent
        };
        if cancel_fade {
            if self.mpv.is_running().await {
                self.mpv.set_pause(paused).await?;
            }
            self.mpv.set_transition_gain(1.0).await?;
        }
        self.emit_snapshot();
        if let Some(app) = self.app_handle.lock().as_ref() {
            let _ = app.emit("immersion:updated", &settings);
        }
        Ok(settings)
    }
    // Every gain write is checked after acquiring the same gate as load/pause.
    // A superseded transition must never change a newer track or pause intent.
    pub(super) async fn ramp_transport(
        &self,
        generation: u64,
        serial: u64,
        to: f64,
        duration_ms: u64,
    ) -> AppResult<bool> {
        let from = self.mpv.transition_gain();
        let mut last = Instant::now();
        let mut elapsed = Duration::ZERO;
        let mut waiting = false;
        loop {
            let progress;
            {
                let _load = self.load_operation.lock().await;
                let (valid, buffering) = {
                    let state = self.inner.lock();
                    (
                        state.session_generation == generation
                            && state.transition_serial == serial
                            && !matches!(state.state.as_str(), "stopped" | "error")
                            && !(to > 0.0 && state.user_paused_intent),
                        state.mpv_buffering,
                    )
                };
                if !valid || *self.shutdown_signal.borrow() {
                    return Ok(false);
                }
                let now = Instant::now();
                if (to <= 0.0 || !buffering) && !waiting {
                    elapsed += now.saturating_duration_since(last);
                }
                last = now;
                waiting=to>0.0&&buffering;
                progress = (elapsed.as_secs_f64() * 1000.0 / duration_ms.max(1) as f64).min(1.0);
                if !waiting {self.mpv.set_transition_gain(fade_gain(from, to, progress)).await?;}
            }
            if progress >= 1.0 {
                return Ok(true);
            }
            tokio::time::sleep(Duration::from_millis(if waiting {100} else {15})).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn player() -> (tempfile::TempDir, PlaybackService) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache"))
            .unwrap();
        let player = PlaybackService::new(db.clone(), AsmrOneProvider::new(db));
        (dir, player)
    }
    #[tokio::test]
    async fn immersion_settings_survive_restart_and_failed_validation_keeps_committed_values() {
        let (dir, player) = player();
        let settings = ImmersionSettings {
            reverse_stereo: true,
            gapless_playback: false,
            fade_enabled: true,
            fade_duration_ms: 150,
        };
        player
            .update_immersion_settings(settings.clone())
            .await
            .unwrap();
        assert!(player
            .update_immersion_settings(ImmersionSettings {
                fade_duration_ms: 0,
                ..settings.clone()
            })
            .await
            .is_err());
        assert_eq!(player.immersion_settings(), settings);
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache"))
            .unwrap();
        let restored = PlaybackService::new(db.clone(), AsmrOneProvider::new(db));
        assert_eq!(restored.immersion_settings(), settings);
        player.shutdown().await;
        restored.shutdown().await;
    }
    #[tokio::test]
    async fn immersion_ramp_cancels_stale_generation_and_serial_without_touching_new_output() {
        let (_dir, player) = player();
        {
            let mut s = player.inner.lock();
            s.state = "playing".into();
            s.user_paused_intent = false;
            s.transition_serial = 1;
        }
        let generation = player.inner.lock().session_generation;
        let pending = player.clone();
        let task = tokio::spawn(async move {
            pending
                .ramp_transport(generation, 1, 0.0, 150)
                .await
                .unwrap()
        });
        tokio::time::sleep(Duration::from_millis(40)).await;
        {
            let _load = player.load_operation.lock().await;
            player.inner.lock().transition_serial += 1;
            player.mpv.set_transition_gain(0.6).await.unwrap();
        }
        assert!(!task.await.unwrap());
        assert_eq!(player.mpv.transition_gain(), 0.6);
        assert!(!player
            .ramp_transport(generation + 1, 2, 0.0, 100)
            .await
            .unwrap());
        assert_eq!(player.mpv.transition_gain(), 0.6);
        player.shutdown().await;
    }
    #[tokio::test]
    async fn immersion_fade_waits_for_buffering_and_disabling_it_honors_pause_intent() {
        let (_dir, player) = player();
        {
            let mut s = player.inner.lock();
            s.state = "buffering".into();
            s.user_paused_intent = false;
            s.mpv_buffering = true;
            s.transition_serial = 1;
        }
        player.mpv.set_transition_gain(0.0).await.unwrap();
        let generation = player.inner.lock().session_generation;
        let pending = player.clone();
        let task = tokio::spawn(async move {
            pending
                .ramp_transport(generation, 1, 1.0, 100)
                .await
                .unwrap()
        });
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert_eq!(player.mpv.transition_gain(), 0.0);
        player.inner.lock().mpv_buffering = false;
        assert!(task.await.unwrap());
        assert_eq!(player.mpv.transition_gain(), 1.0);
        // A setting change during a fade-out must complete the actual pause, not merely its UI state.
        player.mpv.ensure_started().await.unwrap();
        player
            .update_immersion_settings(ImmersionSettings {
                fade_enabled: true,
                ..Default::default()
            })
            .await
            .unwrap();
        {
            let mut s = player.inner.lock();
            s.user_paused_intent = true;
            s.state = "paused".into();
        }
        player.mpv.set_transition_gain(0.4).await.unwrap();
        player
            .update_immersion_settings(ImmersionSettings::default())
            .await
            .unwrap();
        assert_eq!(
            player
                .mpv
                .command(serde_json::json!(["get_property", "pause"]))
                .await
                .unwrap()
                .as_bool(),
            Some(true)
        );
        assert_eq!(player.mpv.transition_gain(), 1.0);
        player.shutdown().await;
    }
    #[tokio::test]
    async fn immersion_engine_applies_gapless_reverse_and_multiplicative_gain_without_output_hardware(
    ) {
        let (_dir, player) = player();
        player.mpv.ensure_started().await.unwrap();
        player
            .update_immersion_settings(ImmersionSettings {
                reverse_stereo: true,
                gapless_playback: false,
                ..Default::default()
            })
            .await
            .unwrap();
        let gapless = player
            .mpv
            .command(serde_json::json!(["get_property", "gapless-audio"]))
            .await
            .unwrap();
        assert!(
            gapless == serde_json::json!(false) || gapless == serde_json::json!("no"),
            "{gapless}"
        );
        player.mpv.set_audio_filters("").await.unwrap();
        let af = player
            .mpv
            .command(serde_json::json!(["get_property", "af"]))
            .await
            .unwrap()
            .to_string();
        assert!(af.contains("c0=c1|c1=c0"));
        player.mpv.set_volume(130.0).await.unwrap();
        player.mpv.set_transition_gain(0.4).await.unwrap();
        assert_eq!(
            player
                .mpv
                .command(serde_json::json!(["get_property", "volume"]))
                .await
                .unwrap()
                .as_f64(),
            Some(52.0)
        );
        player.mpv.set_volume(80.0).await.unwrap();
        assert_eq!(
            player
                .mpv
                .command(serde_json::json!(["get_property", "volume"]))
                .await
                .unwrap()
                .as_f64(),
            Some(32.0)
        );
        player
            .update_immersion_settings(ImmersionSettings::default())
            .await
            .unwrap();
        assert_eq!(
            player
                .mpv
                .command(serde_json::json!(["get_property", "gapless-audio"]))
                .await
                .unwrap()
                .as_str(),
            Some("weak")
        );
        assert!(!player
            .mpv
            .command(serde_json::json!(["get_property", "af"]))
            .await
            .unwrap()
            .to_string()
            .contains("c0=c1|c1=c0"));
        player.shutdown().await;
    }
    #[tokio::test]
    async fn immersion_real_transport_rapid_pause_resume_and_gapless_do_not_leave_silent_or_paused_output(
    ) {
        let (_dir, player) = player();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures/audio/stereo_lr_check.mp3");
        player.db.with_conn(|c|{c.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('iw','合成作品','now','now'); INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES('it','iw','合成音轨','1','now'); INSERT INTO track_versions(id,track_id,created_at) VALUES('iv','it','now');")?;
            c.execute("INSERT INTO media_sources(id,version_id,source_kind,absolute_path,format,availability,created_at,updated_at) VALUES('is','iv','local_imported',?1,'mp3','available','now','now')",[fixture.to_string_lossy().as_ref()])?;Ok(())}).unwrap();
        player.set_volume(0.0).await.unwrap();
        player
            .update_immersion_settings(ImmersionSettings {
                fade_enabled: true,
                fade_duration_ms: 150,
                ..Default::default()
            })
            .await
            .unwrap();
        player.play("iv", None).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if player.inner.lock().file_loaded_for_gen && player.mpv.transition_gain() == 1.0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        let pending = player.clone();
        let pause = tokio::spawn(async move { pending.toggle_pause().await.unwrap() });
        tokio::time::sleep(Duration::from_millis(40)).await;
        player.toggle_pause().await.unwrap();
        pause.await.unwrap();
        assert!(!player.inner.lock().user_paused_intent);
        assert_eq!(player.mpv.transition_gain(), 1.0);
        assert_eq!(
            player
                .mpv
                .command(serde_json::json!(["get_property", "pause"]))
                .await
                .unwrap()
                .as_bool(),
            Some(false)
        );
        player.toggle_pause().await.unwrap();
        assert!(player.inner.lock().user_paused_intent);
        assert_eq!(player.mpv.transition_gain(), 0.0);
        player.toggle_pause().await.unwrap();
        assert_eq!(player.mpv.transition_gain(), 1.0);
        // The natural queue continuation path must not insert a new fade into gapless playback.
        player.inner.lock().current_completed = true;
        player.play_with_transition("iv",None,false,None,None,true).await.unwrap();
        assert!(!player.inner.lock().fade_in_pending);
        assert_eq!(player.mpv.transition_gain(), 1.0);
        player.shutdown().await;
    }
}
