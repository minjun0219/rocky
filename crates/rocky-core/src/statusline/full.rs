//! `rocky statusline --full` 의 렌더 — 경로·git 줄과 모델·ctx·한도 줄. cc-usage `internal/render` 의 이식이고,
//! 같은 입력이면 ANSI 까지 같은 바이트를 낸다(골든 픽스처 `tests/fixtures/cc-usage/`). 표기·색을 고르는 이유는
//! cc-usage 쪽 주석과 README 에 있다 — 여기는 동작만 옮겨 둔다.

use chrono::{DateTime, Datelike, TimeDelta, TimeZone, Timelike, Utc};

use super::git::GitStatus;
use crate::limits::{Alert, AlertLevel, Limits, Tracking, Window};

/// 색을 어디까지 쓰나 — `NO_COLOR` · `COLORTERM` · `TERM` 으로 정한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Style {
    pub color: bool,
    /// 24bit 그라데이션.
    pub true_color: bool,
    /// 가운데 단계 — statusline 프로세스에는 `COLORTERM` 이 오지 않고 `TERM` 만 온다(실측).
    pub color256: bool,
}

impl Style {
    pub fn from_env(get: impl Fn(&str) -> Option<String>) -> Style {
        let colorterm = get("COLORTERM").unwrap_or_default();
        Style {
            color: get("NO_COLOR").unwrap_or_default().is_empty(),
            true_color: colorterm == "truecolor" || colorterm == "24bit",
            color256: get("TERM").unwrap_or_default().contains("256color"),
        }
    }

    fn c(&self, code: &str, text: &str) -> String {
        if !self.color || text.is_empty() || code.is_empty() {
            return text.to_string();
        }
        format!("{code}{text}{RESET}")
    }

    fn rgb(&self, (r, g, b): (i32, i32, i32)) -> Option<String> {
        if self.true_color {
            Some(format!("\x1b[38;2;{r};{g};{b}m"))
        } else if self.color256 {
            Some(format!("\x1b[38;5;{}m", cube256(r, g, b)))
        } else {
            None
        }
    }

    /// 한도 창 색 — 사용률이 오를수록 green → red.
    fn pct_color(&self, used: f64) -> String {
        if let Some(c) = self.rgb(gradient_rgb(used)) {
            return c;
        }
        match used {
            p if p >= 90.0 => RED,
            p if p >= 70.0 => YELLOW,
            _ => GREEN,
        }
        .to_string()
    }

    /// ctx 색 — 한도와 다른 축(파랑). 차오를수록 밝아지되 뒤로 몰아서 밝아진다.
    fn ctx_color(&self, p: f64) -> String {
        let p = p.clamp(0.0, 100.0);
        let t = (p / 100.0).powf(CTX_CURVE);
        let rgb = (
            (75.0 + 45.0 * t) as i32,
            (95.0 + 95.0 * t) as i32,
            (130.0 + 125.0 * t) as i32,
        );
        if let Some(c) = self.rgb(rgb) {
            return c;
        }
        match p {
            p if p >= CTX_HIGH_AT => CTX_HIGH,
            p if p >= CTX_MID_AT => CTX_MID,
            _ => CTX_LOW,
        }
        .to_string()
    }
}

