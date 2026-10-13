use crate::domain::{sanitize_smtc_track_title, SubtitleCueDto};
use parking_lot::{Mutex, RwLock};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const DEFAULT_AMLL_PORT: u16 = 23330;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AmllTrackState {
    pub version_id: Option<String>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub cover_path: Option<String>,
    pub duration_ms: i64,
    pub position_ms: i64,
    pub is_playing: bool,
    pub cues: Vec<SubtitleCueDto>,
    pub active_cue_text: Option<String>,
    pub lrc_text: String,
    pub ttml_text: String,
}

static GLOBAL_AMLL_HUB: std::sync::OnceLock<Arc<AmllBroadcastHub>> = std::sync::OnceLock::new();

pub struct AmllBroadcastHub {
    state: RwLock<AmllTrackState>,
    ws_clients: Mutex<Vec<TcpStream>>,
    winisland_was_online: AtomicBool,
    port: u16,
}

impl AmllBroadcastHub {
    pub fn global() -> Arc<Self> {
        GLOBAL_AMLL_HUB
            .get_or_init(|| {
                let port = std::env::var("ASMR_AMLL_PORT")
                    .ok()
                    .and_then(|s| s.parse::<u16>().ok())
                    .unwrap_or(DEFAULT_AMLL_PORT);
                let hub = Arc::new(Self {
                    state: RwLock::new(AmllTrackState::default()),
                    ws_clients: Mutex::new(Vec::new()),
                    winisland_was_online: AtomicBool::new(false),
                    port,
                });
                Self::spawn_listener(hub.clone());
                hub
            })
            .clone()
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn get_state(&self) -> AmllTrackState {
        self.state.read().clone()
    }

    fn spawn_listener(hub: Arc<Self>) {
        let addr = SocketAddr::from(([127, 0, 0, 1], hub.port));
        std::thread::Builder::new()
            .name("amll-local-server".into())
            .spawn(move || {
                let Ok(listener) = TcpListener::bind(addr) else {
                    return;
                };
                for stream in listener.incoming().flatten() {
                    let hub_conn = hub.clone();
                    std::thread::spawn(move || {
                        Self::handle_incoming_connection(hub_conn, stream);
                    });
                }
            })
            .ok();
    }

    fn handle_incoming_connection(hub: Arc<Self>, mut stream: TcpStream) {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));
        let _ = stream.set_nodelay(true);

        let mut buf = [0u8; 4096];
        let Ok(n) = stream.read(&mut buf) else {
            return;
        };
        if n == 0 {
            return;
        }
        let req_str = String::from_utf8_lossy(&buf[..n]);
        let first_line = req_str.lines().next().unwrap_or("");
        let mut parts = first_line.split_whitespace();
        let _method = parts.next().unwrap_or("GET");
        let raw_path = parts.next().unwrap_or("/");

        // Check if this is a WebSocket upgrade request
        let mut ws_key: Option<String> = None;
        let mut is_upgrade = false;
        for line in req_str.lines().skip(1) {
            let line = line.trim();
            if line.is_empty() {
                break;
            }
            if let Some((k, v)) = line.split_once(':') {
                let k_lower = k.trim().to_ascii_lowercase();
                let v_trim = v.trim();
                if k_lower == "upgrade" && v_trim.eq_ignore_ascii_case("websocket") {
                    is_upgrade = true;
                } else if k_lower == "sec-websocket-key" {
                    ws_key = Some(v_trim.to_string());
                }
            }
        }

