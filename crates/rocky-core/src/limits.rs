//! 한도(5h/7d) 판정 — cc-usage `internal/core` 의 Rust 이식. 같은 입력이면 같은 판정을 낸다는 것이 계약이고,
//! 골든 픽스처(`tests/fixtures/cc-usage/`)가 고정한다. 설계: `docs/design/specs/2026-10-05-cc-usage-mirror-design.md`.
//!
//! 순수 함수만 둔다 — "지금" 은 인자로 받는다. 이름이 `usage` 가 아닌 것은 사용 로그(`crate::usage`)와 겹쳐서다.
//!
//! 캐시(`usage.json` = usage API 응답·크레딧 기준선·backoff, `state.json` = stdin 관측·갱신 기동 시각)는 cc-usage 와 **같은
//! JSON 모양**이다. 파일을 읽고 쓰는 일은 CLI 몫이고, 여기는 그 값으로 판정만 한다.

use chrono::{DateTime, TimeDelta, Timelike, Utc};
use serde::{Deserialize, Serialize};
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
    /// `used_credits` 단위 환산 — 없거나 0 이하면 100(cent 가정).
    pub credit_divisor: Option<f64>,
    /// 통화 기호 — 없거나 비면 `$`.
    pub currency: Option<String>,
    /// 크레딧이 0 이어도 줄을 낸다. stdin 모드에서는 한도 전에도 usage API 를 부르게 된다.
    pub always_show_credits: bool,
    /// usage API 폴링 주기(초) — 없거나 0 이하면 300. 여유 구간에서는 3배로 늘어난다.
    pub poll_seconds: Option<u64>,
    /// 한도 소진 뒤 크레딧 조회 주기(초) — 없거나 0 이하면 300.
    pub credit_poll_seconds: Option<u64>,
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

    pub fn poll(&self) -> TimeDelta {
        TimeDelta::seconds(self.poll_seconds.filter(|s| *s > 0).unwrap_or(300) as i64)
    }

    pub fn credit_poll(&self) -> TimeDelta {
        TimeDelta::seconds(self.credit_poll_seconds.filter(|s| *s > 0).unwrap_or(300) as i64)
    }

    pub fn credit_divisor(&self) -> f64 {
        self.credit_divisor.filter(|d| *d > 0.0).unwrap_or(100.0)
    }

    pub fn currency(&self) -> &str {
        self.currency
            .as_deref()
            .filter(|c| !c.is_empty())
            .unwrap_or("$")
    }
}

/// 한도 창 하나. `percent` 는 **사용률**(0~100)이다 — 화면은 남은 비율(100 − 사용률)을 낸다.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub percent: f64,
    #[serde(default, with = "go_time", skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<DateTime<Utc>>,
}

