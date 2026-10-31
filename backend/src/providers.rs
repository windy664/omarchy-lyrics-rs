// 歌词抓取：词级（lrc.red 的 Apple Music TTML / binimum.org）→ LRCLIB 行级 → 纯文本。
// 抓取在独立阻塞线程里跑（ureq），结果经 mpsc 送回主循环。协议细节见 docs/04。

use crate::lyrics::{self, LrcCandidate, Line};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Duration;
use tracing::{debug, info, warn};

pub const USER_AGENT: &str = concat!("omarchy-lyrics-rs/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone)]
pub struct TrackMeta {
    pub artist: String,
    pub title: String,
    pub album: String,
    pub duration_sec: f64,
}

#[derive(Debug, Clone)]
pub struct FetchResult {
    pub source: String, // word:lrc.red \
    pub lines: Vec<Line>,
    pub instrumental: bool,
    pub generation: u64, // 发起时的查询代号，main 侧用它丢弃换曲后迟到的过期结果
}

pub fn state_dir() -> PathBuf {
    dirs::state_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".local/state"))
        .join("omarchy-lyrics-rs")
}

fn cache_path(artist: &str, title: &str, album: &str, duration_sec: f64) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    artist.to_lowercase().hash(&mut h);
    title.to_lowercase().hash(&mut h);
    album.to_lowercase().hash(&mut h);
    ((duration_sec / 15.0).round() as i64).hash(&mut h); // 15 秒桶，容忍时长达毫秒级抖动
    state_dir().join("cache").join(format!("{:016x}.json", h.finish()))
}

fn http() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(6))
        .timeout(Duration::from_secs(10))
        .user_agent(USER_AGENT)
        .build()
}

fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn get_text(agent: &ureq::Agent, url: &str) -> Option<String> {
    // LRCLIB 高峰期常见瞬时 503，5xx 短退避重试两次；4xx 与网络错误不重试
    for attempt in 0..3u32 {
        match agent.get(url).call() {
            Ok(resp) => return match resp.into_string() {
                Ok(s) => Some(s),
                Err(e) => {
                    debug!(url, err = %e, "body read failed");
                    None
                }
            },
            Err(ureq::Error::Status(code, _)) if code >= 500 && attempt < 2 => {
                debug!(url, code, attempt, "server error, retrying");
                std::thread::sleep(Duration::from_millis(800 * (1 << attempt)));
            }
            Err(ureq::Error::Status(code, _)) => {
                debug!(url, code, "http error");
                return None;
            }
            Err(e) => {
                debug!(url, err = %e, "request failed");
                return None;
            }
        }
    }
    None
}

// --- 词级源 ----------------------------------------------------------------

/// lrc.red：Apple Music TTML 聚合。响应要么本身就是 TTML，要么是带 lyricsUrl 的 JSON。
fn fetch_lrc_red(agent: &ureq::Agent, query: &str) -> Option<Vec<Line>> {
    let url = format!("https://lrc.red/api/v1?q={}", enc(query));
    let body = get_text(agent, &url)?;
    if lyrics::looks_like_ttml(&body) {
        let lines = lyrics::parse_ttml(&body);
        return if lines.is_empty() { None } else { Some(lines) };
    }
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    let results = v.get("results")?.as_array()?;
    let pick = results
        .iter()
        .find(|r| r.get("timing_type").and_then(|t| t.as_str()) == Some("word"))
        .or_else(|| results.first())?;
    let lyrics_url = pick.get("lyricsUrl")?.as_str()?;
    let ttml = get_text(agent, lyrics_url)?;
    let lines = lyrics::parse_ttml(&ttml);
    if lines.is_empty() { None } else { Some(lines) }
}

