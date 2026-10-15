//! `rocky statusline` 의 렌더 — 경로·git 줄과 모델·ctx·한도 줄. cc-usage `internal/render` 에서 옮겨 왔고 지금은
//! rocky 가 정본이다. 출력은 골든 픽스처(`tests/fixtures/statusline-full/`)가 ANSI 까지 바이트로 고정한다.

use chrono::{DateTime, Datelike, TimeDelta, TimeZone, Timelike, Utc};

use super::git::GitStatus;
use super::width::display_width;
use crate::limits::{Alert, AlertLevel, CreditView, Limits, UsageCache, Window, STALE_AFTER};

/// 색을 어디까지 쓰나 — `NO_COLOR` · `COLORTERM` · `TERM` 으로 정한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Style {
    pub color: bool,
    /// 24bit 그라데이션.
    pub true_color: bool,
    /// 가운데 단계 — statusline 프로세스에는 `COLORTERM` 이 오지 않고 `TERM` 만 온다(실측).
    pub color256: bool,
    /// 터미널 폭(`COLUMNS`), 0 이면 모름 — statusline 은 stdout 이 파이프라 직접 잴 수 없다.
    pub width: usize,
}

impl Style {
    pub fn from_env(get: impl Fn(&str) -> Option<String>) -> Style {
        let colorterm = get("COLORTERM").unwrap_or_default();
        Style {
            color: get("NO_COLOR").unwrap_or_default().is_empty(),
            true_color: colorterm == "truecolor" || colorterm == "24bit",
            color256: get("TERM").unwrap_or_default().contains("256color"),
            // 없거나 읽을 수 없거나 음수면 0 — 폭 판단을 건너뛴다.
            width: get("COLUMNS")
                .and_then(|c| c.parse::<i64>().ok())
                .map_or(0, |w| w.max(0) as usize),
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

    /// 크레딧 금액 색 — 사용률 색(`pct_color`)을 `glow` 만큼 진하게. 0 이면 회색 쪽으로 반쯤 섞은 옅은 색, 1 이면 원래 색.
    /// 색을 섞을 수 없는 터미널(24bit·256 아님)은 1 이 아니면 회색이다. `None` 이면 원래 색 그대로.
    fn credit_tone(&self, used: f64, glow: Option<f64>) -> String {
        let Some(glow) = glow else {
            return self.pct_color(used);
        };
        let base = gradient_rgb(used);
        let pale = mix(base, (128, 128, 128), 0.5);
        match self.rgb(mix(pale, base, glow)) {
            Some(c) => c,
            None if glow >= 1.0 => self.pct_color(used),
            None => DIM.to_string(),
        }
    }

    /// ctx 색 — 한도와 다른 축(파랑). 차오를수록 밝아지되 뒤로 몰아서 밝아지고, 자동 압축이 다가오면(`CTX_ALERT_AT`)
    /// 굵은 금색으로 바뀐다 — 파랑이 밝아지는 것만으로는 눈에 띄지 않는다.
    fn ctx_color(&self, p: f64) -> String {
        let p = p.clamp(0.0, 100.0);
        // 그리는 숫자(반올림)로 견준다 — `90%` 로 보이는데 파랑이면 안 된다.
        if p.round() >= CTX_ALERT_AT {
            return match self.rgb(CTX_ALERT_RGB) {
                Some(c) => format!("{BOLD}{c}"),
                None => CTX_ALERT.to_string(),
            };
        }
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
const CTX_ALERT: &str = "\x1b[1;93m";
const CTX_ALERT_RGB: (i32, i32, i32) = (255, 215, 0);
const MAGENTA: &str = "\x1b[1;35m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const RED_BG: &str = "\x1b[41;97m";
const BOLD: &str = "\x1b[1m";

/// statusline 이 쓰지 않고 비워 두는 오른쪽 칸 — Claude Code 가 그 자리에 배지·알림을 얹는다(실측 38칸 + 여유).
const RIGHT_MARGIN: usize = 40;

const CTX_CURVE: f64 = 2.2;
const CTX_MID_AT: f64 = 80.0;
/// ctx 를 강조하는 사용률. 1M 창의 자동 압축은 97% 언저리에서 일어난다(실측 967k~974k 토큰) — 그 전에 눈에 띄게.
/// 200k 창은 83% 언저리에서 압축돼 이 색까지 가지 않는다.
const CTX_ALERT_AT: f64 = 90.0;

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

/// `a` 에서 `b` 쪽으로 `t`(0~1)만큼.
fn mix(a: (i32, i32, i32), b: (i32, i32, i32), t: f64) -> (i32, i32, i32) {
    let t = t.clamp(0.0, 1.0);
    let one = |x: i32, y: i32| (x as f64 + (y - x) as f64 * t).round() as i32;
    (one(a.0, b.0), one(a.1, b.1), one(a.2, b.2))
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
    /// usage 캐시 — `None` 이면 한도를 다루지 않는다(`source: none`): 상태 문구도 크레딧도 없다.
    pub usage: Option<&'a UsageCache>,
    pub alert: Alert,
    pub credits: CreditView,
    /// 통화 기호.
    pub currency: &'a str,
    /// 로그인된 계정의 표시 — `None` 이면 없음(목록에 없는 계정).
    pub badge: Option<&'a Badge>,
    /// 크레딧 금액 색의 진하기(`limits::credit_glow`) — `None` 이면 페이드 없이 원래 색.
    pub credit_glow: Option<f64>,
    /// `~` 로 줄일 홈 디렉터리.
    pub home: Option<&'a str>,
    /// 경로를 감쌀 링크 템플릿(`statusline.dirLink`) — `None` 이면 링크 없이 경로만.
    pub dir_link: Option<&'a str>,
    /// 세션을 띄운 프로젝트 — `dir_link` 가 있으면 경로의 프로젝트 부분이 이 폴더를 열고, 지금 폴더가 프로젝트 밖이면
    /// 줄 끝에 `↗ 이름` 버튼을 단다. 비면 경로 전체가 지금 폴더를 연다.
    pub project_dir: &'a str,
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
    let note = status_note(v, s);
    if !note.is_empty() {
        parts.push(note);
    }

    let mut out = Vec::new();
    let dir = dir_line(v, s);
    if !dir.is_empty() {
        out.push(dir);
    }
    // 크레딧은 평소엔 상태 줄 끝에 붙는다 — statusline 의 세로 칸이 곧 프롬프트가 밀리는 양이다.
    let sep = s.c(DIM, " · ");
    let mut row = parts.join(&sep);
    let credit = credit_line(v, s);
    // 배지는 세그먼트가 아니라 머리표다 — 구분자 없이 크레딧까지 다 이은 **뒤에** 앞에 붙인다. 폭 판단에는 따로 넘긴다.
    let badge = badge_text(v.badge, s);
    let standalone =
        !credit.is_empty() && credit_standalone(v, s, &row, &credit, &sep, display_width(&badge));
    if !credit.is_empty() && !standalone {
        if row.is_empty() {
            row = credit.clone();
        } else {
            row = format!("{row}{sep}{credit}");
        }
    }
    // 나머지가 다 비어도 배지는 낸다 — 정보가 가장 적은 순간에 "여기는 평소 자리가 아니다" 신호가 사라지면 안 된다.
    if !badge.is_empty() {
        row = if row.is_empty() {
            badge
        } else {
            format!("{badge} {row}")
        };
    }
    if !row.is_empty() {
        out.push(row);
    }
    if standalone {
        out.push(credit);
    }
    if out.is_empty() {
        // 모든 세그먼트가 비어도 무엇이 도는지는 보이게 한다.
        out.push(s.c(DIM, "[rocky]"));
    }
    out
}

/// 크레딧을 제 줄로 내리나. 강조가 붙는 상태(소진 중 · 한도 소진 · 조회 중 · 비활성)는 늘 내리고 — 줄이 하나
/// 느는 것 자체가 신호다 —, 폭을 알면 붙였을 때 오른쪽 여백(`RIGHT_MARGIN`)을 침범하는지 본다.
fn credit_standalone(
    v: &View,
    s: &Style,
    row: &str,
    credit: &str,
    sep: &str,
    badge_width: usize,
) -> bool {
    let cv = &v.credits;
    if !cv.enabled || cv.spending || v.limits.exhausted() {
        return true;
    }
    if s.width == 0 {
        return false;
    }
    let mut w = display_width(row) + display_width(credit) + badge_width;
    if badge_width > 0 {
        w += 1; // 배지 뒤의 공백
    }
    if !row.is_empty() {
        w += display_width(sep);
    }
    w + RIGHT_MARGIN > s.width
}

/// 크레딧 줄 — **남은 금액**(한도 − 사용). 한도를 모르면 쓴 금액임을 밝힌다.
fn credit_line(v: &View, s: &Style) -> String {
    let cv = &v.credits;
    if !cv.show {
        return String::new();
    }
    if !cv.enabled {
        if cv.disabled {
            return s.c(DIM, "크레딧 비활성 — 한도 reset까지 대기");
        }
        return s.c(YELLOW, "한도 소진 · 크레딧 조회 중…");
    }
    let cur = v.currency;
    let mut t = match cv.limit.filter(|l| *l > 0.0) {
        Some(limit) => {
            let tone = s.credit_tone(cv.used / limit * 100.0, v.credit_glow);
            s.c(&tone, &money(cur, limit - cv.used))
                + &s.c(DIM, &format!(" ({})", money_short(cur, limit)))
        }
        None => s.c(DIM, &money(cur, cv.used)) + &s.c(DIM, " 사용"),
    };
    let mut tail = String::new();
    if cv.spent_window > 0.0 {
        tail += &format!(" · 이번 window +{}", money(cur, cv.spent_window));
    }
    if cv.spending {
        t += &s.c(&format!("{BOLD}{RED}"), &format!("{tail} · 크레딧 소진 중"));
    } else if v.limits.exhausted() {
        t += &s.c(YELLOW, &format!("{tail} · 다음 prompt부터 크레딧 사용"));
    } else {
        t += &s.c(DIM, &tail);
    }
    t
}

/// 금액 — 매 렌더 바뀌는 값이라 소수점을 늘 둔다(폭이 흔들리지 않게).
fn money(cur: &str, v: f64) -> String {
    format!("{cur}{:.2}", v.max(0.0))
}

/// 한도처럼 잘 안 바뀌는 값 — `.00` 을 뗀다.
fn money_short(cur: &str, v: f64) -> String {
    let v = v.max(0.0);
    if v == v.trunc() {
        format!("{cur}{v:.0}")
    } else {
        format!("{cur}{v:.2}")
    }
}

/// usage 캐시의 상태 — 조회가 실패하는 중이면 그 이유, 아직 응답이 없으면 "usage …", 응답이 30분 넘게 묵었으면 stale.
/// stdin 쪽에서는 한도가 소진됐을 때(크레딧을 봐야 할 때)만 낸다.
fn status_note(v: &View, s: &Style) -> String {
    let Some(cache) = v.usage else {
        return String::new();
    };
    if v.limits.from_stdin && !v.limits.exhausted() {
        return String::new();
    }
    match &cache.usage {
        None if !cache.last_error.is_empty() => {
            s.c(YELLOW, &format!("usage: {}", short_err(&cache.last_error)))
        }
        None if !v.limits.from_stdin => s.c(DIM, "usage …"),
        Some(usage) => {
            // fetched_at 이 없으면 Go 의 zero 시각 — Go 는 그 차이를 i64 나노초 최대(약 292년)에서 포화시킨다.
            let age = usage
                .fetched_at
                .map_or(TimeDelta::nanoseconds(i64::MAX), |t| v.now - t);
            if age > STALE_AFTER {
                s.c(YELLOW, &format!("⚠︎ stale {}", duration(age)))
            } else {
                String::new()
            }
        }
        None => String::new(),
    }
}

/// 에러 문구를 40바이트에서 자른다. 글자 중간에서 자르지는 않는다.
fn short_err(e: &str) -> String {
    if e.len() <= 40 {
        return e.to_string();
    }
    format!("{}…", &e[..e.floor_char_boundary(40)])
}

/// 로그인된 계정을 알아보는 표시 — `rocky.json` 의 `statusline.badges` 에 이메일을 키로 적는다.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Badge {
    /// 있으면 이것만 쓴다(이모지는 제 색이 있다).
    pub emoji: String,
    /// 없으면 `●`.
    pub glyph: String,
    /// 이름(`blue` · `brightblue` · `cyan` · `green` · `yellow` · `magenta` · `red` · `gray` · `white`) 또는 256 인덱스.
    pub color: String,
}

fn badge_text(badge: Option<&Badge>, s: &Style) -> String {
    let Some(b) = badge else {
        return String::new();
    };
    if !b.emoji.is_empty() {
        return b.emoji.clone();
    }
    let glyph = if b.glyph.is_empty() { "●" } else { &b.glyph };
    s.c(&badge_color(&b.color), glyph)
}

/// 이름 또는 256 인덱스 — 모르는 값이면 색 없이(설정 오타로 글리프가 사라지는 것보다 낫다).
fn badge_color(v: &str) -> String {
    // Go 의 Atoi 처럼 정수로 읽고 0~255 만 받는다("-0"·"+33"·"033" 도 같게).
    if let Some(n) = v.parse::<i64>().ok().filter(|n| (0..=255).contains(n)) {
        return format!("\x1b[38;5;{n}m");
    }
    match v.to_lowercase().as_str() {
        "blue" => "\x1b[34m",
        "brightblue" => "\x1b[94m",
        "cyan" => "\x1b[36m",
        "green" => "\x1b[32m",
        "yellow" => "\x1b[33m",
        "magenta" => "\x1b[35m",
        "red" => "\x1b[31m",
        "gray" | "grey" => DIM,
        "white" => "\x1b[97m",
        _ => "",
    }
    .to_string()
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
    let link = |dir: &str| v.dir_link.and_then(|tpl| dir_link_url(tpl, dir));
    let project_url = (!v.project_dir.is_empty())
        .then(|| link(v.project_dir))
        .flatten();
    // 지금 폴더가 세션을 띄운 프로젝트 안이면 경로를 둘로 나눠 건다 — 앞부분은 프로젝트를, 뒤의 하위 경로는 지금 폴더를
    // 연다. 누른 글자가 곧 여는 경로라 글자와 대상이 어긋나지 않는다.
    let rest = (!v.project_dir.is_empty())
        .then(|| v.dir.strip_prefix(v.project_dir))
        .flatten()
        .filter(|rest| rest.is_empty() || rest.starts_with('/'));
    let mut t = match (rest, &project_url) {
        (Some(rest), Some(url)) => {
            let mut t = hyperlink(url, &s.c(CYAN, &abbrev_home(v.project_dir, v.home)));
            if !rest.is_empty() {
                let sub = s.c(CYAN, rest);
                t += &match link(v.dir) {
                    Some(url) => hyperlink(&url, &sub),
                    None => sub,
                };
            }
            t
        }
        _ => {
            let path = s.c(CYAN, &abbrev_home(v.dir, v.home));
            match link(v.dir) {
                Some(url) => hyperlink(&url, &path),
                None => path,
            }
        }
    };
    let branch = branch_text(v.git, s);
    if !branch.is_empty() {
        t += &s.c(DIM, " · ");
        t += &branch;
    }
    // 프로젝트 밖(다른 워크트리 등)으로 나가면 경로로는 프로젝트를 열 수 없다 — 줄 끝에 프로젝트 이름을 단 버튼을 둔다.
    if let (None, Some(url)) = (rest, &project_url) {
        let name = std::path::Path::new(v.project_dir)
            .file_name()
            .map_or(v.project_dir.into(), |n| n.to_string_lossy());
        t += "  ";
        t += &hyperlink(url, &format!("{} {}", s.c(DIM, "↗"), s.c(CYAN, &name)));
    }
    t
}

/// OSC 8 — Claude Code 가 그대로 넘기고, 터미널(fullscreen 이면 Claude Code)이 Cmd+클릭으로 연다. 색은 링크 안에 둔다.
fn hyperlink(url: &str, text: &str) -> String {
    format!("\x1b]8;;{url}\x07{text}\x1b]8;;\x07")
}

/// 경로 링크 — 템플릿의 `{{cwd}}` 자리에 퍼센트 인코딩한 경로(`/` 는 그대로)를 넣는다. `{{cwd}}` 가 없으면(오타) `None` —
/// 모든 세션이 같은 엉뚱한 주소로 열린다. 결과가 공백·제어 문자·ASCII 밖의 글자를 담아도 `None` — OSC 8 의 URI 는 출력
/// 가능한 ASCII 여야 하고, 섞이면 줄 전체가 깨진다.
pub fn dir_link_url(template: &str, dir: &str) -> Option<String> {
    use std::fmt::Write;
    if !template.contains("{{cwd}}") {
        return None;
    }
    let mut path = String::with_capacity(dir.len());
    for b in dir.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                path.push(b as char)
            }
            _ => {
                let _ = write!(path, "%{b:02X}");
            }
        }
    }
    let url = template.replace("{{cwd}}", &path);
    url.bytes().all(|b| b.is_ascii_graphic()).then_some(url)
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
    } else if v.alert.burst && !v.alert.on {
        // 깜빡임의 꺼진 프레임 — 여백은 남긴다(프레임마다 폭이 바뀌면 줄 전체가 좌우로 출렁인다).
        s.c(&format!("{BOLD}{RED}"), &format!(" {pct} "))
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

    #[test]
    fn money_short_drops_zero_fraction_only() {
        for (v, want) in [
            (100.0, "$100"),
            (100.5, "$100.50"),
            (0.0, "$0"),
            (33.33, "$33.33"),
        ] {
            assert_eq!(money_short("$", v), want);
        }
        assert_eq!(money("$", 100.0), "$100.00");
        assert_eq!(money("$", -3.0), "$0.00");
    }

    #[test]
    fn ctx_turns_bold_gold_from_the_shown_90() {
        let paths = [
            (
                Style {
                    color: true,
                    ..Default::default()
                },
                "\x1b[1;93m",
            ),
            (
                Style {
                    color: true,
                    color256: true,
                    ..Default::default()
                },
                "\x1b[1m\x1b[38;5;220m",
            ),
            (
                Style {
                    color: true,
                    true_color: true,
                    ..Default::default()
                },
                "\x1b[1m\x1b[38;2;255;215;0m",
            ),
        ];
        for (s, want) in paths {
            // 89.5 는 `90%` 로 그려진다 — 보이는 숫자와 색이 같이 바뀌어야 한다.
            assert_eq!(format!("{:.0}%", 89.5), "90%");
            for p in [89.5, 90.0, 97.0, 100.0, 140.0] {
                assert_eq!(s.ctx_color(p), want, "ctx {p} · {s:?}");
            }
            for p in [0.0, 41.0, 85.0, 89.4] {
                assert_ne!(s.ctx_color(p), want, "ctx {p} 는 아직 파랑 · {s:?}");
            }
        }
    }
}