/// Go `time.Time` 의 JSON — RFC3339 문자열이고, zero 값(`0001-01-01T00:00:00Z`, `omitempty` 로도 빠지지 않는다)과
/// 읽을 수 없는 값은 없음으로 본다. 쓸 때는 없음을 아예 빼고 RFC3339(나노초)로 쓴다.
mod go_time {
    use chrono::{DateTime, Datelike, SecondsFormat, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(t: &Option<DateTime<Utc>>, s: S) -> Result<S::Ok, S::Error> {
        match t {
            Some(t) => s.serialize_str(&t.to_rfc3339_opts(SecondsFormat::AutoSi, true)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<DateTime<Utc>>, D::Error> {
        let raw = Option::<String>::deserialize(d).unwrap_or(None);
        Ok(raw
            .and_then(|r| DateTime::parse_from_rfc3339(&r).ok())
            .map(|t| t.with_timezone(&Utc))
            .filter(|t| t.year() > 1))
    }
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

/// stdin 이 이 시간 안에 한도를 보였으면 `auto` 는 stdin 쪽으로 판단한다.
const STDIN_RECENT: TimeDelta = TimeDelta::hours(6);
/// statusline 이 갱신을 다시 띄우기까지의 최소 간격 — 초당 두 번 도는 자리라 띄움이 쌓이지 않게.
const MIN_SPAWN_GAP: TimeDelta = TimeDelta::seconds(30);
/// 이보다 오래된 usage 응답은 "⚠︎ stale" 로 표시한다.
pub const STALE_AFTER: TimeDelta = TimeDelta::minutes(30);
/// 사용률이 이 아래면 여유 구간 — 폴링을 늘린다(색이 노래지는 지점과 같다).
const IDLE_BELOW: f64 = 70.0;
const IDLE_POLL_FACTOR: i32 = 3;
/// 계정 파일을 다시 읽는 최대 간격.
const ACCOUNT_TTL: TimeDelta = TimeDelta::minutes(1);

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
        self.exhausted_key().is_some()
    }

    /// 소진된 창의 키(`5h@2026-09-16T09:00:00Z`) — 크레딧 기준선이 어느 창의 것인지 가른다.
    pub fn exhausted_key(&self) -> Option<String> {
        [("7d", self.seven_day), ("5h", self.five_hour)]
            .into_iter()
            .find_map(|(name, w)| {
                w.filter(|w| w.percent >= 100.0)
                    .map(|w| window_key(name, &w))
            })
    }

    /// 가장 높은 사용률.
    pub fn peak(&self) -> f64 {
        [self.five_hour, self.seven_day]
            .iter()
            .flatten()
            .map(|w| w.percent)
            .fold(0.0, f64::max)
    }

    /// 한도 숫자의 지문(`70/-`) — 계정 파일을 다시 읽을지 정하는 데 쓴다.
    pub fn key(&self) -> String {
        let pct =
            |w: Option<Window>| w.map_or_else(|| "-".to_string(), |w| format!("{:.0}", w.percent));
        format!("{}/{}", pct(self.five_hour), pct(self.seven_day))
    }
}

fn window_key(name: &str, w: &Window) -> String {
    match w.resets_at {
        None => name.to_string(),
        Some(at) => {
            let at = at
                .with_second(0)
                .and_then(|t| t.with_nanosecond(0))
                .unwrap_or(at);
            format!("{name}@{}", at.format("%Y-%m-%dT%H:%M:%SZ"))
        }
    }
}

/// `usage.json` — usage API 응답과 그 주변(크레딧 기준선·실패·backoff). writer 는 갱신 프로세스 하나다.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UsageCache {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<CachedUsage>,
    #[serde(with = "go_time", skip_serializing_if = "Option::is_none")]
    pub last_attempt: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub last_error: String,
    #[serde(skip_serializing_if = "is_zero")]
    pub failures: u32,
    #[serde(with = "go_time", skip_serializing_if = "Option::is_none")]
    pub backoff_until: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev_credits: Option<f64>,
    /// `used_credits` 가 마지막으로 늘어난 시각.
    #[serde(with = "go_time", skip_serializing_if = "Option::is_none")]
    pub credits_rising_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline: Option<CreditBaseline>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// usage API 응답 한 번.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CachedUsage {
    #[serde(with = "go_time", skip_serializing_if = "Option::is_none")]
    pub fetched_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub five_hour: Option<Window>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seven_day: Option<Window>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<ExtraUsage>,
}

/// 응답의 `extra_usage` — 크레딧.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExtraUsage {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_credits: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monthly_limit: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utilization: Option<f64>,
}

/// 한도가 처음 소진된 순간의 `used_credits` — "이번 window 에서 쓴 크레딧" 의 기준.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CreditBaseline {
    pub window_key: String,
    pub credits: f64,
    #[serde(with = "go_time", skip_serializing_if = "Option::is_none")]
    pub at: Option<DateTime<Utc>>,
}

