//! 사용 로그 — 이름 정규화, 파일 append/read, 집계.

use chrono::{TimeZone, Utc};
use rocky_core::usage::{
    append_event, build_report, client_of, month_file, normalize_route, parse_since, read_events,
    render_report, UsageEvent, UsageSource, KNOWN_SURFACES,
};

fn ev(ts: &str, source: UsageSource, name: &str, ok: bool, ms: Option<u64>) -> UsageEvent {
    UsageEvent {
        ts: ts.into(),
        source,
        name: name.into(),
        actor: Some("logan".into()),
        client: Some("cli".into()),
        ok,
        ms,
        meta: None,
    }
}

#[test]
fn routes_fold_the_ref_segment_and_skip_noisy_ones() {
    assert_eq!(
        normalize_route("get", "/api/todos/abc123").as_deref(),
        Some("GET /api/todos/:ref")
    );
    assert_eq!(
        normalize_route("POST", "/api/todos/rocky-12/handoff?x=1").as_deref(),
        Some("POST /api/todos/:ref/handoff")
    );
    assert_eq!(
        normalize_route("PATCH", "/api/boards/rocky").as_deref(),
        Some("PATCH /api/boards/:ref")
    );
    assert_eq!(
        normalize_route("GET", "/api/rc/servers").as_deref(),
        Some("GET /api/rc/servers")
    );
    assert_eq!(
        normalize_route("POST", "/api/rc/servers/repo-a/restart").as_deref(),
        Some("POST /api/rc/servers/:ref/restart")
    );
    assert_eq!(
        normalize_route("GET", "/api/todos").as_deref(),
        Some("GET /api/todos")
    );
    assert_eq!(
        normalize_route("GET", "/api/logs/worklog?board=rocky").as_deref(),
        Some("GET /api/logs/worklog")
    );
    // 셋째 자리가 동작 이름인 라우트는 접지 않는다.
    assert_eq!(
        normalize_route("POST", "/api/handoffs/claim").as_deref(),
        Some("POST /api/handoffs/claim")
    );
    assert_eq!(
        normalize_route("POST", "/api/handoffs/h1/cancel").as_deref(),
        Some("POST /api/handoffs/:ref/cancel")
    );
    for skipped in [
        "/api/health",
        "/api/statusline",
        "/api/events",
        "/api/usage",
        "/api/sessions/inbox",
    ] {
        assert_eq!(normalize_route("GET", skipped), None, "{skipped}");
    }
    // 노트 문서 편집·프레즌스·노트별 SSE 는 모양으로 거른다 — 여는 GET 만 남는다.
    assert_eq!(
        normalize_route("GET", "/api/notes/n1/doc?sv=abc").as_deref(),
        Some("GET /api/notes/:ref/doc")
    );
    for (method, path) in [
        ("POST", "/api/notes/n1/doc"),
        ("GET", "/api/notes/n1/doc/events"),
        ("POST", "/api/notes/n1/presence"),
    ] {
        assert_eq!(normalize_route(method, path), None, "{method} {path}");
    }
}

#[test]
fn client_prefers_the_header_then_guesses_from_user_agent() {
    assert_eq!(client_of(Some("mcp"), Some("Mozilla/5.0")), "mcp");
    assert_eq!(client_of(Some("  "), Some("rocky-cli/0.1")), "cli");
    assert_eq!(client_of(None, Some("ureq/3")), "cli");
    assert_eq!(client_of(None, Some("Mozilla/5.0 Safari")), "web");
    assert_eq!(client_of(None, None), "other");
}

#[test]
fn append_and_read_are_monthly_and_skip_bad_lines() {
    let dir = tempfile::tempdir().unwrap();
    let a = ev(
        "2026-08-30T10:00:00.000Z",
        UsageSource::Cli,
        "rocky ls",
        true,
        Some(12),
    );
    let b = ev(
        "2026-09-02T10:00:00.000Z",
        UsageSource::Mcp,
        "todo_list",
        true,
        Some(3),
    );
    let c = ev(
        "2026-09-28T10:00:00.000Z",
        UsageSource::Rest,
        "GET /api/todos",
        false,
        None,
    );
    for e in [&a, &b, &c] {
        append_event(dir.path(), e).unwrap();
    }
    assert!(month_file(dir.path(), &a.ts).ends_with("2026-08.jsonl"));
    assert!(month_file(dir.path(), &b.ts).ends_with("2026-09.jsonl"));
    // 깨진 줄은 조용히 건너뛴다.
    std::fs::write(
        month_file(dir.path(), &c.ts),
        format!(
            "{}\nnot json\n{}\n",
            serde_json::to_string(&b).unwrap(),
            serde_json::to_string(&c).unwrap()
        ),
    )
    .unwrap();
    let all = read_events(dir.path(), "2026-08-01T00:00:00.000Z");
    assert_eq!(all.len(), 3);
    // since 가 9월이면 8월 파일은 열지도 않고, 9월 안에서도 시각으로 거른다.
    let sept = read_events(dir.path(), "2026-09-10T00:00:00.000Z");
    assert_eq!(sept.len(), 1);
    assert_eq!(sept[0].name, "GET /api/todos");
    assert!(read_events(&dir.path().join("nope"), "2026-01-01T00:00:00.000Z").is_empty());
}

