use crate::domain::{sanitize_smtc_track_title, SmtcDisplayStateDto};
use parking_lot::Mutex;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
struct CachedSmtcMetadata {
    title: String,
    artist: String,
    album_title: String,
    album_artist: String,
    cover_path: Option<String>,
    cover_bytes_hash: Option<u64>,
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

#[cfg(target_os = "windows")]
mod imp {
    use super::*;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use windows::core::{factory, HSTRING};
    use windows::Foundation::{TimeSpan, TypedEventHandler, Uri};
    use windows::Media::{
        MediaPlaybackStatus, MediaPlaybackType, PlaybackPositionChangeRequestedEventArgs,
        SystemMediaTransportControls, SystemMediaTransportControlsButton,
        SystemMediaTransportControlsButtonPressedEventArgs,
        SystemMediaTransportControlsDisplayUpdater,
        SystemMediaTransportControlsTimelineProperties,
    };
    use windows::Storage::StorageFile;
    use windows::Storage::Streams::{
        DataWriter, InMemoryRandomAccessStream, RandomAccessStreamReference,
    };
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::WinRT::ISystemMediaTransportControlsInterop;

    pub struct NativeSmtcController {
        smtc: SystemMediaTransportControls,
        is_playback_position_change_enabled: Arc<AtomicBool>,
        metadata_generation: Arc<AtomicU64>,
        last_metadata: Mutex<Option<CachedSmtcMetadata>>,
    }

    impl NativeSmtcController {
        /// Binds `SystemMediaTransportControls` to a Win32 window handle (`HWND`) via
        /// `ISystemMediaTransportControlsInterop::GetForWindow(hwnd)`, enables transport
        /// and seek permissions, and subscribes to `ButtonPressed` and `PlaybackPositionChangeRequested`.
        pub fn bind_for_window<F>(hwnd_raw: isize, on_action: F) -> Result<Self, String>
        where
            F: Fn(String, Option<i64>) + Send + Sync + 'static,
        {
            if hwnd_raw == 0 {
                return Err("Invalid HWND (null)".into());
            }

            let interop =
                factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>()
                    .map_err(|e| format!("Failed to get ISystemMediaTransportControlsInterop: {e}"))?;

            let hwnd = HWND(hwnd_raw as *mut core::ffi::c_void);
            let smtc: SystemMediaTransportControls = unsafe { interop.GetForWindow(hwnd) }
                .map_err(|e| format!("GetForWindow failed for HWND {hwnd_raw}: {e}"))?;

            let is_playback_position_change_enabled = Arc::new(AtomicBool::new(true));
            let metadata_generation = Arc::new(AtomicU64::new(0));

            // Task 1: Explicitly enable Seek interaction permission & basic transport controls
            let _ = smtc.SetIsEnabled(true);
            let _ = smtc.SetIsPlayEnabled(true);
            let _ = smtc.SetIsPauseEnabled(true);
            let _ = smtc.SetIsStopEnabled(true);
            let _ = smtc.SetIsNextEnabled(true);
            let _ = smtc.SetIsPreviousEnabled(true);
            let _ = smtc.SetIsFastForwardEnabled(true);
            let _ = smtc.SetIsRewindEnabled(true);
            let _ = smtc.SetPlaybackRate(1.0);

            let cb = Arc::new(on_action);

            // Subscribe to hardware / OS transport buttons
            let cb_btn = cb.clone();
            let _ = smtc.ButtonPressed(&TypedEventHandler::<
                SystemMediaTransportControls,
                SystemMediaTransportControlsButtonPressedEventArgs,
            >::new(move |_sender, args| {
                if let Some(args) = args.as_ref() {
                    if let Ok(button) = args.Button() {
                        let mapped = match button {
                            SystemMediaTransportControlsButton::Play => Some("play"),
                            SystemMediaTransportControlsButton::Pause => Some("pause"),
                            SystemMediaTransportControlsButton::Stop => Some("stop"),
                            SystemMediaTransportControlsButton::Next => Some("next"),
                            SystemMediaTransportControlsButton::Previous => Some("previous"),
                            SystemMediaTransportControlsButton::FastForward => Some("fast_forward"),
                            SystemMediaTransportControlsButton::Rewind => Some("rewind"),
                            _ => None,
                        };
                        if let Some(action) = mapped {
                            cb_btn(action.to_string(), None);
                        }
                    }
                }
                Ok(())
            }));

            // Task 3: Subscribe to external Seek requests (Windows Lock Screen, WinIsland, etc.)
            let cb_seek = cb;
            let seek_enabled_flag = is_playback_position_change_enabled.clone();
            let _ = smtc.PlaybackPositionChangeRequested(&TypedEventHandler::<
                SystemMediaTransportControls,
                PlaybackPositionChangeRequestedEventArgs,
            >::new(move |_sender, args| {
                if !seek_enabled_flag.load(Ordering::Relaxed) {
                    return Ok(());
                }
                if let Some(args) = args.as_ref() {
                    if let Ok(ts) = args.RequestedPlaybackPosition() {
                        // Convert 100-ns WinRT TimeSpan ticks to milliseconds
                        let requested_ms = (ts.Duration / 10_000).max(0);
                        cb_seek("seek".to_string(), Some(requested_ms));
                    }
                }
                Ok(())
            }));

            Ok(Self {
                smtc,
                is_playback_position_change_enabled,
                metadata_generation,
                last_metadata: Mutex::new(None),
            })
        }