/// `state.json` — stdin 에서 본 한도, 갱신을 띄운 시각, 경보가 오른 시각. writer 는 statusline 하나다.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StateFile {
    /// stdin 한도를 마지막으로 본 시각과 그 값 — stdin 이 한 번 비어도 6시간 안이면 이 값을 그린다.
    #[serde(with = "go_time", skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<DateTime<Utc>>,
    /// `auto` 가 stdin 쪽인지 정하는 근거.
    #[serde(with = "go_time", skip_serializing_if = "Option::is_none")]
    pub stdin_limits_seen: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub five_hour: Option<Window>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seven_day: Option<Window>,
    #[serde(with = "go_time", skip_serializing_if = "Option::is_none")]
    pub spawned_at: Option<DateTime<Utc>>,
    /// 지금 무장한 경보(`<단계>@<창 키>`)와 그 단계가 오른 시각 — 거기서부터 6초를 깜빡인다.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub alert_key: String,
    #[serde(with = "go_time", skip_serializing_if = "Option::is_none")]
    pub alert_at: Option<DateTime<Utc>>,
}

/// 로그인된 계정 캐시 — 계정 파일(`.claude.json`)을 매 렌더 읽지 않으려고 둔다. rocky 는 `config_dir` 마다 하나다.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AccountCache {
    /// 이메일을 읽은 계정 파일 경로 — 다른 자리의 캐시면 쓰지 않는다.
    pub source: String,
    pub email: String,
    /// 읽었을 때의 한도 지문(`Limits::key`).
    pub at: String,
    #[serde(with = "go_time", skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<DateTime<Utc>>,
}

/// stdin 쪽으로 판단하나. `stdin`·`api` 는 설정대로, `auto` 는 지금 stdin 에 한도가 있거나 6시간 안에 본 적이 있으면.
pub fn use_stdin(
    cfg: &LimitsConfig,
    state: &StateFile,
    stdin_present: bool,
    now: DateTime<Utc>,
) -> bool {
    match cfg.source {
        Source::Stdin => true,
        Source::Api | Source::None => false,
        Source::Auto => {
            stdin_present
                || state
                    .stdin_limits_seen
                    .is_some_and(|t| now - t < STDIN_RECENT)
        }
    }
}

/// 한도를 고른다. `None` 이면 한도를 다루지 않는다(`source: none`) — 상태 문구도 크레딧도 없다.
/// stdin 쪽이면 지금 stdin → 6시간 안에 본 stdin(state) 순, 아니면 usage 캐시. 리셋이 지난 창은 버린다.
pub fn select(
    cfg: &LimitsConfig,
    input: &Input,
    state: &StateFile,
    usage: &UsageCache,
    now: DateTime<Utc>,
) -> Option<Limits> {
    if cfg.source == Source::None {
        return None;
    }
    let stdin_present = input.five_hour.is_some() || input.seven_day.is_some();
    let lim = if use_stdin(cfg, state, stdin_present, now) {
        let (five, seven) = if stdin_present {
            (input.five_hour, input.seven_day)
        } else if state.observed_at.is_some_and(|t| now - t < STDIN_RECENT) {
            (state.five_hour, state.seven_day)
        } else {
            (None, None)
        };
        Limits {
            five_hour: drop_expired(five, now),
            seven_day: drop_expired(seven, now),
            from_stdin: true,
        }
    } else {
        let cached = usage.usage.as_ref();
        Limits {
            five_hour: drop_expired(cached.and_then(|u| u.five_hour), now),
            seven_day: drop_expired(cached.and_then(|u| u.seven_day), now),
            from_stdin: false,
        }
    };
    Some(lim)
}

/// 리셋 시각이 지난 창은 버린다 — 옛 사용률이 더는 맞지 않는다.
fn drop_expired(w: Option<Window>, now: DateTime<Utc>) -> Option<Window> {
    w.filter(|w| w.resets_at.is_none_or(|at| now <= at))
}

/// usage API 폴링 간격 — 가장 급한 창이 소진이면 크레딧 주기, 70% 이상이면 기본, 여유면 3배(단 stale 경계를 넘지 않게).
pub fn poll_interval(cfg: &LimitsConfig, lim: &Limits) -> TimeDelta {
    let peak = lim.peak();
    if peak >= 100.0 {
        cfg.credit_poll()
    } else if peak >= IDLE_BELOW {
        cfg.poll()
    } else {
        (cfg.poll() * IDLE_POLL_FACTOR).min(cfg.poll().max(STALE_AFTER))
    }
}

