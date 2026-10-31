// LRC / TTML / JSON 歌词解析、候选打分、当前行/词定位。
// 规则大量移植自 crmne/omarchy-lyrics 的 Model.js 与 DMP 的 LyricsClient.js（见 docs/02、docs/07）。

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, serde::Deserialize)]
pub struct Word(pub u32, pub String); // (开始时间 ms, 词文本)

#[derive(Debug, Clone, Serialize, PartialEq, serde::Deserialize)]
pub struct Line {
    pub t: f64, // 行开始时间，秒
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub words: Option<Vec<Word>>, // 词级时间轴（卡拉OK）
}

// --- 元数据清洗 -----------------------------------------------------------

fn strip_zero_width(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(*c, '\u{200b}' | '\u{2060}'..='\u{2064}' | '\u{feff}'))
        .collect()
}

const BRACKETED_NOISE: &str = r"(?i)\s*[\(\[][^\)\]]*\b(remaster(ed)?|remix|live|acoustic|version|edit|mix|mono|stereo|deluxe|bonus|expanded|explicit|anniversary|feat\.?|featuring|with)\b[^\)\]]*[\)\]]";
const TRAILING_NOISE: &str = r"(?i)\s+-\s+.*\b(remaster(ed)?|radio edit|single version|album version|live|mono|stereo|re-?recorded|\d{4}\s+version)\b.*$";
const FEATURING: &str = r"(?i)\s+[-\(]?\s*\b(feat|ft|featuring)\b\.?\s+.*$";

pub fn clean_title(title: &str) -> String {
    let re_br = regex::Regex::new(BRACKETED_NOISE).unwrap();
    let re_tr = regex::Regex::new(TRAILING_NOISE).unwrap();
    let re_ft = regex::Regex::new(FEATURING).unwrap();
    let cleaned = re_ft
        .replace_all(&re_tr.replace_all(&re_br.replace_all(&strip_zero_width(title), ""), ""), "")
        .trim()
        .to_string();
    if cleaned.is_empty() {
        strip_zero_width(title).trim().to_string()
    } else {
        cleaned
    }
}

pub fn clean_artist(artist: &str) -> String {
    let re_ft = regex::Regex::new(FEATURING).unwrap();
    let cleaned = re_ft
        .replace_all(&strip_zero_width(artist), "")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if cleaned.is_empty() {
        strip_zero_width(artist).trim().to_string()
    } else {
        cleaned
    }
}

fn normalize(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if c.is_alphanumeric() {
            // 保留 CJK 等非 ASCII 文字，避免中文全被剥掉
            out.push(c);
        }
    }
    out
}

fn loose_match(a: &str, b: &str) -> bool {
    let a = normalize(a);
    let b = normalize(b);
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || a.contains(&b) || b.contains(&a)
}

// --- LRC ------------------------------------------------------------------

const TIMESTAMP: &str = r"\[(\d{1,2}):(\d{1,2})(?:[.:](\d{1,3}))?\]";
const WORD_TAG: &str = r"[<\(\[](\d{1,2}):(\d{1,2})[.:](\d{1,3})[>\)\]]";

fn stamp_to_sec(min: &str, sec: &str, frac: Option<&str>) -> f64 {
    let f = frac.map(|s| format!("0.{s}").parse::<f64>().unwrap_or(0.0)).unwrap_or(0.0);
    min.parse::<f64>().unwrap_or(0.0) * 60.0 + sec.parse::<f64>().unwrap_or(0.0) + f
}

fn stamp_to_ms(min: &str, sec: &str, frac: Option<&str>) -> u32 {
    (stamp_to_sec(min, sec, frac) * 1000.0).round() as u32
}

