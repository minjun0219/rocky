//! cc-usage 와의 동일성 — `scripts/cc-usage-capture.ts` 가 뜬 골든(`tests/fixtures/cc-usage/<케이스>/`)을
//! `limits` + `statusline::full` 로 다시 그려 바이트 단위로 비교한다. 일부러 다르게 둔 곳은 케이스의 `allow`.

use std::path::Path;

use chrono::{DateTime, FixedOffset, Utc};
use rocky_core::limits::{
    alerts, credits, select, Input, LimitsConfig, Source, StateFile, UsageCache,
};
use rocky_core::statusline::full::{abbrev_home, duration, lines, Badge, Style, View};
use rocky_core::statusline::width::display_width;
use serde_json::Value;

/// 픽스처의 `{{HOME}}` 자리 — 캡처 때는 임시 HOME 이었다.
const HOME: &str = "/home/fixture";

fn render(case: &Value) -> String {
    let stdin = case["stdin"].as_str().unwrap().replace("{{HOME}}", HOME);
    let cfg = LimitsConfig {
        source: case["config"]["source"]
            .as_str()
            .and_then(Source::parse)
            .unwrap_or_default(),
        alert_percent: case["config"]["alert_percent"].as_f64(),
        ..Default::default()
    };
    // 캡처 때 cc-usage 캐시에 심은 usage.json — 없으면 빈 캐시.
    let cache: UsageCache = serde_json::from_value(case["usage"].clone()).unwrap_or_default();
    let now = DateTime::parse_from_rfc3339(case["now"].as_str().unwrap())
        .unwrap()
        .with_timezone(&Utc);
    let tz = match case["tz"].as_str().unwrap() {
        "Asia/Seoul" => FixedOffset::east_opt(9 * 3600).unwrap(),
        "UTC" => FixedOffset::east_opt(0).unwrap(),
        other => panic!("픽스처 시간대 {other} 는 고정 오프셋 표에 없다"),
    };
    let env = &case["env"];
    let style = Style::from_env(|k| env[k].as_str().map(str::to_string));

    let input = Input::parse(&stdin);
    // 캡처 때 cc-usage 캐시에 심은 state.json — 없으면 빈 상태.
    let mut state: StateFile = serde_json::from_value(case["state"].clone()).unwrap_or_default();
    let selected = select(&cfg, &input, &state, &cache, now);
    let limits = selected.unwrap_or_default();
    // 경보 시각(state)으로 깜빡임 프레임을 고른다 — 한도를 다루지 않으면 경보도 없다.
    let alert = selected.map_or_else(Default::default, |_| {
        alerts(&cfg, &limits, &mut state, now).0
    });
    // 로그인된 계정(캡처 때 ~/.claude.json 에 심은 이메일)의 배지.
    let badge = case["account"].as_str().and_then(|email| {
        let b = &case["config"]["badges"][email];
        b.is_object().then(|| Badge {
            emoji: b["emoji"].as_str().unwrap_or_default().to_string(),
            glyph: b["glyph"].as_str().unwrap_or_default().to_string(),
            color: b["color"].as_str().unwrap_or_default().to_string(),
        })
    });
    let view = View {
        dir: input.dir(),
        git: None,
        model: &input.model,
        effort: &input.effort,
        context_pct: input.context_pct,
        limits,
        usage: selected.map(|_| &cache),
        alert,
        credits: selected.map_or_else(Default::default, |_| credits(&cfg, &limits, &cache, now)),
        currency: cfg.currency(),
        badge: badge.as_ref(),
        home: Some(HOME),
        now,
    };
    // cc-usage 는 fmt.Println 으로 줄들을 한 번에 낸다.
    format!("{}\n", lines(&view, &style, &tz).join("\n"))
}

fn expected(dir: &Path, case: &Value) -> String {
    let mut want = std::fs::read_to_string(dir.join("expected.txt")).unwrap();
    for pair in case["allow"].as_array().unwrap() {
        let (from, to) = (pair[0].as_str().unwrap(), pair[1].as_str().unwrap());
        assert!(
            want.contains(from),
            "{}: allow 의 {from:?} 가 expected 에 없다",
            dir.display()
        );
        want = want.replace(from, to);
    }
    want
}