/// statusline 이 갱신을 띄워야 하나. backoff 중이거나 방금 띄웠거나 방금 시도했으면 아니다. stdin 쪽은 한도가
/// 소진됐을 때(크레딧을 봐야 할 때)나 `always_show_credits` 일 때만 부른다.
pub fn need_refresh(
    cfg: &LimitsConfig,
    use_stdin: bool,
    lim: &Limits,
    state: &StateFile,
    usage: &UsageCache,
    now: DateTime<Utc>,
) -> bool {
    let recent = |t: Option<DateTime<Utc>>| t.is_some_and(|t| now - t < MIN_SPAWN_GAP);
    if usage.backoff_until.is_some_and(|t| now < t)
        || recent(state.spawned_at)
        || recent(usage.last_attempt)
    {
        return false;
    }
    let due = usage
        .usage
        .as_ref()
        .and_then(|u| u.fetched_at)
        .is_none_or(|fetched| now - fetched >= poll_interval(cfg, lim));
    if !use_stdin {
        return due;
    }
    (lim.exhausted() || cfg.always_show_credits) && due
}

/// usage API 응답 본문 → 캐시에 넣을 한 번의 응답. 필드는 모두 optional 이고, 타입이 틀리면 응답 전체를 실패로 본다.
pub fn parse_usage_response(body: &[u8], now: DateTime<Utc>) -> Result<CachedUsage, String> {
    #[derive(Deserialize)]
    struct RawWindow {
        utilization: Option<f64>,
        resets_at: Option<String>,
    }
    #[derive(Deserialize)]
    struct RawExtra {
        #[serde(default)]
        is_enabled: bool,
        monthly_limit: Option<f64>,
        used_credits: Option<f64>,
        utilization: Option<f64>,
    }
    #[derive(Deserialize)]
    struct Raw {
        five_hour: Option<RawWindow>,
        seven_day: Option<RawWindow>,
        extra_usage: Option<RawExtra>,
    }
    let raw: Raw = serde_json::from_slice(body).map_err(|e| format!("parse usage: {e}"))?;
    let window = |w: Option<RawWindow>| {
        let w = w?;
        Some(Window {
            percent: w.utilization?,
            resets_at: w
                .resets_at
                .and_then(|r| DateTime::parse_from_rfc3339(&r).ok())
                .map(|t| t.with_timezone(&Utc)),
        })
    };
    Ok(CachedUsage {
        fetched_at: Some(now),
        five_hour: window(raw.five_hour),
        seven_day: window(raw.seven_day),
        extra: raw.extra_usage.map(|e| ExtraUsage {
            enabled: e.is_enabled,
            used_credits: e.used_credits,
            monthly_limit: e.monthly_limit,
            utilization: e.utilization,
        }),
    })
}

/// 갱신 성공 — 응답을 넣고 실패 기록을 지우고, 크레딧이 늘었는지(소진 중)와 한도 소진 창의 크레딧 기준선을 고친다.
/// `hit_key` 는 새 응답 기준으로 소진된 창의 키(`Limits::exhausted_key`).
pub fn apply_fetch(
    cache: &mut UsageCache,
    fetched: CachedUsage,
    hit_key: Option<String>,
    now: DateTime<Utc>,
) {
    let credits = used_credits(Some(&fetched));
    cache.usage = Some(fetched);
    cache.last_attempt = Some(now);
    cache.last_error.clear();
    cache.failures = 0;
    cache.backoff_until = None;

    let Some(credits) = credits else {
        cache.prev_credits = None;
        if hit_key.is_none() {
            cache.baseline = None;
        }
        return;
    };
    if cache.prev_credits.is_some_and(|prev| credits > prev) {
        cache.credits_rising_at = Some(now);
    }
    if credits < 0.0 || cache.prev_credits.is_some_and(|prev| credits < prev) {
        cache.baseline = None; // 월 리셋이나 정정
    }
    cache.prev_credits = Some(credits);
    match hit_key {
        None => cache.baseline = None,
        Some(key) if cache.baseline.as_ref().is_none_or(|b| b.window_key != key) => {
            cache.baseline = Some(CreditBaseline {
                window_key: key,
                credits,
                at: Some(now),
            });
        }
        Some(_) => {}
    }
}