/// 解析行级或增强型（词级 `<mm:ss.xx>` 标签）LRC。
pub fn parse_lrc(text: &str) -> Vec<Line> {
    let ts_re = regex::Regex::new(TIMESTAMP).unwrap();
    let wt_re = regex::Regex::new(WORD_TAG).unwrap();
    let mut lines = Vec::new();

    for raw in text.replace("\r\n", "\n").split('\n') {
        let mut times = Vec::new();
        let mut cursor = 0usize;
        for cap in ts_re.captures_iter(raw) {
            let m = cap.get(0).unwrap();
            if m.start() != cursor {
                break; // 只认行首连续时间戳，行中出现的方括号按歌词处理
            }
            cursor = m.end();
            times.push(stamp_to_sec(
                cap.get(1).unwrap().as_str(),
                cap.get(2).unwrap().as_str(),
                cap.get(3).map(|c| c.as_str()),
            ));
        }
        if times.is_empty() {
            continue;
        }
        let body = &raw[cursor..];
        // 增强型 LRC：正文里的 <mm:ss.xx>word 词级标签
        let mut words = Vec::new();
        let mut plain = String::new();
        let mut last = 0usize;
        for cap in wt_re.captures_iter(body) {
            let m = cap.get(0).unwrap();
            plain.push_str(&body[last..m.start()]);
            last = m.end();
            let text = body[m.end()..]
                .chars()
                .take_while(|c| !matches!(*c, '<' | '>' | '(' | '['))
                .collect::<String>();
            if !text.trim().is_empty() {
                words.push(Word(
                    stamp_to_ms(
                        cap.get(1).unwrap().as_str(),
                        cap.get(2).unwrap().as_str(),
                        cap.get(3).map(|c| c.as_str()),
                    ),
                    text.trim().to_string(),
                ));
            }
        }
        plain.push_str(&body[last..]);
        let text = if plain.trim().is_empty() { body.trim() } else { plain.trim() }.to_string();
        for t in times {
            lines.push(Line {
                t,
                text: text.clone(),
                words: if words.len() > 1 { Some(words.clone()) } else { None },
            });
        }
    }
    lines.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
    lines
}

// --- TTML（Apple Music 词级） ----------------------------------------------

fn ttml_time_ms(v: &str) -> u32 {
    let parts: Vec<&str> = v.trim().split(':').collect();
    let sec = match parts.len() {
        1 => parts[0].parse::<f64>().unwrap_or(0.0),
        2 => parts[0].parse::<f64>().unwrap_or(0.0) * 60.0 + parts[1].parse::<f64>().unwrap_or(0.0),
        3 => {
            parts[0].parse::<f64>().unwrap_or(0.0) * 3600.0
                + parts[1].parse::<f64>().unwrap_or(0.0) * 60.0
                + parts[2].parse::<f64>().unwrap_or(0.0)
        }
        _ => 0.0,
    };
    (sec * 1000.0).round() as u32
}

pub fn looks_like_ttml(text: &str) -> bool {
    text.contains("<tt") || text.contains("<p begin=") || text.contains("<p ") && text.contains("begin=\"")
}