#[test]
fn renders_the_same_bytes_as_cc_usage() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cc-usage");
    let mut dirs: Vec<_> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("{}: {e}", root.display()))
        .map(|e| e.unwrap().path())
        .collect();
    dirs.sort();
    assert!(
        dirs.len() >= 30,
        "픽스처가 {}건뿐이다 — 캡처를 다시 돌렸나",
        dirs.len()
    );

    let mut failures = Vec::new();
    for dir in &dirs {
        let case: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("case.json")).unwrap()).unwrap();
        // git 세그먼트는 실제 repo 가 필요하다 — CLI 를 프로세스째 돌리는 테스트(rocky-cli)가 본다.
        if case["repo"].as_array().is_some_and(|r| !r.is_empty()) {
            continue;
        }
        // extra_commands 도 프로세스를 돌려야 한다 — 같은 곳이 본다.
        if case["config"]["extra_commands"].is_array() {
            continue;
        }
        let (got, want) = (render(&case), expected(dir, &case));
        if got != want {
            failures.push(format!(
                "{} ({})\n  want {want:?}\n  got  {got:?}",
                dir.file_name().unwrap().to_string_lossy(),
                case["why"].as_str().unwrap_or_default()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{}건 다름:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn duration_drops_zero_lower_units() {
    for (minutes, want) in [
        (0, "<1m"),
        (45, "45m"),
        (80, "1h 20m"),
        (52 * 60, "2d 4h"),
        (24 * 60, "1d"),
        (120, "2h"),
        (48 * 60, "2d"),
    ] {
        assert_eq!(duration(chrono::TimeDelta::minutes(minutes)), want);
    }
}

#[test]
fn abbrev_home_only_at_a_path_boundary() {
    assert_eq!(abbrev_home("/home/me", Some("/home/me")), "~");
    assert_eq!(abbrev_home("/home/me/x", Some("/home/me")), "~/x");
    assert_eq!(abbrev_home("/home/meow", Some("/home/me")), "/home/meow");
    assert_eq!(abbrev_home("/home/me/x", None), "/home/me/x");
}

/// cc-usage `render_test.go` 의 폭 케이스 — 배지·크레딧 배치가 이 계산에 기댄다.
#[test]
fn display_width_matches_cc_usage() {
    for (s, want) in [
        ("", 0),
        ("5h 70%", 6),
        ("\x1b[32m70%\x1b[0m", 3),
        ("🏢 $11.60", 9),
        ("크레딧 소진 중", 14),
        ("\x1b[90m(1h 20m→10:17)\x1b[0m", 14),
        ("→", 1),
        ("⎇ main", 6),
    ] {
        assert_eq!(display_width(s), want, "{s:?}");
    }
    for e in [
        "💳",
        "🏢",
        "🚀",
        "🟠",
        "🧠",
        "🪄",
        "⚡",
        "✅",
        "⌛",
        "⚡️",
        "❤️",
        "👨‍👩‍👧",
        "🏳️‍🌈",
    ] {
        assert_eq!(display_width(e), 2, "{e}");
    }
    for a in ["⎇", "⇡", "⇣", "↻", "·"] {
        assert_eq!(display_width(a), 1, "{a}");
    }
    // 텍스트 표현이 기본인 글자는 1칸, VS16 이 붙으면 2칸.
    for (s, want) in [
        ("🛠", 1),
        ("🛠️", 2),
        ("🛰", 1),
        ("🛰️", 2),
        ("🚀", 2),
        ("🛬", 2),
    ] {
        assert_eq!(display_width(s), want, "{s}");
    }
}

/// cc-usage `internal/extra` 의 치환 규칙 — 쓰인 placeholder 가 비면 건너뛰고, 안 쓰인 placeholder 는 비어도 된다.
#[test]
fn extra_expand_skips_only_when_a_used_placeholder_is_empty() {
    use rocky_core::statusline::extra::{expand, output_lines, Vars};
    let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let full = Vars {
        session_id: "s1",
        cwd: "/w",
    };
    assert_eq!(
        expand(&argv(&["t", "-s", "{{session_id}}", "--cwd={{cwd}}"]), full),
        Some(argv(&["t", "-s", "s1", "--cwd=/w"]))
    );
    let no_session = Vars {
        session_id: "",
        cwd: "/w",
    };
    assert_eq!(expand(&argv(&["t", "{{session_id}}"]), no_session), None);
    assert_eq!(
        expand(&argv(&["t", "{{cwd}}"]), no_session),
        Some(argv(&["t", "/w"]))
    );
    assert_eq!(expand(&[], full), None);

    assert_eq!(
        output_lines(b"a\n  \n\n  b\n\x1b[32mc\x1b[0m\n\n"),
        [&b"a"[..], b"  b", b"\x1b[32mc\x1b[0m"]
    );
    // UTF-8 이 아닌 바이트는 그대로 두고, 공백으로 보지 않는다.
    assert_eq!(output_lines(b"a\xffb\n\xff\n"), [&b"a\xffb"[..], b"\xff"]);
    assert!(output_lines(b"").is_empty());
}