/// 갱신 실패 — 1분부터 두 배씩(최대 30분) 쉬고, 서버가 더 길게(`Retry-After`) 말하면 그만큼 쉰다.
pub fn apply_failure(
    cache: &mut UsageCache,
    error: &str,
    retry_after: TimeDelta,
    now: DateTime<Utc>,
) {
    cache.last_attempt = Some(now);
    cache.last_error = error.to_string();
    cache.failures += 1;
    let backoff =
        (TimeDelta::minutes(1) * (1 << (cache.failures - 1).min(5))).min(TimeDelta::minutes(30));
    // 넘치면(터무니없는 Retry-After) 그 자리에 머문다 — 시각이 비면 backoff 가 풀린다.
    let wait = backoff.max(retry_after);
    cache.backoff_until = Some(
        now.checked_add_signed(wait)
            .unwrap_or(DateTime::<Utc>::MAX_UTC),
    );
}

fn used_credits(usage: Option<&CachedUsage>) -> Option<f64> {
    let extra = usage?.extra.as_ref()?;
    extra.used_credits.filter(|_| extra.enabled)
}

/// 계정 파일을 다시 읽어야 하나 — 다른 자리의 캐시거나, 한도 숫자가 바뀌었거나(전환 직후 한도가 다른 계정 것으로
/// 바뀐다), 1분이 지났으면.
pub fn need_account_check(
    source: &str,
    lim: &Limits,
    account: &AccountCache,
    now: DateTime<Utc>,
) -> bool {
    account.source != source
        || account.at != lim.key()
        || account.checked_at.is_none_or(|t| now - t >= ACCOUNT_TTL)
}

/// 캐시된 이메일 — **이 자리의 계정 파일에서 읽은 것일 때만**.
pub fn account_cached<'a>(source: &str, account: &'a AccountCache) -> Option<&'a str> {
    (account.source == source).then_some(account.email.as_str())
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

/// 단계가 오른 직후 깜빡이는 시간 — 계속 움직이는 표시는 결국 눈에 안 들어오므로 그 뒤에는 배지로 남는다.
pub const ALERT_BURST: TimeDelta = TimeDelta::seconds(6);
/// 깜빡임 한 프레임 — 벽시계에서 프레임을 고르므로 프레임 카운터를 저장하지 않는다.
const ALERT_FRAME: TimeDelta = TimeDelta::milliseconds(500);

/// 강조할 창.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Alert {
    pub level: AlertLevel,
    /// `"5h"` / `"7d"` — 단계가 `None` 이면 빈 문자열.
    pub window: &'static str,
    /// 지금이 깜빡이는 구간인가.
    pub burst: bool,
    /// 깜빡이는 중 지금 프레임이 "켜짐" 인가 — 꺼진 프레임은 배지 대신 굵은 빨강이다.
    pub on: bool,
}

/// 가장 급한 창(깜빡임 없이) — 소진이 임박을 이기고, 같은 단계면 7d 가 5h 를 이긴다. 경보 시각을 저장할 수 없는
/// 경로(캐시 자리가 없을 때)는 이것으로 배지를 고정한다.
pub fn alert(cfg: &LimitsConfig, lim: &Limits) -> Alert {
    worst_window(cfg, lim).map_or_else(Alert::default, |(level, window, _)| Alert {
        level,
        window,
        ..Alert::default()
    })
}