        if is_upgrade {
            if let Some(key) = ws_key {
                let accept = compute_websocket_accept(&key);
                let handshake = format!(
                    "HTTP/1.1 101 Switching Protocols\r\n\
                     Upgrade: websocket\r\n\
                     Connection: Upgrade\r\n\
                     Sec-WebSocket-Accept: {}\r\n\r\n",
                    accept
                );
                if stream.write_all(handshake.as_bytes()).is_ok() {
                    let _ = stream.set_read_timeout(None);
                    // Send initial AMLL state snapshot to the newly connected WebSocket client
                    let snap = hub.state.read().clone();
                    let init_msgs = build_amll_track_messages(&snap);
                    for msg in init_msgs {
                        let frame = encode_ws_text_frame(&msg);
                        if stream.write_all(&frame).is_err() {
                            return;
                        }
                    }
                    let progress_msg = build_amll_progress_message(&snap);
                    let _ = stream.write_all(&encode_ws_text_frame(&progress_msg));
                    hub.ws_clients.lock().push(stream);
                }
            }
            return;
        }

        // Otherwise handle HTTP requests (AMLL TTML / LRC / search / status endpoints)
        let snap = hub.state.read().clone();
        let path_only = raw_path.split('?').next().unwrap_or("/");

        match path_only {
            "/v1/lyrics/search" | "/search" | "/api/v1/lyrics/search" => {
                let body = if snap.cues.is_empty() {
                    serde_json::json!({ "candidates": [] }).to_string()
                } else {
                    serde_json::json!({
                        "candidates": [
                            {
                                "id": snap.version_id.as_deref().unwrap_or("asmr-active"),
                                "musicNames": [snap.title],
                                "artistNames": [snap.artist],
                                "album": snap.album,
                                "score": 100
                            }
                        ]
                    })
                    .to_string()
                };
                write_http_response(&mut stream, "200 OK", "application/json; charset=utf-8", body.as_bytes());
            }
            "/v1/lyrics/get" | "/get" | "/api/v1/lyrics/get" | "/lyrics.ttml" => {
                if snap.ttml_text.is_empty() {
                    write_http_response(&mut stream, "404 Not Found", "text/plain; charset=utf-8", b"");
                } else {
                    write_http_response(
                        &mut stream,
                        "200 OK",
                        "application/xml; charset=utf-8",
                        snap.ttml_text.as_bytes(),
                    );
                }
            }
            "/lyrics.lrc" | "/lrc" => {
                write_http_response(
                    &mut stream,
                    "200 OK",
                    "text/plain; charset=utf-8",
                    snap.lrc_text.as_bytes(),
                );
            }
            _ => {
                let body = serde_json::json!({
                    "protocol": "AMLL-Local-Bridge",
                    "port": hub.port,
                    "title": snap.title,
                    "artist": snap.artist,
                    "album": snap.album,
                    "durationMs": snap.duration_ms,
                    "positionMs": snap.position_ms,
                    "isPlaying": snap.is_playing,
                    "activeCue": snap.active_cue_text,
                    "cueCount": snap.cues.len(),
                    "lrc": snap.lrc_text,
                })
                .to_string();
                write_http_response(&mut stream, "200 OK", "application/json; charset=utf-8", body.as_bytes());
            }
        }
    }

    pub fn broadcast_ws_message(&self, json_text: &str) {
        let frame = encode_ws_text_frame(json_text);
        let mut clients = self.ws_clients.lock();
        clients.retain_mut(|stream| stream.write_all(&frame).is_ok());
    }

    /// Updates the active track and its matched subtitle cues, writes `.lrc` into `cache_dir/smtc_lrc`,
    /// syncs WinIsland local LRC configuration if present, and broadcasts AMLL `setMusicInfo` + `setLyric`.
    /// Returns `true` if WinIsland's `config.toml` was updated on disk.
    pub fn update_track_and_subtitles(
        &self,
        cache_dir: &Path,
        version_id: Option<String>,
        raw_title: &str,
        artist: &str,
        album: &str,
        cover_path: Option<String>,
        duration_ms: i64,
        position_ms: i64,
        is_playing: bool,
        cues: Vec<SubtitleCueDto>,
    ) -> bool {
        let clean_title = sanitize_smtc_track_title(raw_title);
        let active_cue_text = find_active_cue_at(&cues, position_ms);
        let lrc_text = if cues.is_empty() {
            String::new()
        } else {
            format_cues_as_lrc(&clean_title, artist, album, &cues)
        };
        let ttml_text = if cues.is_empty() {
            String::new()
        } else {
            format_cues_as_ttml(&clean_title, artist, album, &cues)
        };

        let next_state = AmllTrackState {
            version_id,
            title: clean_title.clone(),
            artist: artist.to_string(),
            album: album.to_string(),
            cover_path,
            duration_ms,
            position_ms,
            is_playing,
            cues: cues.clone(),
            active_cue_text,
            lrc_text,
            ttml_text,
        };

        let track_or_cues_changed = {
            let mut guard = self.state.write();
            let changed = guard.version_id != next_state.version_id
                || guard.title != next_state.title
                || guard.artist != next_state.artist
                || guard.cues.len() != next_state.cues.len()
                || guard.lrc_text != next_state.lrc_text;
            *guard = next_state.clone();
            changed
        };

        let mut winisland_config_updated = false;
        let smtc_lrc_dir = cache_dir.join("smtc_lrc");
        if !cues.is_empty() {
            let _ = write_smtc_lrc_files(&smtc_lrc_dir, &clean_title, artist, &next_state.lrc_text);
            winisland_config_updated =
                self.sync_winisland_config(&smtc_lrc_dir, true);
        } else if track_or_cues_changed {
            winisland_config_updated =
                self.sync_winisland_config(&smtc_lrc_dir, false);
        }

        if track_or_cues_changed {
            for msg in build_amll_track_messages(&next_state) {
                self.broadcast_ws_message(&msg);
            }
        }
        let progress_msg = build_amll_progress_message(&next_state);
        self.broadcast_ws_message(&progress_msg);

        winisland_config_updated
    }

    /// Updates playback position and active subtitle cue on playback ticks, broadcasting AMLL `onPlayProgress`.
    pub fn update_progress(&self, position_ms: i64, duration_ms: i64, is_playing: bool) -> Option<String> {
        let (progress_msg, active_cue) = {
            let mut guard = self.state.write();
            guard.position_ms = position_ms.max(0);
            if duration_ms > 0 {
                guard.duration_ms = duration_ms;
            }
            guard.is_playing = is_playing;
            guard.active_cue_text = find_active_cue_at(&guard.cues, guard.position_ms);
            (
                build_amll_progress_message(&guard),
                guard.active_cue_text.clone(),
            )
        };
        self.broadcast_ws_message(&progress_msg);
        active_cue
    }

    pub fn clear_on_stop(&self, cache_dir: &Path) {
        {
            let mut guard = self.state.write();
            *guard = AmllTrackState::default();
        }
        let smtc_lrc_dir = cache_dir.join("smtc_lrc");
        let _ = self.sync_winisland_config(&smtc_lrc_dir, false);
        self.broadcast_ws_message(r#"{"type":"onPaused"}"#);
    }

    /// Syncs `%USERPROFILE%\.winisland\config.toml` when present on the system so WinIsland
    /// automatically loads local `.lrc` subtitles exported by ASMR Player without failing on NetEase.
    fn sync_winisland_config(&self, smtc_lrc_dir: &Path, has_subtitles: bool) -> bool {
        let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) else {
            return false;
        };
        let cfg_path = home.join(".winisland").join("config.toml");
        if !cfg_path.is_file() {
            return false;
        }
        let Ok(content) = std::fs::read_to_string(&cfg_path) else {
            return false;
        };

        let clean_dir = smtc_lrc_dir
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .replace('\\', "\\\\");
        let desired_dir_line = format!("lyrics_local_dir = \"{}\"", clean_dir);

        let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        let mut modified = false;

        // 1. Ensure both "com.asmrplayer.desktop" and "asmr.player.desktop" are in smtc_apps if smtc_apps exists
        if content.contains("com.asmrplayer.desktop") && !content.contains("\"asmr.player.desktop\"") {
            if let Some(idx) = lines.iter().position(|l| l.contains("\"com.asmrplayer.desktop\"")) {
                lines.insert(idx + 1, "    \"asmr.player.desktop\",".to_string());
                modified = true;
            }
        }

        // 2. Ensure lyrics_local_dir is set to our smtc_lrc_dir
        if let Some(dir_idx) = lines
            .iter()
            .position(|l| l.trim_start().starts_with("lyrics_local_dir"))
        {
            if lines[dir_idx].trim() != desired_dir_line {
                lines[dir_idx] = desired_dir_line;
                modified = true;
            }
        } else if let Some(mode_idx) = lines
            .iter()
            .position(|l| l.trim_start().starts_with("lyrics_mode"))
        {
            lines.insert(mode_idx + 1, desired_dir_line);
            modified = true;
        }

        // 3. Switch lyrics_mode to "lrc" when playing a subtitled ASMR track, and restore "online" when not
        if let Some(mode_idx) = lines
            .iter()
            .position(|l| l.trim_start().starts_with("lyrics_mode"))
        {
            let cur_line = lines[mode_idx].trim();
            if has_subtitles {
                if cur_line.contains("\"online\"") {
                    self.winisland_was_online.store(true, Ordering::Relaxed);
                }
                if cur_line != "lyrics_mode = \"lrc\"" {
                    lines[mode_idx] = "lyrics_mode = \"lrc\"".to_string();
                    modified = true;
                }
            } else if self.winisland_was_online.swap(false, Ordering::Relaxed)
                && cur_line == "lyrics_mode = \"lrc\""
            {
                lines[mode_idx] = "lyrics_mode = \"online\"".to_string();
                modified = true;
            }
        }

        if modified {
            let new_content = format!("{}\n", lines.join("\n"));
            let _ = std::fs::write(&cfg_path, new_content.as_bytes());
        }
        modified
    }
}

