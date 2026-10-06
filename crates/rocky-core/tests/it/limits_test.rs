//! `rocky_core::limits`(입력 파싱·한도 선택·경보)와 `statusline::git`(porcelain v2 파싱). cc-usage `internal/core` ·
//! `internal/git` 테스트에서 옮긴 케이스다. 렌더까지 포함한 바이트 대조는 `cc_usage_parity_test`.

use chrono::{DateTime, TimeDelta, Utc};
use rocky_core::limits::{
    account_cached, alert, apply_failure, apply_fetch, need_account_check, need_refresh,
    poll_interval, select, AccountCache, AlertLevel, CachedUsage, ExtraUsage, Input, Limits,
    LimitsConfig, Source, StateFile, UsageCache, Window,
};
use rocky_core::statusline::git::GitStatus;

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-09-16T07:40:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn cfg(source: Source, alert_percent: Option<f64>) -> LimitsConfig {
    LimitsConfig {
        source,
        alert_percent,
        ..Default::default()
    }
}

fn win(percent: f64) -> Option<Window> {
    Some(Window {
        percent,
        resets_at: None,
    })
}

#[test]
fn parses_claude_code_payload() {
    let input = Input::parse(
        r#"{"session_id":"s","cwd":"/a","model":{"display_name":"Opus 5"},"effort":{"level":"high"},
            "context_window":{"used_percentage":41},"workspace":{"current_dir":"/b"},
            "rate_limits":{"five_hour":{"used_percentage":23.5,"resets_at":1738425600}},"cost":{"x":1}}"#,
    );
    assert_eq!(input.session_id, "s");
    assert_eq!(input.model, "Opus 5");
    assert_eq!(input.effort, "high");
    assert_eq!(input.context_pct, Some(41.0));
    assert_eq!(input.dir(), "/b");
    let five = input.five_hour.unwrap();
    assert_eq!(five.percent, 23.5);
    assert_eq!(five.resets_at.unwrap().timestamp(), 1738425600);
    assert_eq!(input.seven_day, None);
}

