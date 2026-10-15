//! `rocky statusline` 골든 — `tests/fixtures/statusline-full/<케이스>/` 의 `case.json` 을 `limits` + `statusline::full` 로
//! 다시 그려 `expected.txt` 와 바이트 단위로 비교한다. 처음 골든은 cc-usage 에서 떴고 지금은 rocky 출력이 기준이다.
//!
//! `case.json`: `why`(무엇을 고정하나) · `now`(RFC3339) · `tz`(`Asia/Seoul`·`UTC`) · `env`(`COLUMNS`·`TERM`·`COLORTERM`·
//! `NO_COLOR`) · `config`(snake_case — CLI 테스트가 `rocky.json` 키로 옮긴다) · `stdin`(`{{HOME}}` 자리) · `usage`/`state`
//! (그 계정의 캐시에 심을 JSON) · 있으면 `account`(로그인 이메일) · `args` · `sourceEnv` · `repo`(git 세그먼트용 명령).
//! 새 케이스는 폴더와 `case.json` 을 만들고 `ROCKY_UPDATE_GOLDEN=1 cargo test -p rocky-cli full_replays_goldens` 로
//! `expected.txt` 를 쓴 뒤 diff 를 눈으로 본다 — 이 파일은 비교만 한다.

use std::path::Path;

use chrono::{DateTime, FixedOffset, Utc};
use rocky_core::limits::{
    alerts, credits, local_limits, override_source, select, Input, LimitsConfig, Source, StateFile,
    UsageCache,
};
use rocky_core::statusline::full::{abbrev_home, duration, lines, Badge, Style, View};
use rocky_core::statusline::width::display_width;
use serde_json::Value;

/// 픽스처의 `{{HOME}}` 자리 — 캡처 때는 임시 HOME 이었다.
const HOME: &str = "/home/fixture";

/// 케이스의 인자에서 `--source` 값.
fn source_flag(case: &Value) -> Option<&str> {
    let args = case["args"].as_array()?;
    let at = args.iter().position(|a| a == "--source")?;
    args.get(at + 1)?.as_str()
}

