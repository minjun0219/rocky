//! cc-usage 와의 동일성 — `scripts/cc-usage-capture.ts` 가 뜬 골든(`tests/fixtures/cc-usage/<케이스>/`)을
//! `limits` + `statusline::full` 로 다시 그려 바이트 단위로 비교한다. 일부러 다르게 둔 곳은 케이스의 `allow`.

use std::path::Path;

use chrono::{DateTime, FixedOffset, Utc};
use rocky_core::limits::{alert, select, Input, LimitsConfig, Source};
use rocky_core::statusline::full::{abbrev_home, duration, lines, Style, View};
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
    };
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
    let (limits, tracking) = select(&cfg, &input, now);
    let view = View {
        dir: input.dir(),
        git: None,
        model: &input.model,
        effort: &input.effort,
        context_pct: input.context_pct,
        limits,
        tracking,
        alert: alert(&cfg, &limits),
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