pub fn find_active_cue_at(cues: &[SubtitleCueDto], pos_ms: i64) -> Option<String> {
    if cues.is_empty() || pos_ms < 0 {
        return None;
    }
    cues.iter()
        .find(|c| pos_ms >= c.start_ms && pos_ms <= c.end_ms)
        .map(|c| c.text.trim().replace('\n', " "))
        .filter(|s| !s.is_empty())
}

pub fn format_lrc_timestamp(ms: i64) -> String {
    let clamped = ms.max(0);
    let total_secs = clamped / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    let hundredths = (clamped % 1000) / 10;
    format!("{:02}:{:02}.{:02}", mins, secs, hundredths)
}

pub fn format_ttml_timestamp(ms: i64) -> String {
    let clamped = ms.max(0);
    let total_secs = clamped / 1000;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    let millis = clamped % 1000;
    if hours > 0 {
        format!("{:02}:{:02}:{:02}.{:03}", hours, mins, secs, millis)
    } else {
        format!("{:02}:{:02}.{:03}", mins, secs, millis)
    }
}

pub fn format_cues_as_lrc(
    title: &str,
    artist: &str,
    album: &str,
    cues: &[SubtitleCueDto],
) -> String {
    let mut out = String::with_capacity(cues.len() * 48 + 128);
    out.push_str(&format!("[ti:{}]\n", title.replace(['\r', '\n', ']'], " ")));
    if !artist.trim().is_empty() {
        out.push_str(&format!("[ar:{}]\n", artist.replace(['\r', '\n', ']'], " ")));
    }
    if !album.trim().is_empty() {
        out.push_str(&format!("[al:{}]\n", album.replace(['\r', '\n', ']'], " ")));
    }
    for (idx, cue) in cues.iter().enumerate() {
        let single_line = cue
            .text
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" / ");
        if single_line.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "[{}]{}\n",
            format_lrc_timestamp(cue.start_ms),
            single_line
        ));
        // Emit a clearing line if there is a gap > 600ms before the next cue
        let next_start = cues.get(idx + 1).map(|n| n.start_ms).unwrap_or(i64::MAX);
        if cue.end_ms > cue.start_ms && next_start.saturating_sub(cue.end_ms) > 600 {
            out.push_str(&format!("[{}]\n", format_lrc_timestamp(cue.end_ms)));
        }
    }
    out
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn format_cues_as_ttml(
    title: &str,
    artist: &str,
    album: &str,
    cues: &[SubtitleCueDto],
) -> String {
    let mut out = String::with_capacity(cues.len() * 120 + 256);
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<tt xmlns=\"http://www.w3.org/ns/ttml\" xmlns:ttm=\"http://www.w3.org/ns/ttml#metadata\" xmlns:amll=\"http://www.example.com/ns/amll\">\n");
    out.push_str("  <head>\n    <metadata>\n");
    out.push_str(&format!("      <ttm:title>{}</ttm:title>\n", escape_xml(title)));
    out.push_str(&format!(
        "      <amll:meta key=\"musicName\" value=\"{}\"/>\n",
        escape_xml(title)
    ));
    if !artist.trim().is_empty() {
        out.push_str(&format!(
            "      <amll:meta key=\"artists\" value=\"{}\"/>\n",
            escape_xml(artist)
        ));
    }
    if !album.trim().is_empty() {
        out.push_str(&format!(
            "      <amll:meta key=\"album\" value=\"{}\"/>\n",
            escape_xml(album)
        ));
    }
    out.push_str("    </metadata>\n  </head>\n  <body>\n    <div>\n");
    for cue in cues {
        let single_line = cue
            .text
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" / ");
        if single_line.is_empty() {
            continue;
        }
        let begin = format_ttml_timestamp(cue.start_ms);
        let end = format_ttml_timestamp(cue.end_ms.max(cue.start_ms + 500));
        let escaped = escape_xml(&single_line);
        out.push_str(&format!(
            "      <p begin=\"{}\" end=\"{}\"><span begin=\"{}\" end=\"{}\">{}</span></p>\n",
            begin, end, begin, end, escaped
        ));
    }
    out.push_str("    </div>\n  </body>\n</tt>\n");
    out
}

