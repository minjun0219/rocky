//! `rocky_core::limits`(입력 파싱·한도 선택·경보)와 `statusline::git`(porcelain v2 파싱). cc-usage `internal/core` ·
//! `internal/git` 테스트에서 옮긴 케이스다. 렌더까지 포함한 바이트 대조는 `cc_usage_parity_test`.

use chrono::{DateTime, TimeDelta, Utc};
use rocky_core::limits::{
    alert, select, AlertLevel, Input, Limits, LimitsConfig, Source, Tracking, Window,
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

#[test]
fn select_follows_source() {
    let with = Input {
        five_hour: win(30.0),
        ..Default::default()
    };
    let without = Input::default();

    let (lim, tracking) = select(&cfg(Source::None, None), &with, now());
    assert_eq!((lim, tracking), (Limits::default(), Tracking::Off));

    for source in [Source::Stdin, Source::Auto] {
        let (lim, tracking) = select(&cfg(source, None), &with, now());
        assert!(lim.from_stdin, "{source:?}");
        assert_eq!(lim.five_hour, win(30.0));
        assert_eq!(tracking, Tracking::Empty);
    }

    // api 는 stdin 을 보지 않고, auto 는 stdin 에 한도가 없으면 api 로 간다 — 둘 다 조회 대기.
    for (source, input) in [(Source::Api, &with), (Source::Auto, &without)] {
        let (lim, tracking) = select(&cfg(source, None), input, now());
        assert_eq!(
            (lim, tracking),
            (Limits::default(), Tracking::Empty),
            "{source:?}"
        );
    }

    // stdin 인데 한도가 없으면 stdin 출처의 빈 한도.
    let (lim, _) = select(&cfg(Source::Stdin, None), &without, now());
    assert!(lim.from_stdin && lim.five_hour.is_none());
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
    let (lim, _) = select(&cfg(Source::Stdin, None), &input, now());
    assert_eq!(lim.five_hour, None);
    assert_eq!(
        lim.seven_day.unwrap().percent,
        40.0,
        "리셋 시각 그 순간은 아직 유효"
    );
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