/// karalyr.com：开放词级卡拉OK库，API 形状与 LRCLIB 兼容，只服务词级（未命中 404）。
/// syncedLyrics 为 Enhanced LRC（行内 <mm:ss.xx> 词标签），parse_lrc 直接消化。
/// 2026-10-31 探测覆盖率仍接近 0，先接入占位，库长大后零成本生效。
fn fetch_karalyr(agent: &ureq::Agent, m: &TrackMeta) -> Option<Vec<Line>> {
    let url = format!(
        "https://www.karalyr.com/api/get?artist_name={}&track_name={}",
        enc(&lyrics::clean_artist(&m.artist)),
        enc(&lyrics::clean_title(&m.title))
    );
    let body = get_text(agent, &url)?;
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    let synced = v.get("syncedLyrics")?.as_str()?;
    let lines = lyrics::parse_lrc(synced);
    if lines.is_empty() { None } else { Some(lines) }
}

// --- LRCLIB ----------------------------------------------------------------

fn candidate_from_json(v: &serde_json::Value) -> Option<LrcCandidate> {
    Some(LrcCandidate {
        track: v.get("trackName")?.as_str()?.to_string(),
        artist: v.get("artistName")?.as_str()?.to_string(),
        duration: v.get("duration").and_then(|d| d.as_f64()).unwrap_or(0.0),
        synced: v.get("syncedLyrics").and_then(|s| s.as_str()).map(String::from),
        plain: v.get("plainLyrics").and_then(|s| s.as_str()).map(String::from),
    })
}

fn lrclib_get(agent: &ureq::Agent, m: &TrackMeta) -> Option<serde_json::Value> {
    let url = format!(
        "https://lrclib.net/api/get?artist_name={}&track_name={}&album_name={}&duration={}",
        enc(&lyrics::clean_artist(&m.artist)),
        enc(&lyrics::clean_title(&m.title)),
        enc(&m.album),
        m.duration_sec.round() as i64,
    );
    let body = get_text(agent, &url)?;
    serde_json::from_str(&body).ok()
}

fn lrclib_search(agent: &ureq::Agent, m: &TrackMeta) -> Vec<serde_json::Value> {
    let url = format!(
        "https://lrclib.net/api/search?track_name={}&artist_name={}",
        enc(&lyrics::clean_title(&m.title)),
        enc(&lyrics::clean_artist(&m.artist)),
    );
    let Some(body) = get_text(agent, &url) else { return Vec::new() };
    serde_json::from_str::<Vec<serde_json::Value>>(&body).unwrap_or_default()
}

/// LRCLIB：精确匹配 → 搜索打分。命中行级返回 (source, lines)，只有纯文本也返回。
fn fetch_lrclib(agent: &ureq::Agent, m: &TrackMeta) -> Option<(String, Vec<Line>, bool)> {
    let mut candidates: Vec<LrcCandidate> = Vec::new();
    if let Some(v) = lrclib_get(agent, m) {
        if let Some(c) = candidate_from_json(&v) {
            candidates.push(c);
        }
    }
    if candidates.iter().all(|c| c.synced.is_none()) {
        for v in lrclib_search(agent, m) {
            if let Some(c) = candidate_from_json(&v) {
                candidates.push(c);
            }
        }
    }
    let pick = lyrics::pick_candidate(&candidates, &m.artist, &m.title, m.duration_sec)?;
    if let Some(synced) = pick.synced.clone() {
        let lines = lyrics::parse_lrc(&synced);
        if !lines.is_empty() {
            return Some(("lrclib".into(), lines, false));
        }
    }
    if let Some(plain) = pick.plain.clone() {
        let lines = lyrics::plain_to_lines(&plain, m.duration_sec);
        if !lines.is_empty() {
            return Some(("lrclib-plain".into(), lines, false));
        }
    }
    None
}

// --- 缓存 + 总入口 -----------------------------------------------------------

fn read_cache(path: &PathBuf) -> Option<FetchResult> {
    let text = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(FetchResult {
        source: v.get("source").and_then(|s| s.as_str()).unwrap_or("cache").to_string(),
        lines: serde_json::from_value(v.get("lines").cloned()?).ok()?,
        instrumental: v.get("instrumental").and_then(|b| b.as_bool()).unwrap_or(false),
        generation: 0,
    })
}

