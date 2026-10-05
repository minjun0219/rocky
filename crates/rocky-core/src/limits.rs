//! 한도(5h/7d) 판정 — cc-usage `internal/core` 의 Rust 이식. 같은 입력이면 같은 판정을 낸다는 것이 계약이고,
//! 골든 픽스처(`tests/fixtures/cc-usage/`)가 고정한다. 설계: `docs/design/specs/2026-10-05-cc-usage-mirror-design.md`.
//!
//! 순수 함수만 둔다 — "지금" 은 인자로 받는다. 이름이 `usage` 가 아닌 것은 사용 로그(`crate::usage`)와 겹쳐서다.
//!
//! 지금은 캐시(usage API 응답·state)가 없는 경로까지만 옮겼다. 캐시를 읽는 판정은 그 캐시를 쓰는 조각에서 더한다.

use chrono::{DateTime, Utc};
use serde_json::Value;

/// 한도를 어디서 읽나 — cc-usage 설정의 `source` 와 같은 값.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Source {
    /// stdin 에 `rate_limits` 가 있으면 stdin, 없으면 usage API.
    #[default]
    Auto,
    Stdin,
    Api,
    /// 한도를 다루지 않는다 — 경로·git·모델·ctx 만.
    None,
}

impl Source {
    /// 모르는 값은 `None`(Option) — 호출자가 기본값으로 둘지 정한다.
    pub fn parse(v: &str) -> Option<Source> {
        match v {
            "auto" => Some(Source::Auto),
            "stdin" => Some(Source::Stdin),
            "api" => Some(Source::Api),
            "none" => Some(Source::None),
            _ => None,
        }
    }
}

/// 판정에 쓰는 설정 — cc-usage `config.json` 중 이 모듈이 읽는 필드.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LimitsConfig {
    pub source: Source,
    /// "임박" 임계. 없으면 90, `0` 이면 임박 경고를 끈다(소진 강조는 남는다).
    pub alert_percent: Option<f64>,
}

impl LimitsConfig {
    /// 실제로 쓰는 임박 임계 — 범위를 벗어나면 끈 것(0)으로 본다.
    pub fn alert(&self) -> f64 {
        match self.alert_percent {
            None => 90.0,
            Some(p) if !(0.0..=100.0).contains(&p) => 0.0,
            Some(p) => p,
        }
    }
}

/// 한도 창 하나. `percent` 는 **사용률**(0~100)이다 — 화면은 남은 비율(100 − 사용률)을 낸다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
    pub percent: f64,
    pub resets_at: Option<DateTime<Utc>>,
}

/// Claude Code statusline 입력(stdin JSON) 중 쓰는 것. 필드마다 따로 읽어서, 타입이 어긋난 필드만 비고
/// 나머지는 채워진다 — 호스트가 달라도 줄이 통째로 비지 않는다.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Input {
    pub session_id: String,
    pub cwd: String,
    pub current_dir: String,
    pub model: String,
    /// `effort.level` — 모델이 effort 를 지원할 때만 온다.
    pub effort: String,
    pub context_pct: Option<f64>,
    pub five_hour: Option<Window>,
    pub seven_day: Option<Window>,
}