/// 解析 TTML：`<p begin>` 行 + `<span begin end>` 词级时间轴。
pub fn parse_ttml(text: &str) -> Vec<Line> {
    let bg_re = regex::Regex::new(r#"(?is)<span\b[^>]*ttm:role="x-bg"[^>]*>.*?</span>"#).unwrap();
    let p_re = regex::Regex::new(r#"(?is)<p\b[^>]*\bbegin="([^"]+)"[^>]*>(.*?)</p>"#).unwrap();
    let span_re =
        regex::Regex::new(r#"(?is)<span\b[^>]*\bbegin="([^"]+)"[^>]*\bend="([^"]+)"[^>]*>([^<]*)</span>"#)
            .unwrap();
    let tag_re = regex::Regex::new(r"<[^>]+>").unwrap();

    let mut lines: Vec<(u32, String, Option<Vec<Word>>)> = Vec::new();
    for cap in p_re.captures_iter(text) {
        let start = ttml_time_ms(cap.get(1).unwrap().as_str());
        let body = bg_re.replace_all(cap.get(2).unwrap().as_str(), "");
        let mut words = Vec::new();
        for sc in span_re.captures_iter(&body) {
            let wtext = sc.get(3).unwrap().as_str().trim();
            if !wtext.is_empty() {
                words.push(Word(ttml_time_ms(sc.get(1).unwrap().as_str()), wtext.to_string()));
            }
        }
        let text = tag_re.replace_all(&body, "").split_whitespace().collect::<Vec<_>>().join(" ");
        if !text.is_empty() {
            let words = if words.len() > 1 { Some(words) } else { None };
            lines.push((start, text, words));
        }
    }
    lines.sort_by_key(|l| l.0);
    // 合并时间戳几乎相同的行（TTML 常见重复）
    let mut merged: Vec<(u32, String, Option<Vec<Word>>)> = Vec::new();
    for (t, text, words) in lines {
        if let Some(last) = merged.last_mut() {
            if (last.0 as i64 - t as i64).abs() < 100 {
                if !last.1.contains(&text) {
                    last.1.push(' ');
                    last.1.push_str(&text);
                }
                if let (Some(lw), Some(w)) = (&mut last.2, words) {
                    lw.extend(w);
                }
                continue;
            }
        }
        merged.push((t, text, words));
    }
    merged
        .into_iter()
        .map(|(ms, text, words)| Line { t: ms as f64 / 1000.0, text, words })
        .collect()
}

// --- JSON 词级（binimum 等 {lines:[{startTimeMs,text,syllables}]} 形态） ----

pub fn parse_json_lines(v: &serde_json::Value) -> Vec<Line> {
    let arr = match v.get("lines").or(v.get("lyrics")).or(v.get("data")).or(v.get("result")) {
        Some(a) if a.is_array() => a,
        _ => return Vec::new(),
    };
    let mut lines: Vec<(u32, String, Option<Vec<Word>>)> = Vec::new();
    for item in arr.as_array().unwrap() {
        let ms = item
            .get("startTimeMs")
            .or(item.get("time"))
            .or(item.get("begin"))
            .and_then(|x| x.as_str().map(|s| s.parse::<u32>().ok()).unwrap_or(x.as_u64().map(|u| u as u32)))
            .unwrap_or(0);
        let text = item
            .get("text")
            .or(item.get("words"))
            .or(item.get("line"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if text.is_empty() {
            continue;
        }
        let mut words = Vec::new();
        if let Some(syl) = item.get("syllables").or(item.get("wordsArray")).and_then(|x| x.as_array()) {
            for (i, w) in syl.iter().enumerate() {
                let wt = w
                    .get("startTimeMs")
                    .or(w.get("time"))
                    .or(w.get("begin"))
                    .and_then(|x| {
                        x.as_str().map(|s| s.parse::<u32>().ok()).unwrap_or(x.as_u64().map(|u| u as u32))
                    })
                    .unwrap_or(ms);
                let wtext = w
                    .get("text")
                    .or(w.get("word"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if !wtext.is_empty() {
                    let _ = i;
                    words.push(Word(wt, wtext));
                }
            }
        }
        let words = if words.len() > 1 { Some(words) } else { None };
        lines.push((ms, text, words));
    }
    lines.sort_by_key(|l| l.0);
    lines
        .into_iter()
        .map(|(ms, text, words)| Line { t: ms as f64 / 1000.0, text, words })
        .collect()
}

// --- 纯文本兜底 ------------------------------------------------------------

pub fn plain_to_lines(text: &str, duration_sec: f64) -> Vec<Line> {
    let rows: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let n = rows.len();
    if n == 0 {
        return Vec::new();
    }
    let per = if duration_sec > 10.0 { duration_sec / n as f64 } else { 4.0 };
    rows.into_iter()
        .enumerate()
        .map(|(i, text)| Line { t: i as f64 * per, text: text.to_string(), words: None })
        .collect()
}

// --- LRCLIB 候选打分（Model.js 移植） --------------------------------------

pub struct LrcCandidate {
    pub track: String,
    pub artist: String,
    pub duration: f64,
    pub synced: Option<String>,
    pub plain: Option<String>,
}

fn score_candidate(c: &LrcCandidate, artist: &str, title: &str, duration: f64) -> f64 {
    if !loose_match(&c.track, &clean_title(title)) {
        return -1.0;
    }
    let mut score = 0.0;
    if loose_match(&c.artist, &clean_artist(artist)) {
        score += 1000.0;
    }
    if duration > 0.0 && c.duration > 0.0 {
        let drift = (c.duration - duration).abs();
        if drift > 30.0 {
            return -1.0;
        }
        score += (30.0 - drift) * 10.0;
    }
    if c.synced.is_some() {
        score += 200.0;
    } else if c.plain.is_some() {
        score += 50.0;
    }
    score
}

pub fn pick_candidate<'a>(
    list: &'a [LrcCandidate],
    artist: &str,
    title: &str,
    duration: f64,
) -> Option<&'a LrcCandidate> {
    let mut best: Option<(f64, &LrcCandidate)> = None;
    for c in list {
        let s = score_candidate(c, artist, title, duration);
        if s >= 0.0 && best.as_ref().map(|(bs, _)| s > *bs).unwrap_or(true) {
            best = Some((s, c));
        }
    }
    best.map(|(_, c)| c)
}

// --- 定位 -------------------------------------------------------------------

/// 当前行下标（最后一个 t <= pos 的行），二分。
pub fn active_line(lines: &[Line], pos_sec: f64) -> i64 {
    let mut low = 0i64;
    let mut high = lines.len() as i64 - 1;
    let mut found = -1i64;
    while low <= high {
        let mid = (low + high) / 2;
        if lines[mid as usize].t <= pos_sec {
            found = mid;
            low = mid + 1;
        } else {
            high = mid - 1;
        }
    }
    found
}

pub fn active_word(line: &Line, pos_ms: u32) -> i64 {
    let Some(words) = &line.words else { return -1 };
    let mut found = -1i64;
    for (i, w) in words.iter().enumerate() {
        if w.0 <= pos_ms {
            found = i as i64;
        } else {
            break;
        }
    }
    found
}

pub fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// 当前行的卡拉OK富文本：已唱词加粗，未唱词半透明。
pub fn rich_markup(line: &Line, pos_ms: u32) -> String {
    match &line.words {
        Some(words) if !words.is_empty() => {
            let mut out = String::new();
            for w in words {
                let esc = escape_html(&w.1);
                if w.0 <= pos_ms {
                    out.push_str(&format!("<b>{esc}</b> "));
                } else {
                    out.push_str(&format!("<span color=\"#888888\">{esc}</span> "));
                }
            }
            out.trim_end().to_string()
        }
        _ => format!("<b>{}</b>", escape_html(&line.text)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc_line_level() {
        let lrc = "[00:12.30]第一句\n[01:02.5]第二句\n[ar:某人]";
        let lines = parse_lrc(lrc);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].t, 12.3);
        assert_eq!(lines[0].text, "第一句");
        assert_eq!(lines[1].t, 62.5);
        assert!(lines[0].words.is_none());
    }

    #[test]
    fn lrc_multi_stamp_and_word_tags() {
        let lrc = "[00:10.00][00:20.00]repeat <00:10.00>he<00:10.50>llo";
        let lines = parse_lrc(lrc);
        assert_eq!(lines.len(), 2);
        let w = lines[0].words.as_ref().unwrap();
        assert_eq!(w.len(), 2);
        assert_eq!(w[1], Word(10500, "llo".into()));
    }

    #[test]
    fn ttml_words() {
        let ttml = r#"<tt><p begin="00:05.000">not-a <span begin="00:05.000" end="00:05.400">first</span> <span begin="00:05.400" end="00:06.000">word</span></p></tt>"#;
        let lines = parse_ttml(ttml);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].t, 5.0);
        let w = lines[0].words.as_ref().unwrap();
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].1, "first");
    }

    #[test]
    fn active_line_bsearch() {
        let lines = vec![
            Line { t: 10.0, text: "a".into(), words: None },
            Line { t: 20.0, text: "b".into(), words: None },
        ];
        assert_eq!(active_line(&lines, 5.0), -1);
        assert_eq!(active_line(&lines, 10.0), 0);
        assert_eq!(active_line(&lines, 19.9), 0);
        assert_eq!(active_line(&lines, 100.0), 1);
    }

    #[test]
    fn scoring_rejects_drift_and_rewards_sync() {
        let c = LrcCandidate {
            track: "Song".into(),
            artist: "Singer".into(),
            duration: 100.0,
            synced: None,
            plain: Some("x".into()),
        };
        assert!(score_candidate(&c, "singer", "song", 101.0) > 0.0);
        assert_eq!(score_candidate(&c, "singer", "song", 200.0), -1.0);
        assert_eq!(score_candidate(&c, "someone else", "song", 101.0), 340.0);
    }

    #[test]
    fn clean_title_strips_noise() {
        assert_eq!(clean_title("Song (Remastered 2011)"), "Song");
        assert_eq!(clean_title("Song feat. Someone"), "Song");
        assert_eq!(clean_title("Song"), "Song");
    }
}