fn write_cache(path: &PathBuf, r: &FetchResult) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let v = serde_json::json!({ "source": r.source, "lines": r.lines, "instrumental": r.instrumental });
    if let Ok(s) = serde_json::to_string(&v) {
        let _ = std::fs::write(path, s);
    }
}

/// 完整查询入口（跑在工作线程里）。
pub fn lookup(tx: Sender<FetchResult>, meta: TrackMeta, bypass_cache: bool, generation: u64) {
    let start = std::time::Instant::now();
    let cache = cache_path(&meta.artist, &meta.title, &meta.album, meta.duration_sec);
    if !bypass_cache {
        if let Some(hit) = read_cache(&cache) {
            info!(artist = %meta.artist, title = %meta.title, source = %hit.source, "cache hit");
            let _ = tx.send(FetchResult { generation, ..hit });
            return;
        }
    }

    let agent = http();
    let mut result: Option<FetchResult> = None;

    // 词级（lrc.red ‖ karalyr 并发）与行级（LRCLIB）三路并发，词级优先、行级 12 秒兜底
    // binimum.org 已退役：2026-10-31 实测其 API 307 重定向到 lrc.red，是同一家
    enum Msg {
        Word(String, Vec<Line>),
        Line(String, Vec<Line>, bool),
    }
    let (ltx, lrx) = std::sync::mpsc::channel::<Msg>();
    let ltx2 = ltx.clone();
    let ltx3 = ltx.clone();
    let m1 = meta.clone();
    let m2 = meta.clone();
    let m3 = meta.clone();
    let agent1 = http();
    let agent2 = http();
    std::thread::scope(|s| {
        s.spawn(move || {
            let query = format!(
                "{} {}",
                lyrics::clean_title(&m1.title),
                lyrics::clean_artist(&m1.artist)
            );
            if let Some(lines) = fetch_lrc_red(&agent1, &query) {
                let _ = ltx.send(Msg::Word("word:lrc.red".into(), lines));
            }
        });
        s.spawn(move || {
            if let Some(lines) = fetch_karalyr(&agent2, &m3) {
                let _ = ltx2.send(Msg::Word("word:karalyr".into(), lines));
            }
        });
        s.spawn(move || {
            if let Some((src, lines, inst)) = fetch_lrclib(&agent, &m2) {
                let _ = ltx3.send(Msg::Line(src, lines, inst));
            }
        });
        let mut word: Option<(String, Vec<Line>)> = None;
        let mut line: Option<(String, Vec<Line>, bool)> = None;
        let deadline = std::time::Instant::now() + Duration::from_secs(12);
        while (word.is_none() || line.is_none()) && std::time::Instant::now() < deadline {
            match lrx.recv_timeout(Duration::from_millis(200)) {
                Ok(Msg::Word(src, lines)) => word = Some((src, lines)),
                Ok(Msg::Line(src, lines, inst)) => line = Some((src, lines, inst)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        result = match (word, line) {
            (Some((src, lines)), _) => Some(FetchResult { source: src, lines, instrumental: false, generation }),
            (None, Some((src, lines, inst))) => Some(FetchResult { source: src, lines, instrumental: inst, generation }),
            (None, None) => None,
        };
    });

    let final_result = result.unwrap_or(FetchResult { source: "none".into(), lines: Vec::new(), instrumental: false, generation });
    if final_result.source != "none" {
        write_cache(&cache, &final_result);
    }
    info!(
        artist = %meta.artist,
        title = %meta.title,
        source = %final_result.source,
        lines = final_result.lines.len(),
        elapsed_ms = start.elapsed().as_millis() as u64,
        "lookup done"
    );
    if final_result.source == "none" {
        warn!("no lyrics found from any provider");
    }
    let _ = tx.send(final_result);
}