fn sanitize_filename_stem(raw: &str) -> String {
    let s: String = raw
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | '\r' | '\n') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = s.trim().trim_matches('.');
    if trimmed.is_empty() {
        "track".to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn write_smtc_lrc_files(
    smtc_lrc_dir: &Path,
    clean_title: &str,
    artist: &str,
    lrc_text: &str,
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(smtc_lrc_dir)?;
    let safe_title = sanitize_filename_stem(clean_title);
    let primary_path = smtc_lrc_dir.join(format!("{}.lrc", safe_title));
    std::fs::write(&primary_path, lrc_text.as_bytes())?;

    if !artist.trim().is_empty() {
        let safe_artist = sanitize_filename_stem(artist);
        let combo_path = smtc_lrc_dir.join(format!("{} - {}.lrc", safe_artist, safe_title));
        let _ = std::fs::write(&combo_path, lrc_text.as_bytes());
    }
    Ok(primary_path)
}

fn build_amll_track_messages(state: &AmllTrackState) -> Vec<String> {
    let music_info = serde_json::json!({
        "type": "setMusicInfo",
        "musicId": state.version_id.as_deref().unwrap_or("asmr-active"),
        "musicName": state.title,
        "albumId": state.album,
        "albumName": state.album,
        "artists": [
            {
                "id": state.artist,
                "name": state.artist
            }
        ],
        "duration": state.duration_ms.max(0)
    })
    .to_string();

    let lines_json: Vec<serde_json::Value> = state
        .cues
        .iter()
        .map(|c| {
            let line_text = c
                .text
                .lines()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" / ");
            serde_json::json!({
                "startTime": c.start_ms.max(0),
                "endTime": c.end_ms.max(c.start_ms + 500),
                "words": [
                    {
                        "startTime": c.start_ms.max(0),
                        "endTime": c.end_ms.max(c.start_ms + 500),
                        "word": line_text
                    }
                ],
                "translatedLyric": "",
                "romanLyric": "",
                "isBG": false,
                "isDuet": false
            })
        })
        .collect();

    let set_lyric = serde_json::json!({
        "type": "setLyric",
        "lines": lines_json,
        "lrc": state.lrc_text,
        "ttml": state.ttml_text
    })
    .to_string();

    let play_state = if state.is_playing {
        r#"{"type":"onResumed"}"#.to_string()
    } else {
        r#"{"type":"onPaused"}"#.to_string()
    };

    vec![music_info, set_lyric, play_state]
}