const RESET: &str = "\x1b[0m";
const DIM: &str = "\x1b[90m";
const GREEN: &str = "\x1b[32m";
const CYAN: &str = "\x1b[36m";
const BLUE: &str = "\x1b[1;34m";
const CTX_LOW: &str = "\x1b[34m";
const CTX_MID: &str = "\x1b[94m";
const CTX_HIGH: &str = "\x1b[96m";
const MAGENTA: &str = "\x1b[1;35m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const RED_BG: &str = "\x1b[41;97m";

const CTX_CURVE: f64 = 2.2;
const CTX_MID_AT: f64 = 80.0;
const CTX_HIGH_AT: f64 = 95.0;

/// 7d 세그먼트가 나타나는 사용률 — 색이 노래지기 시작하는 지점과 같다.
const SEVEN_DAY_SHOW_AT: f64 = 70.0;

/// 6×6×6 색 큐브(16-231). 축은 등간격이 아니라 0·95·135·175·215·255 다.
fn cube256(r: i32, g: i32, b: i32) -> i32 {
    16 + 36 * cube_axis(r) + 6 * cube_axis(g) + cube_axis(b)
}

fn cube_axis(v: i32) -> i32 {
    const LEVELS: [i32; 6] = [0, 95, 135, 175, 215, 255];
    let mut best = (0, i32::MAX);
    for (i, l) in LEVELS.iter().enumerate() {
        let d = (v - l).abs();
        if d < best.1 {
            best = (i as i32, d);
        }
    }
    best.0
}

fn gradient_rgb(used: f64) -> (i32, i32, i32) {
    let t = (100.0 - used.clamp(0.0, 100.0)) / 100.0; // 남은 비율
    (
        (230.0 * (1.0 - t).powf(0.7)) as i32,
        (200.0 * t.powf(0.6)) as i32,
        (30.0 * t) as i32,
    )
}

/// 한 번 그릴 재료.
#[derive(Debug, Clone)]
pub struct View<'a> {
    /// 빈 값이면 경로 줄을 생략한다.
    pub dir: &'a str,
    /// `None` 이면 git repo 가 아니거나 조회 실패 — 세그먼트 생략.
    pub git: Option<&'a GitStatus>,
    pub model: &'a str,
    pub effort: &'a str,
    pub context_pct: Option<f64>,
    pub limits: Limits,
    pub tracking: Tracking,
    pub alert: Alert,
    /// `~` 로 줄일 홈 디렉터리.
    pub home: Option<&'a str>,
    pub now: DateTime<Utc>,
}

/// statusline 줄들 — 경로 줄(있으면), 상태 줄, 크레딧 줄(있으면). 리셋 시각은 `tz` 로 그린다.
pub fn lines<Tz: TimeZone>(v: &View, s: &Style, tz: &Tz) -> Vec<String> {
    let mut parts = Vec::new();
    let model = model_text(v, s);
    if !model.is_empty() {
        parts.push(model);
    }
    if let Some(p) = v.context_pct {
        parts.push(format!("ctx {}", s.c(&s.ctx_color(p), &format!("{p:.0}%"))));
    }
    if let Some(w) = &v.limits.five_hour {
        parts.push(window_text("5h", w, v, s, tz));
    }
    if let Some(w) = &v.limits.seven_day {
        if w.percent >= SEVEN_DAY_SHOW_AT || v.alert.window == "7d" {
            parts.push(window_text("7d", w, v, s, tz));
        }
    }
    if v.tracking == Tracking::Empty && !v.limits.from_stdin {
        parts.push(s.c(DIM, "usage …"));
    }

    let mut out = Vec::new();
    let dir = dir_line(v, s);
    if !dir.is_empty() {
        out.push(dir);
    }
    let row = parts.join(&s.c(DIM, " · "));
    if !row.is_empty() {
        out.push(row);
    }
    // 한도가 소진됐는데 크레딧을 아직 모른다 — 줄이 하나 느는 것 자체가 신호라 제 줄로 낸다.
    if v.tracking == Tracking::Empty && v.limits.exhausted() {
        out.push(s.c(YELLOW, "한도 소진 · 크레딧 조회 중…"));
    }
    if out.is_empty() {
        // 모든 세그먼트가 비어도 무엇이 도는지는 보이게 한다.
        out.push(s.c(DIM, "[rocky]"));
    }
    out
}

/// 모델 이름 뒤에 effort 를 흐리게 붙인다 — effort 는 모델의 속성이라 세그먼트로 떼지 않는다.
fn model_text(v: &View, s: &Style) -> String {
    let m = s.c(MAGENTA, v.model);
    let e = s.c(DIM, v.effort);
    match (m.is_empty(), e.is_empty()) {
        (true, _) => e,
        (_, true) => m,
        _ => format!("{m} {e}"),
    }
}

fn dir_line(v: &View, s: &Style) -> String {
    if v.dir.is_empty() {
        return String::new();
    }
    let mut t = s.c(CYAN, &abbrev_home(v.dir, v.home));
    let branch = branch_text(v.git, s);
    if !branch.is_empty() {
        t += &s.c(DIM, " · ");
        t += &branch;
    }
    t
}

