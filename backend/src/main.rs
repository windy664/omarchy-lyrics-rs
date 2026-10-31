// lyricsd — Rust 后端：MPRIS 轮询 → 位置外推 → 歌词抓取 → stdout 行式 JSON 事件。
// 架构与协议见 docs/04。故意用 500ms 轮询而非 DBus 信号订阅：鲁棒优先，
// 原插件同样是 500ms 节奏（docs/02 源码实读 #2）。

mod lyrics;
mod providers;

use providers::TrackMeta;
use std::collections::HashMap;
use std::io::Write as _;
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use zbus::fdo::DBusProxy;
use zbus::proxy;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};
use zbus::Connection;

#[proxy(interface = "org.mpris.MediaPlayer2.Player", assume_defaults = true)]
trait MprisPlayer {
    #[zbus(property)]
    fn playback_status(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn rate(&self) -> zbus::Result<f64>;
    #[zbus(property)]
    fn position(&self) -> zbus::Result<i64>;
    #[zbus(property)]
    fn metadata(&self) -> zbus::Result<HashMap<String, OwnedValue>>;
    fn set_position(&self, track_id: ObjectPath<'_>, position: i64) -> zbus::Result<()>;
    fn play_pause(&self) -> zbus::Result<()>;
    fn next(&self) -> zbus::Result<()>;
    fn previous(&self) -> zbus::Result<()>;
    #[zbus(property)]
    fn shuffle(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn set_shuffle(&self, shuffle: bool) -> zbus::Result<()>;
    #[zbus(property)]
    fn loop_status(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn set_loop_status(&self, status: String) -> zbus::Result<()>;
}

#[proxy(interface = "org.mpris.MediaPlayer2", assume_defaults = true)]
trait MprisRoot {
    #[zbus(property)]
    fn identity(&self) -> zbus::Result<String>;
}

#[derive(Clone, Debug)]
struct Track {
    artist: String,
    title: String,
    album: String,
    duration_sec: f64,
    art_url: String,
    track_id: String,
}

impl Track {
    fn key(&self) -> String {
        format!("{}|{}|{}", self.artist, self.title, self.track_id)
    }
    fn empty() -> Self {
        Self {
            artist: String::new(),
            title: String::new(),
            album: String::new(),
            duration_sec: 0.0,
            art_url: String::new(),
            track_id: String::new(),
        }
    }
    fn is_empty(&self) -> bool {
        self.title.is_empty()
    }
}

struct Player<'a> {
    bus: String,
    proxy: MprisPlayerProxy<'a>,
    track: Track,
    status: String,
    rate: f64,
    anchor_us: i64,
    anchor_at: Instant,
}

impl Player<'_> {
    fn position_sec(&self, now: Instant) -> f64 {
        let elapsed = if self.status == "Playing" {
            now.duration_since(self.anchor_at).as_secs_f64() * self.rate.max(0.0)
        } else {
            0.0
        };
        (self.anchor_us as f64 / 1e6 + elapsed).max(0.0)
    }
}

fn emit(v: serde_json::Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

// 播放器状态（Shuffle/LoopStatus）→ QML 面板开关按钮；不支持的播放器按关闭兜底
async fn emit_player_state(proxy: &MprisPlayerProxy<'_>) {
    let shuffle = proxy.shuffle().await.unwrap_or(false);
    let loop_status = proxy
        .loop_status()
        .await
        .unwrap_or_else(|_| "None".to_string());
    emit(serde_json::json!({ "ev": "player", "shuffle": shuffle, "loop": loop_status }));
}

fn owned_to_string(v: &OwnedValue) -> Option<String> {
    match &**v {
        Value::Str(s) => Some(s.to_string()),
        Value::ObjectPath(p) => Some(p.to_string()),
        _ => None,
    }
}

fn owned_to_u64(v: &OwnedValue) -> Option<u64> {
    match &**v {
        Value::U64(u) => Some(*u),
        Value::I64(i) => Some(*i as u64),
        Value::U32(u) => Some(*u as u64),
        Value::F64(f) => Some(*f as u64),
        _ => None,
    }
}

fn metadata_to_track(md: &HashMap<String, OwnedValue>) -> Track {
    let get_str = |k: &str| md.get(k).and_then(owned_to_string).unwrap_or_default();
    let artist = match md.get("xesam:artist") {
        Some(v) => match &**v {
            Value::Array(arr) => arr
                .iter()
                .filter_map(|x| match x {
                    Value::Str(s) => Some(s.to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(", "),
            Value::Str(s) => s.to_string(),
            _ => String::new(),
        },
        None => String::new(),
    };
    Track {
        artist,
        title: get_str("xesam:title"),
        album: get_str("xesam:album"),
        duration_sec: md.get("mpris:length").and_then(owned_to_u64).unwrap_or(0) as f64 / 1e6,
        art_url: get_str("mpris:artUrl"),
        track_id: get_str("mpris:trackid"),
    }
}

fn emit_track(t: &Track) {
    emit(serde_json::json!({
        "ev": "track",
        "artist": t.artist,
        "title": t.title,
        "album": t.album,
        "durationSec": t.duration_sec,
        "artUrl": t.art_url,
    }));
}

async fn read_identity(conn: &Connection, bus: &str) -> String {
    let proxy = MprisRootProxy::builder(conn)
        .destination(bus.to_string())
        .unwrap()
        .path("/org/mpris/MediaPlayer2")
        .unwrap()
        .build()
        .await;
    match proxy {
        Ok(p) => p.identity().await.unwrap_or_default(),
        Err(_) => String::new(),
    }
}

async fn peek_player(conn: &Connection, bus: &str) -> Option<(String, Track)> {
    let proxy = MprisPlayerProxy::builder(conn)
        .destination(bus.to_string())
        .ok()?
        .path("/org/mpris/MediaPlayer2")
        .ok()?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
        .ok()?;
    let status = proxy.playback_status().await.ok()?;
    let track = metadata_to_track(&proxy.metadata().await.unwrap_or_default());
    Some((status, track))
}

/// 活跃播放器选择：同一 Identity 的多个 bus（Spotify 的 chromium.instance 副本）按
/// Identity 去重，组内优先 bus 名不含 ".instance" 的；playerctld 代理跳过；Playing 优先。
async fn choose_player(conn: &Connection, dbus: &DBusProxy<'_>) -> Option<String> {
    let names = dbus.list_names().await.ok()?;
    let mut candidates: Vec<String> = names
        .iter()
        .map(|n| n.to_string())
        .filter(|n| n.starts_with("org.mpris.MediaPlayer2.") && n != "org.mpris.MediaPlayer2")
        .filter(|n| !n.to_lowercase().contains("playerctld"))
        .collect();
    candidates.sort();

    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for bus in &candidates {
        let identity = match conn_id_cache(&bus).await {
            Some(id) if !id.is_empty() => id,
            _ => read_identity(conn, bus).await,
        };
        let identity = if identity.is_empty() {
            bus.trim_start_matches("org.mpris.MediaPlayer2.").to_string()
        } else {
            identity
        };
        groups.entry(identity).or_default().push(bus.clone());
    }

    let mut players: Vec<String> = Vec::new();
    for (_, mut buses) in groups {
        buses.sort_by_key(|b| b.contains(".instance")); // 非 instance 排前
        players.push(buses.remove(0));
    }

    let mut fallback = None;
    for bus in &players {
        let Some((status, track)) = peek_player(conn, bus).await else { continue };
        if status == "Playing" && !track.is_empty() {
            return Some(bus.clone());
        }
        if status != "Stopped" && !track.is_empty() && fallback.is_none() {
            fallback = Some(bus.clone());
        }
    }
    fallback.or_else(|| players.first().cloned())
}

// Identity 在 DBus 侧很少变，进程内缓存避免每 500ms 全量查询
async fn conn_id_cache(_bus: &str) -> Option<String> {
    None // v0: 不缓存，读 Identity 的开销可接受
}

async fn build_player<'c>(conn: &'c Connection, bus: &str) -> Option<Player<'c>> {
    let proxy = MprisPlayerProxy::builder(conn)
        .destination(bus.to_string())
        .ok()?
        .path("/org/mpris/MediaPlayer2")
        .ok()?
        // Spotify 从不发 Position 的 PropertiesChanged，zbus 默认缓存会让 position() 永远返回旧值
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
        .ok()?;
    Some(Player {
        bus: bus.to_string(),
        proxy,
        track: Track::empty(),
        status: String::new(),
        rate: 1.0,
        anchor_us: 0,
        anchor_at: Instant::now(),
    })
}

async fn poll_player(p: &mut Player<'_>, last_track_key: &mut String) -> bool {
    let status = p.proxy.playback_status().await.unwrap_or_else(|_| p.status.clone());
    let rate = p.proxy.rate().await.unwrap_or(1.0);
    let pos = match p.proxy.position().await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(err = %e, "position read failed");
            p.anchor_us
        }
    };
    let md = p.proxy.metadata().await.unwrap_or_default();
    let track = metadata_to_track(&md);

    let changed = track.key() != *last_track_key;
    *last_track_key = track.key();
    p.status = status;
    p.rate = rate;
    p.anchor_us = pos;
    p.anchor_at = Instant::now();
    p.track = track;
    changed
}

struct LyricsState {
    lines: Vec<lyrics::Line>,
    cur_line: i64,
    cur_word: i64,
}

fn main() -> zbus::Result<()> {
    zbus::block_on(run())
}

// 日志同时写 stderr（终端联调）和文件（quickshell 抓不到子进程 stderr，必须落盘）
struct MakeTee {
    file: Mutex<tracing_appender::non_blocking::NonBlocking>,
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for MakeTee {
    type Writer = TeeWriter<'a>;
    fn make_writer(&'a self) -> Self::Writer {
        TeeWriter { tee: self }
    }
}

struct TeeWriter<'a> {
    tee: &'a MakeTee,
}

impl std::io::Write for TeeWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        if let Ok(mut g) = self.tee.file.lock() {
            let _ = g.write_all(buf);
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

async fn run() -> zbus::Result<()> {
    let state_dir = providers::state_dir();
    let _ = std::fs::create_dir_all(state_dir.join("cache"));
    let file_appender = tracing_appender::rolling::daily(&state_dir, "lyricsd.log");
    let _log_guard = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(MakeTee { file: Mutex::new(_log_guard.0) })
        .with_ansi(false)
        .init();

    tracing::info!("lyricsd starting");
    emit(serde_json::json!({ "ev": "backend", "msg": "started" }));

    let conn = Connection::session().await?;
    let dbus = DBusProxy::new(&conn).await?;

    let mut player: Option<Player> = None;
    let mut last_track_key = String::new();
    let mut lyrics_state = LyricsState { lines: Vec::new(), cur_line: -1, cur_word: -1 };

    let (fetch_tx, fetch_rx) = mpsc::channel::<providers::FetchResult>();
    let (cmd_tx, cmd_rx) = mpsc::channel::<serde_json::Value>();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                if cmd_tx.send(v).is_err() {
                    break;
                }
            }
        }
    });

    let mut lookup_at: Option<Instant> = None;
    let mut bypass_cache = false;
    let mut lookup_gen: u64 = 0;
    let mut none_retries: u32 = 0; // 同一首曲目查空后的自动重试计数（词源临时故障兜底）
    let mut last_poll = Instant::now() - Duration::from_secs(3600);
    let mut last_position_emit = Instant::now();
    let mut last_playing = false;
    let mut last_player_emit = Instant::now() - Duration::from_secs(3600);

    loop {
        async_io::Timer::after(Duration::from_millis(100)).await;
        let now = Instant::now();

        if now.duration_since(last_poll) >= Duration::from_millis(500) {
            last_poll = now;
            let chosen = choose_player(&conn, &dbus).await;
            match chosen {
                Some(bus) => {
                    let need_new = player.as_ref().map(|p| p.bus != bus).unwrap_or(true);
                    if need_new {
        player = build_player(&conn, &bus).await;
                    }
                }
                None => {
                    if player.is_some() {
                        player = None;
                        last_track_key.clear();
                        lyrics_state = LyricsState { lines: Vec::new(), cur_line: -1, cur_word: -1 };
                        emit_track(&Track::empty());
                    }
                }
            }
            if let Some(p) = player.as_mut() {
                if poll_player(p, &mut last_track_key).await && !p.track.is_empty() {
                    tracing::info!(artist = %p.track.artist, title = %p.track.title, bus = %p.bus, "track changed");
                    emit_track(&p.track);
                    emit_player_state(&p.proxy).await;
                    lyrics_state = LyricsState { lines: Vec::new(), cur_line: -1, cur_word: -1 };
                    bypass_cache = false;
                    lookup_gen += 1;
                    none_retries = 0;
                    if p.track.artist.is_empty() {
                        // Spotify 广告位特征（artist 为空）：不查词源，广告文案直接当字幕
                        lookup_at = None;
                        emit(serde_json::json!({ "ev": "status", "msg": "ad", "text": p.track.title }));
                    } else {
                        lookup_at = Some(now + Duration::from_millis(700));
                    }
                }
            }
        }

        // 到点发起歌词查询（换曲 700ms 防抖后触发；refresh 直接置 now）
        if let Some(deadline) = lookup_at {
            if now >= deadline {
                lookup_at = None;
                if let Some(p) = player.as_ref() {
                    if !p.track.is_empty() {
                        let tx = fetch_tx.clone();
                        let meta = TrackMeta {
                            artist: p.track.artist.clone(),
                            title: p.track.title.clone(),
                            album: p.track.album.clone(),
                            duration_sec: p.track.duration_sec,
                        };
                        let bypass = bypass_cache;
                        bypass_cache = false;
                        tracing::info!(artist = %meta.artist, title = %meta.title, "lookup spawned");
                        let generation = lookup_gen;
                        std::thread::spawn(move || providers::lookup(tx, meta, bypass, generation));
                    }
                }
            }
        }

        // 命令
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd.get("cmd").and_then(|c| c.as_str()) {
                Some("refresh") => {
                    tracing::info!("cmd refresh");
                    bypass_cache = true;
                    lookup_gen += 1;
                    lookup_at = Some(now);
                }
                Some("seek") => {
                    if let (Some(p), Some(sec)) = (player.as_ref(), cmd.get("sec").and_then(|s| s.as_f64())) {
                        if let Ok(tp) = ObjectPath::try_from(p.track.track_id.as_str()) {
                            if let Err(e) = p.proxy.set_position(tp, (sec * 1e6) as i64).await {
                                tracing::warn!(err = %e, "set_position failed");
                            }
                        }
                    }
                }
                Some("toggle") => {
                    if let Some(p) = player.as_ref() {
                        if let Err(e) = p.proxy.play_pause().await {
                            tracing::warn!(err = %e, "play_pause failed");
                        }
                    }
                }
                Some("next") => {
                    if let Some(p) = player.as_ref() {
                        if let Err(e) = p.proxy.next().await {
                            tracing::warn!(err = %e, "next failed");
                        }
                    }
                }
                Some("prev") => {
                    if let Some(p) = player.as_ref() {
                        if let Err(e) = p.proxy.previous().await {
                            tracing::warn!(err = %e, "previous failed");
                        }
                    }
                }
                Some("shuffle") => {
                    if let Some(p) = player.as_ref() {
                        let cur = p.proxy.shuffle().await.unwrap_or(false);
                        let want = cmd.get("on").and_then(|v| v.as_bool()).unwrap_or(!cur);
                        if let Err(e) = p.proxy.set_shuffle(want).await {
                            tracing::warn!(err = %e, "set_shuffle failed");
                        }
                        emit_player_state(&p.proxy).await;
                    }
                }
                Some("loop") => {
                    if let Some(p) = player.as_ref() {
                        // MPRIS LoopStatus：None → Playlist(列表循环) → Track(单曲循环) → None
                        let cur = p.proxy.loop_status().await.unwrap_or_else(|_| "None".into());
                        let want = match cmd.get("mode").and_then(|v| v.as_str()) {
                            Some(m @ ("None" | "Track" | "Playlist")) => m.to_string(),
                            _ => match cur.as_str() {
                                "None" => "Playlist".to_string(),
                                "Track" => "None".to_string(),
                                _ => "Track".to_string(),
                            },
                        };
                        if let Err(e) = p.proxy.set_loop_status(want).await {
                            tracing::warn!(err = %e, "set_loop_status failed");
                        }
                        emit_player_state(&p.proxy).await;
                    }
                }
                _ => {}
            }
        }

        // 抓取结果（单飞：一次只有一个查询线程；换曲/refresh 后的迟到结果按 gen 丢弃）
        while let Ok(res) = fetch_rx.try_recv() {
            if res.generation != lookup_gen {
                tracing::info!(source = %res.source, gen = res.generation, "stale fetch result dropped");
                continue;
            }
            tracing::info!(source = %res.source, lines = res.lines.len(), "fetch result");
            if res.source == "none" || res.lines.is_empty() {
                lyrics_state.lines.clear();
                emit(serde_json::json!({ "ev": "status", "msg": "no_lyrics" }));
                // 词源临时故障（如 LRCLIB 间歇 503）兜底：30s 后重查，同一首最多 3 次
                if none_retries < 3 {
                    none_retries += 1;
                    lookup_gen += 1;
                    lookup_at = Some(Instant::now() + Duration::from_secs(30));
                    tracing::info!(retry = none_retries, "no lyrics; retry scheduled in 30s");
                }
                break;
            }
            lyrics_state.lines = res.lines;
            lyrics_state.cur_line = -1;
            lyrics_state.cur_word = -1;
            emit(serde_json::json!({ "ev": "lines", "source": res.source, "synced": lyrics_state.lines }));
        }

        // 播放器开关状态周期同步（外部在播放器里改 shuffle/loop 也能反映到面板）
        if player.is_some() && now.duration_since(last_player_emit) >= Duration::from_secs(2) {
            last_player_emit = now;
            if let Some(p) = player.as_ref() {
                emit_player_state(&p.proxy).await;
            }
        }

        // 位置外推 → 行/词事件
        if let Some(p) = player.as_ref() {
            let pos = p.position_sec(now);
            let playing = p.status == "Playing" && !p.track.is_empty();
            if playing != last_playing {
                last_playing = playing;
                last_position_emit = now;
                emit(serde_json::json!({ "ev": "position", "sec": (pos * 10.0).round() / 10.0, "playing": playing }));
            } else if playing && now.duration_since(last_position_emit) >= Duration::from_secs(1) {
                last_position_emit = now;
                emit(serde_json::json!({ "ev": "position", "sec": (pos * 10.0).round() / 10.0, "playing": true }));
            }
            if !lyrics_state.lines.is_empty() && p.track.is_empty() == false {
                let idx = lyrics::active_line(&lyrics_state.lines, pos);
                let mut need_emit = idx != lyrics_state.cur_line;
                let mut word = -1i64;
                if !need_emit && idx >= 0 {
                    let line = &lyrics_state.lines[idx as usize];
                    if line.words.is_some() {
                        word = lyrics::active_word(line, (pos * 1000.0) as u32);
                        need_emit = word != lyrics_state.cur_word;
                    }
                }
                if need_emit {
                    lyrics_state.cur_line = idx;
                    lyrics_state.cur_word = word;
                    if let Some(line) = (idx >= 0).then(|| lyrics_state.lines[idx as usize].clone()) {
                        emit(serde_json::json!({
                            "ev": "line",
                            "index": idx,
                            "word": word,
                            "text": line.text,
                            "rich": lyrics::rich_markup(&line, (pos * 1000.0) as u32),
                        }));
                    }
                }
            }
        }
    }
}