fn build_amll_progress_message(state: &AmllTrackState) -> String {
    serde_json::json!({
        "type": "onPlayProgress",
        "progress": state.position_ms.max(0),
        "duration": state.duration_ms.max(0),
        "isPlaying": state.is_playing,
        "currentLine": state.active_cue_text
    })
    .to_string()
}

fn write_http_response(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8]) {
    let header = format!(
        "HTTP/1.1 {}\r\n\
         Content-Type: {}\r\n\
         Content-Length: {}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Connection: close\r\n\r\n",
        status,
        content_type,
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
}

fn encode_ws_text_frame(text: &str) -> Vec<u8> {
    let payload = text.as_bytes();
    let len = payload.len();
    let mut frame = Vec::with_capacity(len + 10);
    frame.push(0x81); // FIN + Text opcode
    if len < 126 {
        frame.push(len as u8);
    } else if len <= 65535 {
        frame.push(126);
        frame.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        frame.push(127);
        frame.extend_from_slice(&(len as u64).to_be_bytes());
    }
    frame.extend_from_slice(payload);
    frame
}

/// Computes RFC 6455 `Sec-WebSocket-Accept` = Base64(SHA-1(key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11")).
fn compute_websocket_accept(key: &str) -> String {
    let mut input = Vec::with_capacity(key.len() + 36);
    input.extend_from_slice(key.trim().as_bytes());
    input.extend_from_slice(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    let digest = sha1_digest(&input);
    base64_encode(&digest)
}

fn sha1_digest(data: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xEFCDAB89;
    let mut h2: u32 = 0x98BADCFE;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xC3D2E1F0;

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let mut a = h0;
        let mut b = h1;
        let mut c = h2;
        let mut d = h3;
        let mut e = h4;

        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1u32),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDCu32),
                _ => (b ^ c ^ d, 0xCA62C1D6u32),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut out = [0u8; 20];
    out[0..4].copy_from_slice(&h0.to_be_bytes());
    out[4..8].copy_from_slice(&h1.to_be_bytes());
    out[8..12].copy_from_slice(&h2.to_be_bytes());
    out[12..16].copy_from_slice(&h3.to_be_bytes());
    out[16..20].copy_from_slice(&h4.to_be_bytes());
    out
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((n >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_websocket_accept_rfc6455_vector() {
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        assert_eq!(
            compute_websocket_accept(key),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn test_format_cues_as_lrc_and_ttml() {
        let cues = vec![
            SubtitleCueDto {
                index: 0,
                start_ms: 1250,
                end_ms: 3500,
                text: "あ、来た来た！".into(),
            },
            SubtitleCueDto {
                index: 1,
                start_ms: 5000,
                end_ms: 8200,
                text: "待ってたよ～っ♪".into(),
            },
        ];

        let lrc = format_cues_as_lrc("01トラック", "御子柴泉", "ゼロ距離", &cues);
        assert!(lrc.contains("[ti:01トラック]"));
        assert!(lrc.contains("[ar:御子柴泉]"));
        assert!(lrc.contains("[00:01.25]あ、来た来た！"));
        assert!(lrc.contains("[00:03.50]\n"));
        assert!(lrc.contains("[00:05.00]待ってたよ～っ♪"));

        let ttml = format_cues_as_ttml("01トラック", "御子柴泉", "ゼロ距離", &cues);
        assert!(ttml.contains("<p begin=\"00:01.250\" end=\"00:03.500\">"));
        assert!(ttml.contains("あ、来た来た！"));
        assert_eq!(
            find_active_cue_at(&cues, 2000).as_deref(),
            Some("あ、来た来た！")
        );
        assert_eq!(find_active_cue_at(&cues, 4000), None);
    }
}