/// 경보와 깜빡임 — 단계가 오르면(또는 창이 리셋돼 키가 바뀌면) 그 시각을 `state` 에 적고, 거기서부터 6초를 깜빡인다.
/// 돌려주는 `bool` 이 true 면 `state` 가 바뀌었다(써야 한다). 경보가 풀리면 무장을 해제한다.
pub fn alerts(
    cfg: &LimitsConfig,
    lim: &Limits,
    state: &mut StateFile,
    now: DateTime<Utc>,
) -> (Alert, bool) {
    let Some((level, window, wkey)) = worst_window(cfg, lim) else {
        if state.alert_key.is_empty() {
            return (Alert::default(), false);
        }
        state.alert_key.clear();
        state.alert_at = None;
        return (Alert::default(), true);
    };
    let key = format!("{}@{wkey}", level as u8);
    let mut dirty = false;
    if state.alert_key != key {
        state.alert_key = key;
        state.alert_at = Some(now);
        dirty = true;
    }
    let mut alert = Alert {
        level,
        window,
        ..Alert::default()
    };
    if let Some(at) = state.alert_at {
        let d = now - at;
        if d >= TimeDelta::zero() && d < ALERT_BURST {
            alert.burst = true;
            alert.on = (d.num_milliseconds() / ALERT_FRAME.num_milliseconds()) % 2 == 0;
        }
    }
    (alert, dirty)
}

/// 가장 급한 창과 그 창의 키. 경보가 없으면 `None`.
fn worst_window(cfg: &LimitsConfig, lim: &Limits) -> Option<(AlertLevel, &'static str, String)> {
    let mut best: Option<(AlertLevel, &'static str, String)> = None;
    for (name, w) in [("7d", lim.seven_day), ("5h", lim.five_hour)] {
        let Some(w) = w else { continue };
        let level = if w.percent >= 100.0 {
            AlertLevel::Over
        } else if cfg.alert() > 0.0 && w.percent >= cfg.alert() {
            AlertLevel::Near
        } else {
            continue;
        };
        if best.as_ref().is_none_or(|(b, _, _)| level > *b) {
            best = Some((level, name, window_key(name, &w)));
        }
    }
    best
}

/// 크레딧 사용이 "오르는 중" 으로 보는 시간 — 그 안에 `used_credits` 가 늘었으면 소진 중이다.
const RISING_WINDOW: TimeDelta = TimeDelta::minutes(15);

/// 크레딧 줄을 그릴 재료. 금액은 `credit_divisor` 로 환산한 값이다.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CreditView {
    pub show: bool,
    pub enabled: bool,
    /// 응답이 크레딧이 꺼진 계정이라고 알렸다 — "조회 중" 이 아니라 "비활성" 이다.
    pub disabled: bool,
    pub used: f64,
    pub limit: Option<f64>,
    /// 기준선 이후 쓴 크레딧 — 한도가 소진됐을 때만.
    pub spent_window: f64,
    pub spending: bool,
}

/// 크레딧 줄을 낼지, 무엇을 낼지 — usage 캐시에서 판정한다.
pub fn credits(
    cfg: &LimitsConfig,
    lim: &Limits,
    cache: &UsageCache,
    now: DateTime<Utc>,
) -> CreditView {
    let hit = lim.exhausted();
    let extra = cache.usage.as_ref().and_then(|u| u.extra.as_ref());
    let Some((extra, used)) =
        extra.and_then(|e| e.used_credits.filter(|_| e.enabled).map(|u| (e, u)))
    else {
        // 한도는 소진됐는데 크레딧을 아직 모른다.
        return CreditView {
            show: hit,
            disabled: extra.is_some_and(|e| !e.enabled),
            ..CreditView::default()
        };
    };
    let div = cfg.credit_divisor();
    let spent_window = match &cache.baseline {
        Some(b) if hit => (used - b.credits) / div,
        _ => 0.0,
    };
    let rising = cache
        .credits_rising_at
        .is_some_and(|at| now - at < RISING_WINDOW);
    let spending = spent_window > 0.0 || rising;
    CreditView {
        show: hit || spending || cfg.always_show_credits || used / div > 0.0,
        enabled: true,
        disabled: false,
        used: used / div,
        limit: extra.monthly_limit.map(|l| l / div),
        spent_window,
        spending,
    }
}