        pub fn set_is_playback_position_change_enabled(&self, enabled: bool) {
            self.is_playback_position_change_enabled
                .store(enabled, Ordering::Relaxed);
            let _ = self.smtc.SetIsFastForwardEnabled(enabled);
            let _ = self.smtc.SetIsRewindEnabled(enabled);
        }

        /// Task 2: Broadcasts two-way timeline properties (`StartTime`, `MinSeekTime`, `EndTime`, `MaxSeekTime`, `Position`)
        /// to Windows SMTC (`UpdateTimelineProperties`).
        pub fn sync_timeline(&self, start_ms: i64, end_ms: i64, position_ms: i64) {
            let clamped_start = start_ms.max(0);
            let clamped_end = end_ms.max(clamped_start);
            let clamped_pos = if clamped_end > 0 {
                position_ms.clamp(clamped_start, clamped_end)
            } else {
                position_ms.max(0)
            };

            if let Ok(timeline) = SystemMediaTransportControlsTimelineProperties::new() {
                let start_ts = TimeSpan {
                    Duration: clamped_start.saturating_mul(10_000),
                };
                let end_ts = TimeSpan {
                    Duration: clamped_end.saturating_mul(10_000),
                };
                let pos_ts = TimeSpan {
                    Duration: clamped_pos.saturating_mul(10_000),
                };
                let seek_enabled = self
                    .is_playback_position_change_enabled
                    .load(Ordering::Relaxed);
                let _ = timeline.SetStartTime(start_ts);
                let _ = timeline.SetMinSeekTime(start_ts);
                let _ = timeline.SetEndTime(end_ts);
                let _ = timeline.SetMaxSeekTime(if seek_enabled { end_ts } else { start_ts });
                let _ = timeline.SetPosition(pos_ts);
                let _ = self.smtc.UpdateTimelineProperties(&timeline);
            }
        }