#[test]
fn dir_falls_back_to_cwd() {
    assert_eq!(Input::parse(r#"{"cwd":"/a"}"#).dir(), "/a");
}

#[test]
fn resets_at_accepts_seconds_millis_rfc3339_and_numeric_strings() {
    for raw in [
        "1758008400",
        "1758008400000",
        r#""2025-09-16T07:40:00Z""#,
        r#""1758008400""#,
    ] {
        let input = Input::parse(&format!(
            r#"{{"rate_limits":{{"five_hour":{{"used_percentage":1,"resets_at":{raw}}}}}}}"#
        ));
        let at = input.five_hour.unwrap().resets_at;
        assert_eq!(at.map(|t| t.timestamp()), Some(1758008400), "{raw}");
    }
    for raw in ["null", "0", "-5", r#""soon""#] {
        let input = Input::parse(&format!(
            r#"{{"rate_limits":{{"five_hour":{{"used_percentage":1,"resets_at":{raw}}}}}}}"#
        ));
        assert_eq!(input.five_hour.unwrap().resets_at, None, "{raw}");
    }
}

#[test]
fn drops_epoch_leak_and_clamps_overshoot() {
    let input = Input::parse(
        r#"{"rate_limits":{"five_hour":{"used_percentage":1.7e9},"seven_day":{"used_percentage":150}}}"#,
    );
    assert_eq!(input.five_hour, None);
    assert_eq!(input.seven_day.unwrap().percent, 100.0);
}

/// Go 디코더는 `*float64` 에 숫자가 아닌 값이 오면 포인터를 할당한 채 0 으로 둔다 — 그대로 따라간다.
#[test]
fn mismatched_numbers_become_zero_like_go() {
    let input = Input::parse(
        r#"{"model":"Opus 5","context_window":{"used_percentage":"41"},
            "rate_limits":{"five_hour":{"used_percentage":"30"},"seven_day":{"used_percentage":null}}}"#,
    );
    assert_eq!(input.model, "");
    assert_eq!(input.context_pct, Some(0.0));
    assert_eq!(input.five_hour.unwrap().percent, 0.0);
    assert_eq!(input.seven_day, None);
}

#[test]
fn empty_or_invalid_input_is_empty() {
    for raw in ["", "  ", "{\"model\":", "[]"] {
        assert_eq!(Input::parse(raw), Input::default(), "{raw:?}");
    }
}

fn empty() -> (StateFile, UsageCache) {
    (StateFile::default(), UsageCache::default())
}

fn cached(five: f64, fetched_ago_min: i64) -> UsageCache {
    UsageCache {
        usage: Some(CachedUsage {
            fetched_at: Some(now() - TimeDelta::minutes(fetched_ago_min)),
            five_hour: win(five),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn select_follows_source() {
    let with = Input {
        five_hour: win(30.0),
        ..Default::default()
    };
    let without = Input::default();
    let (state, usage) = empty();

    assert_eq!(
        select(&cfg(Source::None, None), &with, &state, &usage, now()),
        None
    );

    for source in [Source::Stdin, Source::Auto] {
        let lim = select(&cfg(source, None), &with, &state, &usage, now()).unwrap();
        assert!(lim.from_stdin, "{source:?}");
        assert_eq!(lim.five_hour, win(30.0));
    }

    // api 는 stdin 을 보지 않고, auto 는 stdin 에 한도가 없으면 api 로 간다 — 캐시가 비면 빈 한도.
    for (source, input) in [(Source::Api, &with), (Source::Auto, &without)] {
        let lim = select(&cfg(source, None), input, &state, &usage, now()).unwrap();
        assert_eq!(lim, Limits::default(), "{source:?}");
    }

    // stdin 인데 한도가 없으면 stdin 출처의 빈 한도.
    let lim = select(&cfg(Source::Stdin, None), &without, &state, &usage, now()).unwrap();
    assert!(lim.from_stdin && lim.five_hour.is_none());
}

#[test]
fn select_reads_the_usage_cache_away_from_stdin() {
    let state = StateFile::default();
    let lim = select(
        &cfg(Source::Api, None),
        &Input::default(),
        &state,
        &cached(42.0, 5),
        now(),
    )
    .unwrap();
    assert_eq!((lim.five_hour, lim.from_stdin), (win(42.0), false));
}

#[test]
fn stdin_side_falls_back_to_what_it_saw_within_six_hours() {
    let state = |ago_h: i64| StateFile {
        observed_at: Some(now() - TimeDelta::hours(ago_h)),
        stdin_limits_seen: Some(now() - TimeDelta::hours(ago_h)),
        five_hour: win(55.0),
        ..Default::default()
    };
    let usage = cached(10.0, 1);
    let input = Input::default();
    // stdin 모드: 6시간 안에 본 값을 그린다, 넘으면 빈 한도.
    let lim = select(&cfg(Source::Stdin, None), &input, &state(1), &usage, now()).unwrap();
    assert_eq!((lim.five_hour, lim.from_stdin), (win(55.0), true));
    let lim = select(&cfg(Source::Stdin, None), &input, &state(7), &usage, now()).unwrap();
    assert_eq!((lim.five_hour, lim.from_stdin), (None, true));
    // auto: 최근에 stdin 을 봤으면 stdin 쪽(관측값), 오래됐으면 api 쪽(캐시).
    let lim = select(&cfg(Source::Auto, None), &input, &state(1), &usage, now()).unwrap();
    assert_eq!((lim.five_hour, lim.from_stdin), (win(55.0), true));
    let lim = select(&cfg(Source::Auto, None), &input, &state(7), &usage, now()).unwrap();
    assert_eq!((lim.five_hour, lim.from_stdin), (win(10.0), false));
}

#[test]
fn select_drops_windows_past_their_reset() {
    let input = Input {
        five_hour: Some(Window {
            percent: 80.0,
            resets_at: Some(now() - TimeDelta::minutes(5)),
        }),
        seven_day: Some(Window {
            percent: 40.0,
            resets_at: Some(now()),
        }),
        ..Default::default()
    };
    let (state, usage) = empty();
    let lim = select(&cfg(Source::Stdin, None), &input, &state, &usage, now()).unwrap();
    assert_eq!(lim.five_hour, None);
    assert_eq!(
        lim.seven_day.unwrap().percent,
        40.0,
        "리셋 시각 그 순간은 아직 유효"
    );
}

#[test]
fn poll_interval_widens_when_idle_but_not_past_stale() {
    let c = cfg(Source::Api, None);
    let lim = |p: f64| Limits {
        five_hour: win(p),
        ..Default::default()
    };
    assert_eq!(poll_interval(&c, &lim(30.0)), TimeDelta::minutes(15)); // 300초 × 3
    assert_eq!(poll_interval(&c, &lim(70.0)), TimeDelta::minutes(5));
    assert_eq!(poll_interval(&c, &lim(100.0)), TimeDelta::minutes(5)); // credit_poll 기본
    let slow = LimitsConfig {
        poll_seconds: Some(1200),
        ..cfg(Source::Api, None)
    };
    // 3배(60분)여도 stale 경계(30분)와 기본 주기(20분) 중 큰 쪽을 넘지 않는다.
    assert_eq!(poll_interval(&slow, &lim(30.0)), TimeDelta::minutes(30));
}

#[test]
fn need_refresh_respects_backoff_spawn_gap_and_stdin_quiet() {
    let api = cfg(Source::Api, None);
    let idle = Limits {
        five_hour: win(30.0),
        ..Default::default()
    };
    let (state, empty_usage) = empty();
    // 응답이 없으면 바로, 15분 지난 여유 구간 응답도.
    assert!(need_refresh(
        &api,
        false,
        &idle,
        &state,
        &empty_usage,
        now()
    ));
    assert!(need_refresh(
        &api,
        false,
        &idle,
        &state,
        &cached(30.0, 15),
        now()
    ));
    assert!(!need_refresh(
        &api,
        false,
        &idle,
        &state,
        &cached(30.0, 14),
        now()
    ));
    // backoff 중·방금 띄움·방금 시도면 아니다.
    let backoff = UsageCache {
        backoff_until: Some(now() + TimeDelta::minutes(1)),
        ..Default::default()
    };
    assert!(!need_refresh(&api, false, &idle, &state, &backoff, now()));
    let spawned = StateFile {
        spawned_at: Some(now() - TimeDelta::seconds(29)),
        ..Default::default()
    };
    assert!(!need_refresh(
        &api,
        false,
        &idle,
        &spawned,
        &empty_usage,
        now()
    ));
    let tried = UsageCache {
        last_attempt: Some(now() - TimeDelta::seconds(10)),
        ..Default::default()
    };
    assert!(!need_refresh(&api, false, &idle, &state, &tried, now()));
    // stdin 쪽은 한도가 소진됐을 때(또는 always_show_credits)만.
    let stdin = cfg(Source::Stdin, None);
    assert!(!need_refresh(
        &stdin,
        true,
        &idle,
        &state,
        &empty_usage,
        now()
    ));
    let hit = Limits {
        five_hour: win(100.0),
        from_stdin: true,
        ..Default::default()
    };
    assert!(need_refresh(
        &stdin,
        true,
        &hit,
        &state,
        &empty_usage,
        now()
    ));
    let always = LimitsConfig {
        always_show_credits: true,
        ..cfg(Source::Stdin, None)
    };
    assert!(need_refresh(
        &always,
        true,
        &idle,
        &state,
        &empty_usage,
        now()
    ));
}

#[test]
fn apply_fetch_tracks_rising_credits_and_the_window_baseline() {
    let fetched = |used: f64| CachedUsage {
        fetched_at: Some(now()),
        extra: Some(ExtraUsage {
            enabled: true,
            used_credits: Some(used),
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut cache = UsageCache {
        last_error: "boom".into(),
        failures: 3,
        backoff_until: Some(now()),
        ..Default::default()
    };
    apply_fetch(&mut cache, fetched(1000.0), None, now());
    assert_eq!(
        (
            cache.last_error.as_str(),
            cache.failures,
            cache.backoff_until
        ),
        ("", 0, None)
    );
    assert_eq!(
        (cache.prev_credits, cache.baseline.as_ref()),
        (Some(1000.0), None)
    );
    // 소진된 창에서 처음 본 값이 기준선, 늘면 rising.
    apply_fetch(&mut cache, fetched(1000.0), Some("5h".into()), now());
    assert_eq!(
        cache
            .baseline
            .as_ref()
            .map(|b| (b.window_key.as_str(), b.credits)),
        Some(("5h", 1000.0))
    );
    assert_eq!(cache.credits_rising_at, None);
    apply_fetch(&mut cache, fetched(1080.0), Some("5h".into()), now());
    assert_eq!(
        cache.baseline.as_ref().map(|b| b.credits),
        Some(1000.0),
        "같은 창이면 기준선을 지킨다"
    );
    assert_eq!(cache.credits_rising_at, Some(now()));
    // 줄면(월 리셋) 기준선을 다시 잡는다, 소진이 풀리면 지운다.
    apply_fetch(&mut cache, fetched(10.0), Some("5h".into()), now());
    assert_eq!(cache.baseline.as_ref().map(|b| b.credits), Some(10.0));
    apply_fetch(&mut cache, fetched(10.0), None, now());
    assert_eq!(cache.baseline, None);
}

#[test]
fn apply_failure_backs_off_exponentially_and_honors_retry_after() {
    let mut cache = UsageCache::default();
    let mut waits = Vec::new();
    for _ in 0..7 {
        apply_failure(&mut cache, "http 500", TimeDelta::zero(), now());
        waits.push((cache.backoff_until.unwrap() - now()).num_minutes());
    }
    assert_eq!(waits, [1, 2, 4, 8, 16, 30, 30]);
    apply_failure(&mut cache, "rate limited (429)", TimeDelta::hours(2), now());
    assert_eq!(cache.backoff_until, Some(now() + TimeDelta::hours(2)));
    assert_eq!(
        (cache.last_error.as_str(), cache.failures),
        ("rate limited (429)", 8)
    );
}

#[test]
fn account_is_rechecked_on_a_new_slot_new_limits_or_after_a_minute() {
    let lim = Limits {
        five_hour: win(30.0),
        ..Default::default()
    };
    let cache = AccountCache {
        source: "/h/.claude/.claude.json".into(),
        email: "a@x".into(),
        at: lim.key(),
        checked_at: Some(now() - TimeDelta::seconds(30)),
    };
    assert!(!need_account_check(
        "/h/.claude/.claude.json",
        &lim,
        &cache,
        now()
    ));
    assert!(need_account_check("/w/.claude.json", &lim, &cache, now()));
    let changed = Limits {
        five_hour: win(31.0),
        ..Default::default()
    };
    assert!(need_account_check(
        "/h/.claude/.claude.json",
        &changed,
        &cache,
        now()
    ));
    assert!(need_account_check(
        "/h/.claude/.claude.json",
        &lim,
        &cache,
        now() + TimeDelta::seconds(30)
    ));
    // 다른 자리의 캐시는 쓰지 않는다.
    assert_eq!(
        account_cached("/h/.claude/.claude.json", &cache),
        Some("a@x")
    );
    assert_eq!(account_cached("/w/.claude.json", &cache), None);
}

/// cc-usage 가 쓴 캐시를 그대로 읽는다 — Go 의 zero 시각은 없음, 모르는 필드는 무시.
#[test]
fn caches_read_cc_usage_json() {
    let usage: UsageCache = serde_json::from_str(
        r#"{"usage":{"fetched_at":"2026-09-16T07:40:00.123456+09:00","five_hour":{"percent":12.5,"resets_at":"0001-01-01T00:00:00Z"},
            "extra":{"enabled":true,"used_credits":1160,"monthly_limit":5000}},
            "last_attempt":"0001-01-01T00:00:00Z","credits_rising_at":"0001-01-01T00:00:00Z","baseline":{"window_key":"5h","credits":1000,"at":"2026-09-16T00:00:00Z"}}"#,
    )
    .unwrap();
    let u = usage.usage.as_ref().unwrap();
    assert_eq!(
        u.five_hour,
        Some(Window {
            percent: 12.5,
            resets_at: None
        })
    );
    assert!(u.fetched_at.is_some());
    assert_eq!((usage.last_attempt, usage.credits_rising_at), (None, None));
    assert_eq!(usage.baseline.as_ref().unwrap().credits, 1000.0);

    let state: StateFile = serde_json::from_str(
        r#"{"observed_at":"2026-09-16T07:00:00Z","stdin_limits_seen":"0001-01-01T00:00:00Z","account_email":"x@y","alert_key":"1@5h"}"#,
    )
    .unwrap();
    assert!(state.observed_at.is_some() && state.stdin_limits_seen.is_none());

    // 쓴 것을 다시 읽으면 같다.
    let back: UsageCache = serde_json::from_str(&serde_json::to_string(&usage).unwrap()).unwrap();
    assert_eq!(back, usage);
}

#[test]
fn alert_picks_the_most_urgent_window() {
    let lim = |five: f64, seven: f64| Limits {
        five_hour: win(five),
        seven_day: win(seven),
        from_stdin: true,
    };
    let at = |c: &LimitsConfig, l: Limits| {
        let a = alert(c, &l);
        (a.level, a.window)
    };
    let default = cfg(Source::Stdin, None);
    assert_eq!(at(&default, lim(30.0, 40.0)), (AlertLevel::None, ""));
    assert_eq!(at(&default, lim(92.0, 40.0)), (AlertLevel::Near, "5h"));
    assert_eq!(
        at(&default, lim(100.0, 95.0)),
        (AlertLevel::Over, "5h"),
        "소진이 임박을 이긴다"
    );
    assert_eq!(
        at(&default, lim(100.0, 100.0)),
        (AlertLevel::Over, "7d"),
        "같은 단계면 7d"
    );
    assert_eq!(at(&default, lim(95.0, 95.0)), (AlertLevel::Near, "7d"));

    // 0 이나 범위 밖이면 임박 경고를 끄고 소진만 남는다.
    for off in [Some(0.0), Some(150.0), Some(-1.0)] {
        let c = cfg(Source::Stdin, off);
        assert_eq!(at(&c, lim(95.0, 30.0)), (AlertLevel::None, ""), "{off:?}");
        assert_eq!(
            at(&c, lim(100.0, 30.0)),
            (AlertLevel::Over, "5h"),
            "{off:?}"
        );
    }
    assert_eq!(
        at(&cfg(Source::Stdin, Some(60.0)), lim(30.0, 65.0)),
        (AlertLevel::Near, "7d")
    );
}

#[test]
fn source_parses_known_values_only() {
    assert_eq!(Source::parse("none"), Some(Source::None));
    assert_eq!(Source::parse("stdin"), Some(Source::Stdin));
    assert_eq!(Source::parse("NONE"), None);
}

#[test]
fn git_status_parses_porcelain_v2() {
    let cases = [
        (
            "# branch.oid 1a2b3c4d5e6f\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +0 -0\n",
            GitStatus { branch: "main".into(), oid: "1a2b3c4d5e6f".into(), has_upstream: true, ..Default::default() },
        ),
        (
            "# branch.oid 1a2b3c4d5e6f\n# branch.head feat/x\n# branch.upstream origin/feat/x\n# branch.ab +1 -2\n1 .M N... 100644 100644 100644 a b f.go\n",
            GitStatus {
                branch: "feat/x".into(),
                oid: "1a2b3c4d5e6f".into(),
                has_upstream: true,
                ahead: 1,
                behind: 2,
                unstaged: 1,
                ..Default::default()
            },
        ),
        (
            // both.go 는 staged·unstaged 양쪽에 잡히고, 충돌은 XY 를 보지 않는다.
            "# branch.oid 1a2b3c4d5e6f\n# branch.head main\n1 M. N... a b c d e staged.go\n1 .M N... a b c d e unstaged.go\n1 MM N... a b c d e both.go\n2 R. N... a b c d e R100 new.go\told.go\nu UU N... a b c d e f g conflict.go\n",
            GitStatus { branch: "main".into(), oid: "1a2b3c4d5e6f".into(), staged: 3, unstaged: 2, conflicted: 1, ..Default::default() },
        ),
        (
            "# branch.oid 1a2b3c4d5e6f\n# branch.head (detached)\n",
            GitStatus { branch: "(detached)".into(), detached: true, oid: "1a2b3c4d5e6f".into(), ..Default::default() },
        ),
        ("# branch.oid (initial)\n# branch.head main\n", GitStatus { branch: "main".into(), ..Default::default() }),
    ];
    for (out, want) in cases {
        assert_eq!(GitStatus::parse(out), want, "{out:?}");
    }
}

#[test]
fn usage_response_parse_keeps_optional_fields_optional() {
    use rocky_core::limits::parse_usage_response;
    let u = parse_usage_response(
        br#"{"five_hour":{"utilization":42,"resets_at":"2026-09-16T09:00:00Z"},"seven_day":{"utilization":null},
            "extra_usage":{"is_enabled":true,"used_credits":1160},"unknown":1}"#,
        now(),
    )
    .unwrap();
    assert_eq!(u.fetched_at, Some(now()));
    assert_eq!(u.five_hour.unwrap().percent, 42.0);
    assert!(u.five_hour.unwrap().resets_at.is_some());
    assert_eq!(u.seven_day, None, "utilization 이 없으면 창이 없다");
    let extra = u.extra.unwrap();
    assert_eq!(
        (extra.enabled, extra.used_credits, extra.monthly_limit),
        (true, Some(1160.0), None)
    );
    // 리셋 시각을 못 읽으면 리셋 없음, 타입이 틀리면 응답 전체가 실패.
    let u = parse_usage_response(
        br#"{"five_hour":{"utilization":1,"resets_at":"soon"}}"#,
        now(),
    )
    .unwrap();
    assert_eq!(u.five_hour.unwrap().resets_at, None);
    let err = parse_usage_response(br#"{"five_hour":{"utilization":"1"}}"#, now()).unwrap_err();
    assert!(err.starts_with("parse usage: "), "{err}");
}

#[test]
fn alerts_arm_on_a_new_level_blink_for_six_seconds_then_settle() {
    use rocky_core::limits::{alerts, ALERT_BURST};
    let c = cfg(Source::Stdin, None);
    let reset = now() + TimeDelta::minutes(80);
    let hit = Limits {
        five_hour: Some(Window {
            percent: 100.0,
            resets_at: Some(reset),
        }),
        from_stdin: true,
        ..Default::default()
    };
    let mut state = StateFile::default();
    // 처음 본 단계 — 지금을 기록하고 켜진 프레임.
    let (a, dirty) = alerts(&c, &hit, &mut state, now());
    assert!(dirty && a.burst && a.on);
    assert_eq!(state.alert_key, "2@5h@2026-09-16T09:00:00Z");
    assert_eq!(state.alert_at, Some(now()));
    // 0.5초마다 켜짐/꺼짐, 같은 키면 다시 쓰지 않는다.
    for (ms, on) in [
        (499, true),
        (500, false),
        (999, false),
        (1000, true),
        (5999, false),
    ] {
        let (a, dirty) = alerts(&c, &hit, &mut state, now() + TimeDelta::milliseconds(ms));
        assert!(!dirty && a.burst, "{ms}");
        assert_eq!(a.on, on, "{ms}ms");
    }
    let (a, _) = alerts(&c, &hit, &mut state, now() + ALERT_BURST);
    assert!(!a.burst, "6초가 지나면 배지로 남는다");
    // 임박으로 내려가면(다른 단계) 다시 무장, 풀리면 해제.
    let near = Limits {
        five_hour: Some(Window {
            percent: 95.0,
            resets_at: Some(reset),
        }),
        ..hit
    };
    let later = now() + TimeDelta::minutes(1);
    let (a, dirty) = alerts(&c, &near, &mut state, later);
    assert!(dirty && a.burst && a.on);
    assert_eq!(state.alert_key, "1@5h@2026-09-16T09:00:00Z");
    let calm = Limits {
        five_hour: win(10.0),
        ..Default::default()
    };
    let (a, dirty) = alerts(&c, &calm, &mut state, later);
    assert!(dirty && a == Default::default());
    assert!(state.alert_key.is_empty() && state.alert_at.is_none());
    let (_, dirty) = alerts(&c, &calm, &mut state, later);
    assert!(!dirty, "이미 풀린 경보는 다시 쓰지 않는다");
}
