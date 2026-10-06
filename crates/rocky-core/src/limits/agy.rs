//! Antigravity(`agy`) — 같은 statusline 을 agy 에서도 쓴다. cc-usage `internal/core/agy.go` 의 이식.
//!
//! agy 는 statusLine stdin 에 Claude Code 와 같은 꼴의 JSON 을 주고, 한도는 `rate_limits` 대신 `quota` 로 준다(1.2.14 실측):
//!
//! ```json
//! "quota": {
//!   "gemini-5h":     {"remaining_fraction": 0.86, "reset_time": "2026-10-01T04:15:38Z"},
//!   "gemini-weekly": {"remaining_fraction": 0.94, "reset_time": "2026-10-07T08:19:44Z"},
//!   "3p-5h":         {...}, "3p-weekly": {...}
//! }
//! ```
//!
//! `3p` 는 Gemini 가 아닌 모델(Claude·GPT-OSS)의 몫이다. 비공식 필드라 전부 optional 로 본다. Claude 쪽 판정은 이 파일을
//! 모른다.

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::{
    alert, drop_expired, go_number, parse_resets, Alert, Input, Limits, LimitsConfig, Source,
    Window,
};

/// agy 가 stdin `product` 에 주는 값.
pub const PRODUCT_ANTIGRAVITY: &str = "antigravity";

/// `quota` 의 버킷 하나.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AgyQuota {
    /// 남은 비율(0~1).
    pub remaining_fraction: Option<f64>,
    pub reset_time: Option<DateTime<Utc>>,
}

impl AgyQuota {
    /// 객체가 아니거나 필드 타입이 틀리면 그 값만 빈다(타입이 틀린 숫자는 Go 처럼 0).
    pub(super) fn parse(v: &Value) -> AgyQuota {
        AgyQuota {
            remaining_fraction: v.get("remaining_fraction").and_then(go_number),
            reset_time: v.get("reset_time").and_then(parse_resets),
        }
    }
}

impl Input {
    /// Claude Code 가 아니라 Antigravity 가 준 입력인가.
    pub fn is_agy(&self) -> bool {
        self.product == PRODUCT_ANTIGRAVITY
    }
}

/// agy 의 `quota` 를 5h/7d 자리에 — 지금 모델이 쓰는 버킷을 고른다.
///
/// 버킷은 모델 이름으로 고른다 — 이름에 gemini 가 있으면 `gemini-*`, 아니면 `3p-*`. payload 에 모델 → 버킷 대응이 따로
/// 오지 않는다. 모델 이름을 모르거나 고른 버킷이 없으면 창을 비운다 — 다른 버킷의 숫자를 대신 그리면 지금 모델과 무관한
/// 한도가 그럴듯하게 보인다. 숫자는 stdin 에서 왔으니 `from_stdin` 이다(usage API 상태 문구를 낼 이유가 없다).
pub fn agy_limits(input: &Input, now: DateTime<Utc>) -> Limits {
    let name = input.model.to_lowercase();
    if name.is_empty() {
        return Limits {
            from_stdin: true,
            ..Limits::default()
        };
    }
    let bucket = if name.contains("gemini") {
        "gemini"
    } else {
        "3p"
    };
    let window = |suffix: &str| {
        let w = input
            .quota
            .get(&format!("{bucket}-{suffix}"))
            .and_then(agy_window);
        drop_expired(w, now)
    };
    Limits {
        five_hour: window("5h"),
        seven_day: window("weekly"),
        from_stdin: true,
    }
}

/// 남은 비율(0~1) → 사용률(%).
fn agy_window(q: &AgyQuota) -> Option<Window> {
    let r = q.remaining_fraction?;
    // 경계의 부동소수 오차(1.0000001, -1e-9)는 잘라 쓴다 — 하필 소진 직후에 창이 사라지면 안 된다. 그보다 크게 벗어나면
    // 단위를 모르는 것이고, 틀린 숫자보다 없는 편이 낫다(stdin 의 epoch 누출 가드와 같은 생각).
    const EPS: f64 = 0.01;
    if !(-EPS..=1.0 + EPS).contains(&r) {
        return None;
    }
    let r = r.clamp(0.0, 1.0);
    Some(Window {
        percent: (1.0 - r) * 100.0,
        resets_at: q.reset_time,
    })
}

/// 이 렌더가 Claude 의 한도 경로를 떠나나 — `Some` 이면 호출자는 토큰·API·캐시·계정 파일을 건드리지 않고 이 한도와 경보를
/// 그린다. `source: none` 이면 한도가 하나도 없고, agy 면(`none` 이 아닌 한) stdin 의 `quota` 를 그린다.
///
/// agy 는 `source` 설정과 무관하게 이쪽이다 — Claude 계정과 상관없는 호스트이고, 설정 파일 하나를 두 호스트가 같이 읽어도
/// 되게 하려는 것이다. 경보는 깜빡이지 않고 배지로 고정한다 — 깜빡임은 단계가 오른 시각을 `state.json` 에 적어야 셀 수
/// 있는데, 이 경로는 캐시를 쓰지 않는다(Claude 세션과 같은 파일을 나눠 쓰면 서로의 경보 시각을 덮는다).
pub fn local_limits(
    cfg: &LimitsConfig,
    input: &Input,
    now: DateTime<Utc>,
) -> Option<(Limits, Alert)> {
    if cfg.source == Source::None {
        return Some((Limits::default(), Alert::default()));
    }
    if !input.is_agy() {
        return None;
    }
    let lim = agy_limits(input, now);
    Some((lim, alert(cfg, &lim)))
}