#[test]
fn since_specs() {
    let now = Utc.with_ymd_and_hms(2026, 9, 28, 12, 0, 0).unwrap();
    assert_eq!(
        parse_since("30d", now).as_deref(),
        Some("2026-08-29T12:00:00.000Z")
    );
    assert_eq!(
        parse_since("12h", now).as_deref(),
        Some("2026-09-28T00:00:00.000Z")
    );
    assert_eq!(
        parse_since("1w", now).as_deref(),
        Some("2026-09-21T12:00:00.000Z")
    );
    assert_eq!(
        parse_since("7", now).as_deref(),
        Some("2026-09-21T12:00:00.000Z")
    );
    assert_eq!(parse_since("soon", now), None);
}

#[test]
fn report_counts_errors_latency_unused_and_distribution() {
    let events = vec![
        ev(
            "2026-09-27T01:00:00.000Z",
            UsageSource::Mcp,
            "todo_list",
            true,
            Some(4),
        ),
        ev(
            "2026-09-27T02:00:00.000Z",
            UsageSource::Mcp,
            "todo_list",
            true,
            Some(8),
        ),
        ev(
            "2026-09-28T02:00:00.000Z",
            UsageSource::Mcp,
            "todo_list",
            false,
            Some(100),
        ),
        ev(
            "2026-09-28T03:00:00.000Z",
            UsageSource::Cli,
            "rocky board path",
            true,
            Some(20),
        ),
        ev(
            "2026-09-28T03:30:00.000Z",
            UsageSource::Hook,
            "hook notify-todo",
            true,
            None,
        ),
    ];
    let known: &[(UsageSource, &str)] = &[
        (UsageSource::Mcp, "todo_list"),
        (UsageSource::Mcp, "todo_write"),
        (UsageSource::Cli, "rocky board"),
        (UsageSource::Cli, "rocky open"),
        (UsageSource::Hook, "hook notify-todo"),
    ];
    let r = build_report(
        &events,
        "2026-09-01T00:00:00.000Z",
        "2026-09-28T12:00:00.000Z",
        known,
    );
    assert_eq!(r.total, 5);
    let top = &r.surfaces[0];
    assert_eq!(
        (top.name.as_str(), top.count, top.errors),
        ("todo_list", 3, 1)
    );
    assert_eq!((top.p50_ms, top.p95_ms), (Some(8), Some(100)));
    assert_eq!(top.last_ts, "2026-09-28T02:00:00.000Z");
    // CLI 는 접두사로 맞는다 — `rocky board path` 가 `rocky board` 를 쓴 것으로.
    assert_eq!(
        r.unused,
        vec![
            (UsageSource::Mcp, "todo_write".to_string()),
            (UsageSource::Cli, "rocky open".to_string()),
        ]
    );
    assert_eq!(
        r.by_day,
        vec![("2026-09-27".to_string(), 2), ("2026-09-28".to_string(), 3)]
    );
    assert_eq!(r.by_hour.iter().sum::<u64>(), 5);
    assert_eq!(r.clients, vec![("cli".to_string(), 5)]);
    let text = render_report(&r);
    assert!(text.contains("5건"));
    assert!(text.contains("todo_list  ✗1  8/100ms"));
    assert!(text.contains("안 쓴 표면 2개"));
    assert!(text.contains("rocky open"));
}

#[test]
fn empty_report_says_so_and_known_surfaces_are_distinct() {
    let r = build_report(
        &[],
        "2026-09-01T00:00:00.000Z",
        "2026-09-28T00:00:00.000Z",
        KNOWN_SURFACES,
    );
    assert_eq!(r.unused.len(), KNOWN_SURFACES.len());
    assert!(
        KNOWN_SURFACES.iter().any(|(s, _)| *s == UsageSource::Rest),
        "REST 라우트가 전수 목록에 없다"
    );
    assert!(render_report(&r).contains("기록 없음"));
    let mut names: Vec<&str> = KNOWN_SURFACES.iter().map(|(_, n)| *n).collect();
    let before = names.len();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), before, "KNOWN_SURFACES 에 중복이 있다");
}

#[test]
fn event_json_shape_is_compact() {
    let e = UsageEvent::new(UsageSource::Web, "web:now-row", true);
    let v: serde_json::Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["source"], "web");
    assert!(v.get("actor").is_none() && v.get("ms").is_none() && v.get("meta").is_none());
}