fn render(case: &Value) -> String {
    let stdin = case["stdin"].as_str().unwrap().replace("{{HOME}}", HOME);
    let configured = case["config"]["source"]
        .as_str()
        .and_then(Source::parse)
        .unwrap_or_default();
    // 이번 실행의 source — 설정 < 환경 변수 < 플래그. 모르는 플래그 값은 CLI 가 한 줄로 알린다(CLI 테스트가 본다).
    let source = override_source(configured, case["sourceEnv"].as_str(), source_flag(case))
        .expect("모르는 --source 케이스는 CLI 테스트 몫");
    let cfg = LimitsConfig {
        source,
        alert_percent: case["config"]["alert_percent"].as_f64(),
        // 골든은 크레딧 페이드 없이 뜬 것이다 — 페이드는 `limits_test` 가 따로 본다.
        credit_fade: Some(false),
        ..Default::default()
    };
    // 그 계정의 캐시에 심을 usage.json — 없으면 빈 캐시.
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
    // 그 계정의 캐시에 심을 state.json — 없으면 빈 상태.
    let mut state: StateFile = serde_json::from_value(case["state"].clone()).unwrap_or_default();
    // none·agy 는 Claude 쪽 캐시·계정을 보지 않는다 — 심어 둔 캐시가 있어도 새면 안 된다.
    let local = local_limits(&cfg, &input, now);
    let selected = match local {
        Some(_) => None,
        None => select(&cfg, &input, &state, &cache, now),
    };
    let limits = local.map_or_else(|| selected.unwrap_or_default(), |(lim, _)| lim);
    // 경보 시각(state)으로 깜빡임 프레임을 고른다 — 한도를 다루지 않으면 경보도 없다. agy 는 배지로 고정.
    let alert = match local {
        Some((_, alert)) => alert,
        None => selected.map_or_else(Default::default, |_| {
            alerts(&cfg, &limits, &mut state, now).0
        }),
    };
    // 로그인된 계정(`~/.claude.json` 에 심을 이메일)의 배지.
    let badge = case["account"]
        .as_str()
        .filter(|_| local.is_none())
        .and_then(|email| {
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
        credit_glow: None,
        home: Some(HOME),
        dir_link: None,
        project_dir: "",
        now,
    };
    // CLI 는 줄들을 한 번에 내고 끝에 줄바꿈 하나를 붙인다.
    format!("{}\n", lines(&view, &style, &tz).join("\n"))
}

#[test]
fn renders_the_goldens() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/statusline-full");
    let mut dirs: Vec<_> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("{}: {e}", root.display()))
        .map(|e| e.unwrap().path())
        .collect();
    dirs.sort();
    assert!(
        dirs.len() >= 30,
        "픽스처가 {}건뿐이다 — 폴더가 지워졌나",
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
        // 모르는 --source 값의 한 줄 안내는 CLI 가 낸다 — 같은 곳이 본다.
        if source_flag(&case).is_some_and(|f| Source::parse(f).is_none()) {
            continue;
        }
        let (got, want) = (
            render(&case),
            std::fs::read_to_string(dir.join("expected.txt")).unwrap(),
        );
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

/// 폭 케이스(처음엔 cc-usage `render_test.go` 에서 옮겼다) — 배지·크레딧 배치가 이 계산에 기댄다.
#[test]
fn display_width_cases() {
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

/// 크레딧 금액 색 — 옅은 색(회색 쪽으로 반)에서 원래 색(사용률 그라데이션)으로. 24bit 는 정확히 섞고, 색을 섞을 수 없는
/// 터미널은 원래 색이 아니면 회색이다.
#[test]
fn credit_amount_fades_from_pale_to_its_usage_color() {
    use rocky_core::limits::{CreditView, Limits};
    let now = DateTime::parse_from_rfc3339("2026-09-16T07:40:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let credit_row = |glow: Option<f64>, style: Style| {
        let view = View {
            dir: "",
            git: None,
            model: "",
            effort: "",
            context_pct: None,
            limits: Limits::default(),
            usage: Some(&UsageCache::default()),
            alert: Default::default(),
            credits: CreditView {
                show: true,
                enabled: true,
                used: 11.6,
                limit: Some(50.0),
                ..Default::default()
            },
            currency: "$",
            home: None,
            dir_link: None,
            project_dir: "",
            now,
            badge: None,
            credit_glow: glow,
        };
        // 상태 문구("usage …")가 크레딧 앞에 오므로 줄 안에서 찾는다.
        lines(&view, &style, &chrono::Utc).join("\n")
    };
    let truecolor = Style {
        color: true,
        true_color: true,
        ..Default::default()
    };
    // 사용률 23.2% 의 원래 색 = rgb(82,170,23). 옅은 색은 회색(128,128,128)과 반씩.
    assert!(credit_row(None, truecolor).contains("\x1b[38;2;82;170;23m$38.40"));
    assert!(credit_row(Some(1.0), truecolor).contains("\x1b[38;2;82;170;23m$38.40"));
    assert!(credit_row(Some(0.0), truecolor).contains("\x1b[38;2;105;149;76m$38.40"));
    assert!(credit_row(Some(0.5), truecolor).contains("\x1b[38;2;94;160;50m$38.40"));
    // 3단계 터미널 — 옅은 쪽은 회색, 원래 색이면 초록.
    let plain = Style {
        color: true,
        ..Default::default()
    };
    assert!(credit_row(Some(0.5), plain).contains("\x1b[90m$38.40"));
    assert!(credit_row(Some(1.0), plain).contains("\x1b[32m$38.40"));
}

/// cc-usage `extra.Describe` 의 문구 — doctor 가 한 줄로 찍는다.
#[test]
fn extra_describe_wording() {
    use rocky_core::statusline::extra::{describe, expand_checked, Probe, Vars};
    use std::time::Duration;
    let ms = Duration::from_millis;
    let t = ms(300);
    for (probe, want) in [
        (
            Probe::Ok {
                lines: vec![b"a".to_vec()],
                elapsed: ms(12),
            },
            "ok 12ms → a",
        ),
        (
            Probe::Ok {
                lines: vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()],
                elapsed: ms(5),
            },
            "ok 5ms → a (외 2줄)",
        ),
        (
            Probe::Empty { elapsed: ms(7) },
            "출력 없음 — exit 0 이지만 stdout 이 비었다 (7ms)",
        ),
        (
            Probe::Skipped { missing: None },
            "건너뜀 — command 가 비어 있다",
        ),
        (
            Probe::Skipped {
                missing: Some("{{cwd}}"),
            },
            "건너뜀 — {{cwd}} 가 비어 있다",
        ),
        (
            Probe::NotFound("x: not found".into()),
            "미설치 — x: not found",
        ),
        (
            Probe::Timeout,
            "타임아웃 — 300ms 를 넘겼다 (timeoutMs 로 늘릴 수 있다)",
        ),
        (
            Probe::Failed {
                error: "exit status 1".into(),
                stderr: String::new(),
            },
            "비정상 종료 — exit status 1",
        ),
        (
            Probe::Failed {
                error: "exit status 1".into(),
                stderr: "boom".into(),
            },
            "비정상 종료 — exit status 1: boom",
        ),
    ] {
        assert_eq!(describe(&probe, t), want);
    }
    // 빈 placeholder 가 둘이면 이름 순서로 앞의 것을 댄다.
    let argv = ["t", "{{cwd}}{{session_id}}"].map(String::from);
    assert_eq!(
        expand_checked(
            &argv,
            Vars {
                session_id: "",
                cwd: ""
            }
        ),
        Err(Some("{{cwd}}"))
    );
}

/// 경로 링크(`statusline.dirLink`)는 rocky 만의 표시다 — 경로는 퍼센트 인코딩해 템플릿에 넣는다.
#[test]
fn dir_link_wraps_the_path_in_an_osc8_hyperlink() {
    use rocky_core::statusline::full::dir_link_url;
    assert_eq!(
        dir_link_url("vscode://file{{cwd}}", "/Users/me/dev/한 글"),
        Some("vscode://file/Users/me/dev/%ED%95%9C%20%EA%B8%80".to_string())
    );
    // 템플릿 자체에 공백·제어 문자·ASCII 밖 글자가 있으면 링크를 걸지 않는다(줄이 깨진다). `{{cwd}}` 가 없는
    // 템플릿(오타)도 — 모든 세션이 같은 주소로 열린다.
    for bad in [
        "vscode://file {{cwd}}",
        "x\x07{{cwd}}",
        "편집기://{{cwd}}",
        "vscode://file{cwd}",
    ] {
        assert_eq!(dir_link_url(bad, "/a"), None, "{bad:?}");
    }

    let now = DateTime::parse_from_rfc3339("2026-10-12T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let dir_row_in = |dir: &str, project_dir: &str, dir_link| {
        let view = View {
            dir,
            git: None,
            model: "",
            effort: "",
            context_pct: None,
            limits: Default::default(),
            usage: None,
            alert: Default::default(),
            credits: Default::default(),
            currency: "$",
            badge: None,
            credit_glow: None,
            home: Some("/home/me"),
            dir_link,
            project_dir,
            now,
        };
        let style = Style {
            color: true,
            ..Default::default()
        };
        lines(&view, &style, &chrono::Utc)[0].clone()
    };
    let dir_row = |dir_link| dir_row_in("/home/me/dev/rocky", "", dir_link);
    assert_eq!(
        dir_row(Some("vscode://file{{cwd}}")),
        "\x1b]8;;vscode://file/home/me/dev/rocky\x07\x1b[36m~/dev/rocky\x1b[0m\x1b]8;;\x07"
    );
    // 설정이 없거나 쓸 수 없는 템플릿이면 링크 없이 경로만.
    assert_eq!(dir_row(None), "\x1b[36m~/dev/rocky\x1b[0m");
    assert_eq!(dir_row(Some("bad {{cwd}}")), "\x1b[36m~/dev/rocky\x1b[0m");

    const TPL: Option<&str> = Some("vscode://file{{cwd}}");
    // 프로젝트 안의 하위 폴더면 경로를 둘로 나눠 건다 — 앞부분은 프로젝트를, 뒤의 하위 경로는 지금 폴더를 연다.
    assert_eq!(
        dir_row_in("/home/me/dev/rocky/crates", "/home/me/dev/rocky", TPL),
        "\x1b]8;;vscode://file/home/me/dev/rocky\x07\x1b[36m~/dev/rocky\x1b[0m\x1b]8;;\x07\
         \x1b]8;;vscode://file/home/me/dev/rocky/crates\x07\x1b[36m/crates\x1b[0m\x1b]8;;\x07"
    );
    // 지금 폴더가 곧 프로젝트면 링크 하나 — 버튼도, 겹치는 이름도 없다.
    assert_eq!(
        dir_row_in("/home/me/dev/rocky", "/home/me/dev/rocky", TPL),
        "\x1b]8;;vscode://file/home/me/dev/rocky\x07\x1b[36m~/dev/rocky\x1b[0m\x1b]8;;\x07"
    );
    // 프로젝트 밖(이름만 앞이 같은 형제 워크트리 포함)이면 경로는 지금 폴더를, 끝의 `↗ 이름` 버튼은 프로젝트를 연다.
    assert_eq!(
        dir_row_in("/home/me/dev/rocky-wt", "/home/me/dev/rocky", TPL),
        "\x1b]8;;vscode://file/home/me/dev/rocky-wt\x07\x1b[36m~/dev/rocky-wt\x1b[0m\x1b]8;;\x07  \
         \x1b]8;;vscode://file/home/me/dev/rocky\x07\x1b[90m↗\x1b[0m \x1b[36mrocky\x1b[0m\x1b]8;;\x07"
    );
    // 템플릿이 없으면 나누지 않고 링크도 없다, 프로젝트를 모르면 경로 전체가 지금 폴더를 연다.
    assert_eq!(
        dir_row_in("/home/me/dev/rocky/crates", "/home/me/dev/rocky", None),
        "\x1b[36m~/dev/rocky/crates\x1b[0m"
    );
    assert_eq!(
        dir_row_in("/home/me/dev/rocky/crates", "", TPL),
        "\x1b]8;;vscode://file/home/me/dev/rocky/crates\x07\x1b[36m~/dev/rocky/crates\x1b[0m\x1b]8;;\x07"
    );
}