        /// Synchronizes full SMTC state (`DisplayUpdater`, `PlaybackStatus`, control flags, and `TimelineProperties`).
        /// - On track switch, updates metadata & cover first (`update_smtc_metadata`), then `PlaybackStatus` and `TimelineProperties`.
        /// - When `playback_status == "Paused"`, strictly preserves `Position` and metadata without ever calling `DisplayUpdater.ClearAll()`.
        pub fn sync_state(&self, state: &SmtcDisplayStateDto) {
            if !state.enabled || state.mode == "disabled" {
                if state.clear_all_called {
                    self.clear_display_updater();
                }
                let _ = self.smtc.SetPlaybackStatus(MediaPlaybackStatus::Closed);
                let _ = self.smtc.SetIsEnabled(false);
                return;
            }

            let _ = self.smtc.SetIsEnabled(true);
            let _ = self.smtc.SetIsPlayEnabled(state.is_play_enabled);
            let _ = self.smtc.SetIsPauseEnabled(state.is_pause_enabled);
            let _ = self.smtc.SetIsStopEnabled(state.is_stop_enabled);
            let _ = self.smtc.SetIsNextEnabled(state.is_next_enabled);
            let _ = self.smtc.SetIsPreviousEnabled(state.is_previous_enabled);
            self.set_is_playback_position_change_enabled(state.is_playback_position_change_enabled);

            match state.playback_status.as_str() {
                "Playing" => {
                    // Section I.3: Set new metadata & cover first, then update PlaybackStatus and TimelineProperties
                    self.update_smtc_metadata(state);
                    let _ = self.smtc.SetPlaybackRate(1.0);
                    let _ = self.smtc.SetPlaybackStatus(MediaPlaybackStatus::Playing);
                    self.sync_timeline(state.start_time_ms, state.end_time_ms, state.position_ms);
                }
                "Paused" => {
                    // Task 4 & Section III.3 Critical Guardrail:
                    // 1. NEVER call DisplayUpdater.ClearAll() on Pause.
                    // 2. Keep metadata & cover up to date if changed.
                    // 3. Preserve the current track's last playback timestamp (`state.position_ms`),
                    //    NEVER resetting Position to 0 or clearing the timeline.
                    self.update_smtc_metadata(state);
                    let _ = self.smtc.SetPlaybackStatus(MediaPlaybackStatus::Paused);
                    self.sync_timeline(state.start_time_ms, state.end_time_ms, state.position_ms);
                }
                "Stopped" | "Closed" | _ => {
                    if state.clear_all_called {
                        self.clear_display_updater();
                    }
                    let _ = self.smtc.SetPlaybackStatus(MediaPlaybackStatus::Stopped);
                }
            }
        }

        fn clear_display_updater(&self) {
            self.metadata_generation.fetch_add(1, Ordering::SeqCst);
            if let Ok(updater) = self.smtc.DisplayUpdater() {
                let _ = updater.ClearAll();
                let _ = updater.Update();
            }
            *self.last_metadata.lock() = None;
        }

        /// Briefly pulses `MusicProperties.Title` to a non-empty placeholder (`"..."`) and clears `last_metadata`
        /// so external SMTC lyric consumers (`WinIsland`, etc.) detect `has_track_changed == true` on the next
        /// `sync_state` tick and reload newly written `.lrc` subtitles.
        pub fn pulse_title_for_lyrics_reload(&self, state: &SmtcDisplayStateDto) {
            if !state.enabled || state.mode == "disabled" || state.title.trim().is_empty() {
                return;
            }
            if let Ok(updater) = self.smtc.DisplayUpdater() {
                let _ = updater.SetType(MediaPlaybackType::Music);
                if let Ok(music) = updater.MusicProperties() {
                    let _ = music.SetTitle(&HSTRING::from("..."));
                    let _ = music.SetArtist(&HSTRING::from(&state.artist));
                }
                let _ = updater.Update();
            }
            *self.last_metadata.lock() = None;
        }

        /// Writes media metadata (`MusicProperties`) and cover thumbnail (`RandomAccessStreamReference`)
        /// into `smtc.DisplayUpdater` and commits `DisplayUpdater.Update()`.
        pub fn update_smtc_metadata(&self, state: &SmtcDisplayStateDto) {
            let clean_title = sanitize_smtc_track_title(&state.title);
            let cover_bytes_hash = state.cover_bytes.as_deref().map(hash_bytes);
            let candidate = CachedSmtcMetadata {
                title: clean_title,
                artist: state.artist.clone(),
                album_title: state.album_title.clone(),
                album_artist: state.album_artist.clone(),
                cover_path: state.cover_path.clone(),
                cover_bytes_hash,
            };

            {
                let guard = self.last_metadata.lock();
                if guard.as_ref() == Some(&candidate) {
                    return;
                }
            }

            let my_gen = self.metadata_generation.fetch_add(1, Ordering::SeqCst) + 1;

            if let Ok(updater) = self.smtc.DisplayUpdater() {
                // Always keep Music mode so Windows SMTC and WinIsland preserve Title + Artist + AlbumTitle
                // and match exported LRC subtitles by song title & artist.
                let _ = updater.SetType(MediaPlaybackType::Music);
                if let Ok(music) = updater.MusicProperties() {
                    let _ = music.SetTitle(&HSTRING::from(&candidate.title));
                    let _ = music.SetArtist(&HSTRING::from(&candidate.artist));
                    let _ = music.SetAlbumTitle(&HSTRING::from(&candidate.album_title));
                    let _ = music.SetAlbumArtist(&HSTRING::from(&candidate.album_artist));
                }

                // Section I.2: Cover Thumbnail construction via RandomAccessStreamReference
                let thumbnail_applied = self.apply_cover_thumbnail(
                    &updater,
                    candidate.cover_path.as_deref(),
                    state.cover_bytes.as_deref(),
                    my_gen,
                );

                // Only cache candidate if we didn't fail to apply a cover that was supposed to be present
                if thumbnail_applied || (candidate.cover_path.is_none() && state.cover_bytes.is_none()) {
                    *self.last_metadata.lock() = Some(candidate);
                }
            }
        }