impl Input {
    /// 비었거나 JSON 이 아니면 빈 입력.
    pub fn parse(raw: &str) -> Input {
        let Ok(v) = serde_json::from_str::<Value>(raw) else {
            return Input::default();
        };
        let text = |p: &str| {
            v.pointer(p)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        Input {
            session_id: text("/session_id"),
            cwd: text("/cwd"),
            current_dir: text("/workspace/current_dir"),
            model: text("/model/display_name"),
            effort: text("/effort/level"),
            context_pct: v
                .pointer("/context_window/used_percentage")
                .and_then(go_number),
            five_hour: v.pointer("/rate_limits/five_hour").and_then(window),
            seven_day: v.pointer("/rate_limits/seven_day").and_then(window),
        }
    }

    /// 그릴 작업 디렉터리 — `workspace.current_dir`, 없으면 `cwd`.
    pub fn dir(&self) -> &str {
        if self.current_dir.is_empty() {
            &self.cwd
        } else {
            &self.current_dir
        }
    }
}

/// Go `*float64` 필드가 JSON 을 받는 방식 — `null` 이면 없음, 숫자면 그 값, **다른 타입이면 0**. Go 디코더가
/// 포인터를 먼저 할당하고 나서 타입 에러를 내기 때문이다(`"41"` → `ctx 0%`). cc-usage 와 같은 출력을 내려고
/// 따라간다(픽스처 `type-mismatch`).
fn go_number(v: &Value) -> Option<f64> {
    match v {
        Value::Null => None,
        other => Some(other.as_f64().unwrap_or(0.0)),
    }
}

fn window(v: &Value) -> Option<Window> {
    let mut p = go_number(v.get("used_percentage")?)?;
    // epoch 타임스탬프가 used_percentage 로 새어 들어온 적이 있다 — 말이 안 되는 값은 버린다.
    if !(0.0..=200.0).contains(&p) {
        return None;
    }
    if p > 100.0 {
        p = 100.0;
    }
    Some(Window {
        percent: p,
        resets_at: v.get("resets_at").and_then(parse_resets),
    })
}

/// epoch 초(문서), 밀리초, RFC3339 문자열, 숫자 문자열을 받는다.
fn parse_resets(v: &Value) -> Option<DateTime<Utc>> {
    let f = match v {
        Value::String(s) => {
            if let Ok(t) = DateTime::parse_from_rfc3339(s) {
                return Some(t.with_timezone(&Utc));
            }
            s.trim().parse::<f64>().ok()?
        }
        other => other.as_f64()?,
    };
    if f.is_nan() || f <= 0.0 {
        return None;
    }
    if f > 1e12 {
        return DateTime::from_timestamp_millis(f as i64);
    }
    DateTime::from_timestamp(f as i64, 0)
}

/// 지금 그릴 한도.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Limits {
    pub five_hour: Option<Window>,
    pub seven_day: Option<Window>,
    /// stdin 에서 왔나 — 아니면 usage API(캐시)에서.
    pub from_stdin: bool,
}

impl Limits {
    /// 소진된 창이 있나 — 7d 를 먼저 본다(같은 단계면 풀리는 데 더 오래 걸리는 쪽).
    pub fn exhausted(&self) -> bool {
        [self.seven_day, self.five_hour]
            .iter()
            .flatten()
            .any(|w| w.percent >= 100.0)
    }
}

/// 한도를 어떻게 다루나 — 렌더가 상태 문구·크레딧 줄을 고르는 근거.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tracking {
    /// 한도를 다루지 않는다(`source: none`) — 상태 문구도 크레딧도 없다.
    Off,
    /// 다루지만 usage API 응답이 아직 없다.
    Empty,
}

/// 설정과 입력으로 한도를 고른다. usage API 캐시가 아직 없는 상태의 cc-usage 와 같다 — `api` 이거나 `auto`
/// 인데 stdin 에 한도가 없으면 빈 한도(조회 대기)다.
pub fn select(cfg: &LimitsConfig, input: &Input, now: DateTime<Utc>) -> (Limits, Tracking) {
    let stdin_present = input.five_hour.is_some() || input.seven_day.is_some();
    let use_stdin = match cfg.source {
        Source::None => return (Limits::default(), Tracking::Off),
        Source::Stdin => true,
        Source::Api => false,
        Source::Auto => stdin_present,
    };
    if !use_stdin {
        return (Limits::default(), Tracking::Empty);
    }
    let lim = Limits {
        five_hour: drop_expired(input.five_hour, now),
        seven_day: drop_expired(input.seven_day, now),
        from_stdin: true,
    };
    (lim, Tracking::Empty)
}

/// 리셋 시각이 지난 창은 버린다 — 옛 사용률이 더는 맞지 않는다.
fn drop_expired(w: Option<Window>, now: DateTime<Utc>) -> Option<Window> {
    w.filter(|w| w.resets_at.is_none_or(|at| now <= at))
}

/// 경보 단계.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum AlertLevel {
    #[default]
    None,
    /// 임박 — `alert_percent` 이상.
    Near,
    /// 소진 — 100%.
    Over,
}

/// 강조할 창. 깜빡임(단계가 오른 직후 6초)은 그 시각을 저장해야 셀 수 있어서 아직 없다 — 배지로 고정한다.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Alert {
    pub level: AlertLevel,
    /// `"5h"` / `"7d"` — 단계가 `None` 이면 빈 문자열.
    pub window: &'static str,
}

/// 가장 급한 창 — 소진이 임박을 이기고, 같은 단계면 7d 가 5h 를 이긴다.
pub fn alert(cfg: &LimitsConfig, lim: &Limits) -> Alert {
    let mut best = Alert::default();
    for (name, w) in [("7d", lim.seven_day), ("5h", lim.five_hour)] {
        let Some(w) = w else { continue };
        let level = if w.percent >= 100.0 {
            AlertLevel::Over
        } else if cfg.alert() > 0.0 && w.percent >= cfg.alert() {
            AlertLevel::Near
        } else {
            AlertLevel::None
        };
        if level > best.level {
            best = Alert {
                level,
                window: name,
            };
        }
    }
    best
}