/// `⎇ main =1 +3 !5 ⇡1⇣2` — 충돌·staged·unstaged·ahead/behind. detached 면 `@<7자리>`.
fn branch_text(st: Option<&GitStatus>, s: &Style) -> String {
    let Some(st) = st.filter(|st| !st.branch.is_empty()) else {
        return String::new();
    };
    let name = if st.detached {
        st.oid
            .get(..7)
            .map_or_else(|| "(detached)".to_string(), |oid| format!("@{oid}"))
    } else {
        st.branch.clone()
    };
    let mut t = format!("{} {}", s.c(BLUE, "⎇"), s.c(GREEN, &name));
    for (n, glyph, color) in [
        (st.conflicted, "=", RED),
        (st.staged, "+", GREEN),
        (st.unstaged, "!", YELLOW),
    ] {
        if n > 0 {
            t += &format!(" {}", s.c(color, &format!("{glyph}{n}")));
        }
    }
    // 업스트림이 없으면 ahead/behind 를 내지 않는다 — 없는 것을 0 으로 보이면 최신으로 읽힌다.
    if st.has_upstream {
        let mut sync = String::new();
        if st.ahead > 0 {
            sync += &format!("⇡{}", st.ahead);
        }
        if st.behind > 0 {
            sync += &format!("⇣{}", st.behind);
        }
        if !sync.is_empty() {
            t += &format!(" {}", s.c(YELLOW, &sync));
        }
    }
    t
}

/// 앞쪽 홈 디렉터리를 `~` 로.
pub fn abbrev_home(p: &str, home: Option<&str>) -> String {
    let Some(home) = home.filter(|h| !h.is_empty()) else {
        return p.to_string();
    };
    if p == home {
        return "~".to_string();
    }
    match p.strip_prefix(home) {
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => p.to_string(),
    }
}

/// 한도 창 하나 — **남은 비율**(100 − 사용률)과 언제 풀리나. 색은 사용률로 고른다.
fn window_text<Tz: TimeZone>(name: &str, w: &Window, v: &View, s: &Style, tz: &Tz) -> String {
    let pct = format!("{:.0}%", 100.0 - w.percent);
    let pct = if v.alert.level == AlertLevel::None || v.alert.window != name {
        s.c(&s.pct_color(w.percent), &pct)
    } else {
        // 경보를 올린 창은 배지 — 양옆 한 칸은 이웃 글자에 붙어 답답해 보이는 것을 막는다.
        s.c(RED_BG, &format!(" {pct} "))
    };
    let mut t = format!("{name} {pct}");
    if let Some(at) = w.resets_at.filter(|at| *at > v.now) {
        t += &format!(" {}", s.c(DIM, &format!("({})", reset_text(at, v.now, tz))));
    }
    t
}

/// 오늘 안이면 `↻14:40`, 날이 바뀌면 남은 시간. 기준은 24시간이 아니라 달력 날짜다.
fn reset_text<Tz: TimeZone>(at: DateTime<Utc>, now: DateTime<Utc>, tz: &Tz) -> String {
    let (a, n) = (at.with_timezone(tz), now.with_timezone(tz));
    if a.year() != n.year() || a.ordinal() != n.ordinal() {
        return duration(at - now);
    }
    format!("↻{:02}:{:02}", a.hour(), a.minute())
}

/// `45m` · `1h 20m` · `2d 4h` — 0 인 아랫단위는 뗀다.
pub fn duration(d: TimeDelta) -> String {
    if d < TimeDelta::minutes(1) {
        return "<1m".to_string();
    }
    let m = d.num_minutes();
    let (days, hours, mins) = (m / 1440, (m % 1440) / 60, m % 60);
    match (days, hours, mins) {
        (d, h, _) if d > 0 && h > 0 => format!("{d}d {h}h"),
        (d, _, _) if d > 0 => format!("{d}d"),
        (_, h, m) if h > 0 && m > 0 => format!("{h}h {m}m"),
        (_, h, _) if h > 0 => format!("{h}h"),
        (_, _, m) => format!("{m}m"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_axis_picks_nearest_level() {
        for (input, want) in [
            (0, 0),
            (40, 0),
            (60, 1),
            (95, 1),
            (120, 2),
            (175, 3),
            (255, 5),
        ] {
            assert_eq!(cube_axis(input), want, "cube_axis({input})");
        }
        assert_eq!(cube256(0, 0, 0), 16);
        assert_eq!(cube256(255, 255, 255), 231);
    }
}