        fn apply_bytes_stream_thumbnail(
            &self,
            updater: &SystemMediaTransportControlsDisplayUpdater,
            bytes: &[u8],
            my_gen: u64,
        ) -> bool {
            if bytes.is_empty() {
                return false;
            }
            let Ok(stream) = InMemoryRandomAccessStream::new() else {
                return false;
            };
            let Ok(writer) = DataWriter::CreateDataWriter(&stream) else {
                return false;
            };
            if writer.WriteBytes(bytes).is_err() {
                return false;
            }
            let Ok(store_op) = writer.StoreAsync() else {
                return false;
            };
            let updater_clone = updater.clone();
            let gen_ref = self.metadata_generation.clone();
            let (tx, rx) = std::sync::mpsc::sync_channel::<bool>(1);
            let _ = store_op.when(move |res| {
                if res.is_err() {
                    let _ = tx.send(false);
                    return;
                }
                let _ = writer.DetachStream();
                let _ = stream.Seek(0);
                if gen_ref.load(Ordering::SeqCst) != my_gen {
                    let _ = tx.send(false);
                    return;
                }
                let ok = if let Ok(stream_ref) = RandomAccessStreamReference::CreateFromStream(&stream) {
                    let set_ok = updater_clone.SetThumbnail(&stream_ref).is_ok();
                    let _ = updater_clone.Update();
                    set_ok
                } else {
                    false
                };
                let _ = tx.send(ok);
            });
            match rx.recv_timeout(std::time::Duration::from_millis(150)) {
                Ok(applied) => applied,
                Err(_) => {
                    let _ = updater.Update();
                    true
                }
            }
        }

        fn decode_base64_data_url(input: &str) -> Option<Vec<u8>> {
            let trimmed = input.trim();
            if !trimmed.starts_with("data:") {
                return None;
            }
            let (_, b64_part) = trimmed.split_once(',')?;
            let clean: Vec<u8> = b64_part
                .bytes()
                .filter(|b| !b.is_ascii_whitespace())
                .collect();
            if clean.is_empty() {
                return None;
            }
            fn val(c: u8) -> Option<u8> {
                match c {
                    b'A'..=b'Z' => Some(c - b'A'),
                    b'a'..=b'z' => Some(c - b'a' + 26),
                    b'0'..=b'9' => Some(c - b'0' + 52),
                    b'+' | b'-' => Some(62),
                    b'/' | b'_' => Some(63),
                    _ => None,
                }
            }
            let mut out = Vec::with_capacity(clean.len() * 3 / 4);
            for chunk in clean.chunks(4) {
                if chunk.len() < 2 {
                    break;
                }
                let b0 = val(chunk[0])?;
                let b1 = val(chunk[1])?;
                out.push((b0 << 2) | (b1 >> 4));
                if chunk.len() > 2 && chunk[2] != b'=' {
                    let b2 = val(chunk[2])?;
                    out.push((b1 << 4) | (b2 >> 2));
                    if chunk.len() > 3 && chunk[3] != b'=' {
                        let b3 = val(chunk[3])?;
                        out.push((b2 << 6) | b3);
                    }
                }
            }
            Some(out)
        }

