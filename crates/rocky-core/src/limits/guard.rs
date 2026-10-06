//! 크레딧 guard — 한도가 소진돼 크레딧이 차감되기 시작하면 prompt 를 막는다. cc-usage `core.Guard` · `allow` 의 이식.
//!
//! **fail-open 이다**: 꺼져 있거나, `source: none` 이거나, 데이터가 없으면 막지 않는다. 막는 것은 둘뿐이다 — 한도가 소진됐는데
//! 크레딧이 꺼졌다고 확인되지 않았을 때(이 prompt 부터 차감된다), 그리고 최근 크레딧이 늘었을 때.

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

use super::{credits, Limits, LimitsConfig, UsageCache};

/// `allow.json` — `rocky statusline allow` 가 쓴다. 이 시각까지 guard 가 막지 않는다.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AllowFile {
    #[serde(with = "super::go_time", skip_serializing_if = "Option::is_none")]
    pub allow_until: Option<DateTime<Utc>>,
}

/// 이 prompt 를 막나 — 막으면 그 이유(`Some`).
///
/// `hint` 는 API 를 거치지 않고 알아낸 크레딧 활성 여부(계정 파일의 `hasExtraUsageEnabled`)이고 `None` 은 모른다는 뜻이다.
/// usage 응답이 아직 없는 구간만 메운다 — `source: stdin` 은 한도에 닿기 전까지 API 를 부르지 않아 그 구간이 평소다.
pub fn guard(
    cfg: &LimitsConfig,
    lim: &Limits,
    usage: &UsageCache,
    allow: &AllowFile,
    hint: Option<bool>,
    now: DateTime<Utc>,
) -> Option<String> {
    if !cfg.guard || allow.allow_until.is_some_and(|until| now < until) {
        return None;
    }
    if let Some(key) = lim.exhausted_key() {
        // 크레딧이 꺼졌으면 쓸 크레딧이 없다 — Claude Code 가 알아서 막는다.
        if credits_enabled(usage, hint) == Some(false) {
            return None;
        }
        return Some(format!(
            "사용량 한도 소진 ({key}) — 이 prompt부터 크레딧이 차감됩니다"
        ));
    }
    if credits(cfg, lim, usage, now).spending {
        return Some("최근 크레딧 소진이 감지됐습니다".to_string());
    }
    None
}

/// 크레딧이 켜져 있나 — 관측값(usage 응답)이 힌트(계정 파일)를 이긴다. 계정 파일은 크레딧을 켜고 끈 직후 뒤처질 수 있지만
/// 관측값은 그 계정으로 실제 받은 응답이다. `None` 은 어느 쪽도 모른다는 뜻이고, 그때 guard 는 막는 쪽으로 남는다 — 한도를
/// 넘긴 상태에서 "모른다" 는 곧 "차감될 수도 있다" 다. `doctor` 가 guard 와 같은 답을 보이려고 이 함수를 같이 쓴다.
pub fn credits_enabled(usage: &UsageCache, hint: Option<bool>) -> Option<bool> {
    match usage.usage.as_ref().and_then(|u| u.extra.as_ref()) {
        Some(extra) => Some(extra.enabled),
        None => hint,
    }
}

/// Go `time.ParseDuration` 과 같은 꼴(`30m` · `2h` · `1h30m` · `1.5h` · `90s`) — 부호, 소수, 단위 `ns`·`us`·`µs`·`ms`·`s`·`m`·`h`
/// 의 나열. 단위가 없는 `0` 만 예외로 받는다. 읽을 수 없으면 `None`.
pub fn parse_go_duration(s: &str) -> Option<TimeDelta> {
    let (negative, mut rest) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    if rest == "0" {
        return Some(TimeDelta::zero());
    }
    if rest.is_empty() {
        return None;
    }
    let mut nanos = 0.0_f64;
    while !rest.is_empty() {
        let number_end = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        let number = &rest[..number_end];
        if number.is_empty() || number == "." {
            return None;
        }
        let value: f64 = number.parse().ok()?;
        rest = &rest[number_end..];
        let unit_end = rest
            .find(|c: char| c.is_ascii_digit() || c == '.')
            .unwrap_or(rest.len());
        let scale = match &rest[..unit_end] {
            "ns" => 1.0,
            "us" | "µs" | "μs" => 1e3,
            "ms" => 1e6,
            "s" => 1e9,
            "m" => 60e9,
            "h" => 3600e9,
            _ => return None,
        };
        nanos += value * scale;
        rest = &rest[unit_end..];
    }
    // Go 처럼 int64 나노초(약 292년)를 넘으면 읽지 못한 것으로 본다.
    if !nanos.is_finite() || nanos > i64::MAX as f64 {
        return None;
    }
    let d = TimeDelta::nanoseconds(nanos as i64);
    Some(if negative { -d } else { d })
}