        fn apply_cover_thumbnail(
            &self,
            updater: &SystemMediaTransportControlsDisplayUpdater,
            cover_path: Option<&str>,
            cover_bytes: Option<&[u8]>,
            my_gen: u64,
        ) -> bool {
            if let Some(raw_cover) = cover_path {
                let trimmed = raw_cover.trim();

                // 1. Handle data:image/...;base64,... URLs by decoding into an in-memory stream
                if trimmed.starts_with("data:") {
                    if let Some(decoded) = Self::decode_base64_data_url(trimmed) {
                        if self.apply_bytes_stream_thumbnail(updater, &decoded, my_gen) {
                            return true;
                        }
                    }
                }

                // 2. Local file on disk:
                //    First try StorageFile::GetFileFromPathAsync -> RandomAccessStreamReference::CreateFromFile
                //    (awaited up to 250ms so DisplayUpdater.Update() has Thumbnail ready before SetPlaybackStatus(Playing)),
                //    with fallback to InMemoryRandomAccessStream from std::fs::read.
                let norm_path = trimmed.trim_start_matches(r"\\?\");
                let path_obj = Path::new(norm_path);
                if path_obj.is_file() {
                    let winrt_path = std::fs::canonicalize(path_obj)
                        .map(|p| {
                            p.to_string_lossy()
                                .trim_start_matches(r"\\?\")
                                .replace('/', "\\")
                        })
                        .unwrap_or_else(|_| norm_path.replace('/', "\\"));

                    if let Ok(async_op) =
                        StorageFile::GetFileFromPathAsync(&HSTRING::from(&winrt_path))
                    {
                        let updater_clone = updater.clone();
                        let gen_ref = self.metadata_generation.clone();
                        let (tx, rx) = std::sync::mpsc::sync_channel::<bool>(1);
                        let _ = async_op.when(move |res| {
                            if gen_ref.load(Ordering::SeqCst) != my_gen {
                                let _ = tx.send(false);
                                return;
                            }
                            let ok = if let Ok(file) = res {
                                if let Ok(stream_ref) =
                                    RandomAccessStreamReference::CreateFromFile(&file)
                                {
                                    let set_ok = updater_clone.SetThumbnail(&stream_ref).is_ok();
                                    let _ = updater_clone.Update();
                                    set_ok
                                } else {
                                    false
                                }
                            } else {
                                false
                            };
                            let _ = tx.send(ok);
                        });

                        if let Ok(true) = rx.recv_timeout(std::time::Duration::from_millis(250)) {
                            return true;
                        }
                    }

                    if let Ok(file_bytes) = std::fs::read(path_obj) {
                        if self.apply_bytes_stream_thumbnail(updater, &file_bytes, my_gen) {
                            return true;
                        }
                    }
                }

                // 3. Network URL: RandomAccessStreamReference::CreateFromUri(new Uri(coverUrl))
                if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                    let mut ok = false;
                    if let Ok(uri) = Uri::CreateUri(&HSTRING::from(trimmed)) {
                        if let Ok(stream_ref) = RandomAccessStreamReference::CreateFromUri(&uri) {
                            ok = updater.SetThumbnail(&stream_ref).is_ok();
                        }
                    }
                    let _ = updater.Update();
                    return ok;
                }
            }

            // 4. In-memory / byte stream (e.g. embedded ID3 APIC / FLAC Picture):
            if let Some(bytes) = cover_bytes {
                if !bytes.is_empty() && self.apply_bytes_stream_thumbnail(updater, bytes, my_gen) {
                    return true;
                }
            }

            let _ = updater.SetThumbnail(None);
            let _ = updater.Update();
            false
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    use super::*;

    pub struct NativeSmtcController;

    impl NativeSmtcController {
        pub fn bind_for_window<F>(_hwnd_raw: isize, _on_action: F) -> Result<Self, String>
        where
            F: Fn(String, Option<i64>) + Send + Sync + 'static,
        {
            Ok(Self)
        }

        pub fn sync_timeline(&self, _start_ms: i64, _end_ms: i64, _position_ms: i64) {}

        pub fn sync_state(&self, _state: &SmtcDisplayStateDto) {}

        pub fn pulse_title_for_lyrics_reload(&self, _state: &SmtcDisplayStateDto) {}

        pub fn update_smtc_metadata(&self, _state: &SmtcDisplayStateDto) {}
    }
}

pub use imp::NativeSmtcController;
