//! TS `src/cli.ts` 의 서브커맨드 디스패치 포팅.
//!
//! 각 커맨드는 REST 한두 번을 치고 결과를 컴팩트 텍스트로 찍는다. `--json` 이면 서버
//! 응답 원본을 그대로 낸다 — 텍스트 렌더는 사람용이고 JSON 은 스크립트/모델용이라
//! 둘을 같은 형태로 억지로 맞추지 않는다.

use rocky_core::github::{is_repo_slug, parse_repo_from_remote};
use rocky_core::refs::{NoteView, TodoView};
use rocky_core::types::{Board, Comment, HistoryEntry, Section};
use serde_json::{json, Value};

use crate::client::{request, request_value, CliContext};
use crate::flags::ParsedFlags;
use crate::format::*;

/// 텍스트/JSON 출력을 한 자리에서 가른다.
pub struct Printer {
    pub json: bool,
}

impl Printer {
    /// `--json` 이면 값 원본을, 아니면 `text()` 가 만든 문자열을 찍는다.
    ///
    /// 텍스트를 클로저로 받는 이유는 JSON 경로에서 렌더 비용을 아예 안 치르기 위해서다.
    pub fn emit(&self, value: &Value, text: impl FnOnce() -> String) {
        if self.json {
            println!(
                "{}",
                serde_json::to_string_pretty(value).unwrap_or_default()
            );
        } else {
            println!("{}", text());
        }
    }

    /// JSON 표현이 따로 없는 안내 문구 — `--json` 이어도 그대로 낸다.
    pub fn line(&self, text: &str) {
        println!("{text}");
    }
}

/// 현재 시각(epoch millis) — doing 경과 계산의 기준.
fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// `null` 이 아닌 값만 남긴 객체를 만든다.
///
/// 서버의 PATCH 는 **키의 존재**를 "이 필드를 바꾼다"로 읽는다. `None` 을 `null` 로
/// 실어 보내면 "지운다"가 되어, 플래그를 안 준 필드가 조용히 비워진다.
fn compact(pairs: Vec<(&str, Option<Value>)>) -> Value {
    let mut map = serde_json::Map::new();
    for (key, value) in pairs {
        if let Some(value) = value {
            map.insert(key.to_string(), value);
        }
    }
    Value::Object(map)
}

fn s(value: Option<&str>) -> Option<Value> {
    value.map(|v| json!(v))
}

/// `--link URL` 들을 서버가 받는 `[{ url }]` 형태로.
fn links_value(flags: &ParsedFlags) -> Option<Value> {
    let links = flags.list_flag("link");
    if links.is_empty() {
        return None;
    }
    Some(Value::Array(
        links.iter().map(|url| json!({ "url": url })).collect(),
    ))
}

fn labels_value(flags: &ParsedFlags) -> Option<Value> {
    let labels = flags.list_flag("label");
    if labels.is_empty() {
        None
    } else {
        Some(json!(labels))
    }
}

/// `ls` — 계층 + 섹션(또는 `--all` 이면 보드)으로 묶어 렌더한다.
pub fn cmd_ls(
    ctx: &CliContext,
    flags: &ParsedFlags,
    board: &str,
    printer: &Printer,
) -> Result<(), String> {
    // `--board` 를 명시하면 그 보드만 본다 — `--all` 과 같이 오면 명시가 이긴다.
    let all_view = flags.bool_flag("all") && flags.str_flag("board").is_none();
    let mut query: Vec<String> = Vec::new();
    if !all_view {
        query.push(format!("board={}", encode_uri_component(board)));
    }
    if flags.bool_flag("archived") {
        query.push("includeArchived=true".to_string());
    }
    let qs = if query.is_empty() {
        String::new()
    } else {
        format!("?{}", query.join("&"))
    };

    let raw = request_value(ctx, "GET", &format!("/api/todos{qs}"), None)?;
    let todos: Vec<TodoView> =
        serde_json::from_value(raw.clone()).map_err(|e| format!("todo 목록을 읽지 못했다: {e}"))?;
    let boards: Vec<Board> = request(ctx, "GET", "/api/boards", None)?;
    let sections: Vec<Section> = if all_view {
        Vec::new()
    } else {
        request(
            ctx,
            "GET",
            &format!("/api/sections?board={}", encode_uri_component(board)),
            None,
        )?
    };
    printer.emit(&raw, || {
        group_and_render(&todos, &sections, &boards, all_view, now_ms())
    });
    Ok(())
}

/// `add` — 새 todo.
pub fn cmd_add(
    ctx: &CliContext,
    rest: &[String],
    flags: &ParsedFlags,
    board: &str,
    printer: &Printer,
) -> Result<(), String> {
    let Some(title) = rest.first().filter(|t| !t.is_empty()) else {
        return Err("usage: rocky add \"제목\" [플래그]".into());
    };
    let body = compact(vec![
        ("board", Some(json!(board))),
        ("title", Some(json!(title))),
        ("description", s(flags.str_flag("desc"))),
        ("section", s(flags.str_flag("section"))),
        ("parentId", s(flags.str_flag("parent"))),
        ("priority", s(flags.str_flag("priority"))),
        ("due", s(flags.str_flag("due"))),
        ("labels", labels_value(flags)),
        ("links", links_value(flags)),
    ]);
    let raw = request_value(ctx, "POST", "/api/todos", Some(&body))?;
    let todo: TodoView =
        serde_json::from_value(raw.clone()).map_err(|e| format!("응답을 읽지 못했다: {e}"))?;
    printer.emit(&raw, || format!("✓ {} 생성 ({board})", todo.r#ref));
    Ok(())
}

/// `show` — 상세 + 댓글 + 히스토리.
pub fn cmd_show(
    ctx: &CliContext,
    rest: &[String],
    board: &str,
    printer: &Printer,
) -> Result<(), String> {
    let Some(id) = rest.first() else {
        return Err("usage: rocky show REF".into());
    };
    let raw = request_value(ctx, "GET", &todo_ref_path(id, "", board), None)?;
    let todo: TodoView = serde_json::from_value(raw.get("todo").cloned().unwrap_or(Value::Null))
        .map_err(|e| format!("응답을 읽지 못했다: {e}"))?;
    let history: Vec<HistoryEntry> =
        serde_json::from_value(raw.get("history").cloned().unwrap_or(json!([])))
            .unwrap_or_default();
    let comments: Vec<Comment> =
        serde_json::from_value(raw.get("comments").cloned().unwrap_or(json!([])))
            .unwrap_or_default();
    printer.emit(&raw, || {
        format_todo_show(&todo, &history, &comments, now_ms())
    });
    Ok(())
}

/// `edit` — 할 일 메타 수정(제목·설명·섹션·우선순위·마감·라벨·링크). 예전 이름은 `update` 였다 —
/// 그 이름은 플러그인·데몬 업데이트(`cmd_update`)로 옮겼다.
pub fn cmd_edit(
    ctx: &CliContext,
    rest: &[String],
    flags: &ParsedFlags,
    board: &str,
    printer: &Printer,
) -> Result<(), String> {
    let Some(id) = rest.first() else {
        return Err("usage: rocky edit REF [플래그]".into());
    };
    let body = compact(vec![
        ("title", s(flags.str_flag("title"))),
        ("description", s(flags.str_flag("desc"))),
        ("section", s(flags.str_flag("section"))),
        ("parentId", s(flags.str_flag("parent"))),
        ("priority", s(flags.str_flag("priority"))),
        ("due", s(flags.str_flag("due"))),
        ("labels", labels_value(flags)),
        ("links", links_value(flags)),
    ]);
    let raw = request_value(ctx, "PATCH", &todo_ref_path(id, "", board), Some(&body))?;
    let todo: TodoView =
        serde_json::from_value(raw.clone()).map_err(|e| format!("응답을 읽지 못했다: {e}"))?;
    printer.emit(&raw, || format!("✓ {} 수정", todo.r#ref));
    Ok(())
}

/// `comment` — todo 타임라인에 한 줄.
pub fn cmd_comment(
    ctx: &CliContext,
    rest: &[String],
    board: &str,
    printer: &Printer,
) -> Result<(), String> {
    let (Some(id), Some(body)) = (rest.first(), rest.get(1)) else {
        return Err("usage: rocky comment REF \"본문\"".into());
    };
    let raw = request_value(
        ctx,
        "POST",
        &todo_ref_path(id, "/comments", board),
        Some(&json!({ "body": body })),
    )?;
    printer.emit(&raw, || format!("✓ {id} 댓글 작성"));
    Ok(())
}

/// `start` / `stop` / `done` / `reopen` / `archive` / `unarchive`.
pub fn cmd_status(
    ctx: &CliContext,
    action: &str,
    rest: &[String],
    board: &str,
    printer: &Printer,
) -> Result<(), String> {
    let Some(id) = rest.first() else {
        return Err(format!("usage: rocky {action} REF"));
    };
    let raw = request_value(
        ctx,
        "POST",
        &todo_ref_path(id, "/status", board),
        Some(&json!({ "action": action })),
    )?;
    let todo: TodoView =
        serde_json::from_value(raw.clone()).map_err(|e| format!("응답을 읽지 못했다: {e}"))?;
    printer.emit(&raw, || format!("✓ {} {action}", todo.r#ref));
    Ok(())
}

/// `move` — 보드 이동(`--to`) 또는 순서 이동(`--before` / `--last`).
pub fn cmd_move(
    ctx: &CliContext,
    rest: &[String],
    flags: &ParsedFlags,
    board: &str,
    printer: &Printer,
) -> Result<(), String> {
    const USAGE: &str = "usage: rocky move REF --to BOARD | --before REF2 | --last";
    let Some(id) = rest.first() else {
        return Err(USAGE.into());
    };
    if let Some(to) = flags.str_flag("to") {
        let raw = request_value(
            ctx,
            "POST",
            &todo_ref_path(id, "/board", board),
            Some(&json!({ "board": to })),
        )?;
        let moved: TodoView =
            serde_json::from_value(raw.clone()).map_err(|e| format!("응답을 읽지 못했다: {e}"))?;
        printer.emit(&raw, || {
            format!("✓ {id} → {to} 보드로 이동 (새 참조 {})", moved.r#ref)
        });
        return Ok(());
    }
    let before = flags.str_flag("before");
    let last = flags.bool_flag("last");
    if before.is_none() && !last {
        return Err(USAGE.into());
    }
    // `--last` 는 명시적 null 로 보낸다 — 여기서만 null 이 "맨 끝"이라는 뜻이다.
    let body = json!({ "before": if last { Value::Null } else { json!(before) } });
    let raw = request_value(ctx, "POST", &todo_ref_path(id, "/move", board), Some(&body))?;
    let moved: TodoView =
        serde_json::from_value(raw.clone()).map_err(|e| format!("응답을 읽지 못했다: {e}"))?;
    printer.emit(&raw, || format!("✓ {} 순서 이동", moved.r#ref));
    Ok(())
}

/// `sessions` — 실행 중인 Claude Code 세션 (`*` 는 이 보드).
pub fn cmd_sessions(ctx: &CliContext, board: &str, printer: &Printer) -> Result<(), String> {
    let raw = request_value(
        ctx,
        "GET",
        &format!("/api/sessions?board={}", encode_uri_component(board)),
        None,
    )?;
    let view: SessionsView =
        serde_json::from_value(raw.clone()).map_err(|e| format!("응답을 읽지 못했다: {e}"))?;
    printer.emit(&raw, || format_sessions(&view));
    Ok(())
}

/// `pr [--all] [--json]` — 데몬이 기억하는 PR 스냅숏(열린 것만). 기본은 현재 보드의 레포.
pub fn cmd_pr(
    ctx: &CliContext,
    rest: &[String],
    flags: &ParsedFlags,
    board: &str,
    printer: &Printer,
) -> Result<(), String> {
    match rest.first().map(String::as_str) {
        Some("subscribe") | Some("unsubscribe") => return cmd_pr_subscribe(ctx, rest, flags, printer),
        Some("subscriptions") => {
            let raw_prs = request_value(ctx, "GET", "/api/prs/subscriptions", None)?;
            let raw_filters = request_value(ctx, "GET", "/api/prs/filters", None)?;
            let subs: Vec<rocky_core::prwatch::PrSubscription> =
                serde_json::from_value(raw_prs.clone())
                    .map_err(|e| format!("응답을 읽지 못했다: {e}"))?;
            let filters: Vec<rocky_core::prwatch::PrFilterSubscription> =
                serde_json::from_value(raw_filters.clone())
                    .map_err(|e| format!("응답을 읽지 못했다: {e}"))?;
            let raw = json!({ "prs": raw_prs, "filters": raw_filters });
            let who = |id: Option<&str>| {
                id.map(|id| format!("세션 {}", &id[..id.len().min(8)]))
                    .unwrap_or_else(|| "지켜보기만".to_string())
            };
            printer.emit(&raw, || {
                if subs.is_empty() && filters.is_empty() {
                    return "구독 없음 — rocky pr subscribe N · rocky pr subscribe --filter \"조건\""
                        .to_string();
                }
                let mut lines: Vec<String> = filters
                    .iter()
                    .map(|f| format!("필터 {}  \"{}\"  {}", f.id, f.query, who(f.session_id.as_deref())))
                    .collect();
                lines.extend(subs.iter().map(|s| {
                    let via = if s.filter_id.is_some() { " (필터)" } else { "" };
                    format!("{}#{}  {}{via}", s.repo, s.number, who(s.session_id.as_deref()))
                }));
                lines.join("\n")
            });
            return Ok(());
        }
        Some(other) => {
            return Err(format!(
                "usage: rocky pr [--board K|--all] | subscribe|unsubscribe N [--repo OWNER/NAME] | subscribe|unsubscribe --filter Q|ID | subscriptions ({other}?)"
            ))
        }
        None => {}
    }
    let path = if flags.bool_flag("all") {
        "/api/prs?open=true".to_string()
    } else {
        format!("/api/prs?open=true&board={}", encode_uri_component(board))
    };
    let raw = request_value(ctx, "GET", &path, None)?;
    let prs: Vec<rocky_core::prwatch::PrSnapshot> =
        serde_json::from_value(raw.clone()).map_err(|e| format!("응답을 읽지 못했다: {e}"))?;
    printer.emit(&raw, || format_prs(&prs));
    Ok(())
}

/// `pr subscribe|unsubscribe N [--repo OWNER/NAME]` — 데몬이 이 PR 을 보고 전이를 이 세션에 보내게 한다(구독) /
/// 그만 본다. 레포는 `--repo`, 없으면 지금 디렉터리의 `gh repo view`. 세션은 `CLAUDE_CODE_SESSION_ID` — 세션 밖
/// (터미널)에서 구독하면 세션 없이 지켜보기만 한다(알림 탭에는 뜨고 아무 세션도 깨우지 않는다).
fn cmd_pr_subscribe(
    ctx: &CliContext,
    rest: &[String],
    flags: &ParsedFlags,
    printer: &Printer,
) -> Result<(), String> {
    let sub = rest.first().map(String::as_str).unwrap_or("");
    if let Some(filter) = flags.str_flag("filter") {
        return cmd_pr_filter(ctx, sub, filter, printer);
    }
    let number: i64 = rest
        .get(1)
        .map(|n| n.trim_start_matches('#'))
        .and_then(|n| n.parse().ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| format!("usage: rocky pr {sub} N [--repo OWNER/NAME]"))?;
    let repo = match flags.str_flag("repo") {
        Some(r) => r.to_string(),
        None => current_repo()?,
    };
    if sub == "unsubscribe" {
        let path = format!(
            "/api/prs/subscriptions?repo={}&number={number}",
            encode_query(&repo)
        );
        let raw = request_value(ctx, "DELETE", &path, None)?;
        let removed = raw.get("removed").and_then(Value::as_bool).unwrap_or(false);
        printer.emit(&raw, || {
            if removed {
                format!("✓ {repo}#{number} 구독 해지")
            } else {
                format!("{repo}#{number} 은 구독하고 있지 않았다")
            }
        });
        return Ok(());
    }
    let session = std::env::var("CLAUDE_CODE_SESSION_ID")
        .ok()
        .filter(|s| !s.is_empty());
    let mut body = json!({ "repo": repo, "number": number });
    if let Some(id) = &session {
        body["sessionId"] = json!(id);
    }
    let raw = request_value(ctx, "POST", "/api/prs/subscriptions", Some(&body))?;
    printer.emit(&raw, || match &session {
        Some(_) => {
            format!("✓ {repo}#{number} 구독 — 리뷰·충돌·CI 실패·머지 후보·머지를 이 세션에 보낸다")
        }
        None => format!("✓ {repo}#{number} 지켜보기 — 세션 밖이라 깨울 세션 없이 보기만 한다"),
    });
    Ok(())
}

/// `pr subscribe --filter "조건"` / `pr unsubscribe --filter ID` — GitHub 검색 조건을 구독한다. 데몬이 3분마다 열린 PR 을
/// 그 조건으로 검색해 걸린 것을 이 세션의 PR 구독으로 넣는다(이미 누가 구독한 PR 은 빼앗지 않는다). 해지하면 그 필터로
/// 들어온 PR 구독도 걷힌다.
fn cmd_pr_filter(
    ctx: &CliContext,
    sub: &str,
    filter: &str,
    printer: &Printer,
) -> Result<(), String> {
    if sub == "unsubscribe" {
        let path = format!("/api/prs/filters?id={}", encode_query(filter));
        let raw = request_value(ctx, "DELETE", &path, None)?;
        let prs = raw.get("prs").and_then(Value::as_u64).unwrap_or(0);
        printer.emit(&raw, || {
            format!("✓ 필터 {filter} 해지 — 그 필터로 들어온 PR 구독 {prs}건도 걷었다")
        });
        return Ok(());
    }
    let session = std::env::var("CLAUDE_CODE_SESSION_ID")
        .ok()
        .filter(|s| !s.is_empty());
    let mut body = json!({ "query": filter });
    if let Some(id) = &session {
        body["sessionId"] = json!(id);
    }
    let raw = request_value(ctx, "POST", "/api/prs/filters", Some(&body))?;
    let id = raw
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("?")
        .to_string();
    printer.emit(&raw, || {
        format!(
            "✓ 필터 구독 {id} — \"{filter}\" 에 걸리는 열린 PR 을 {} (해지: rocky pr unsubscribe --filter {id})",
            if session.is_some() { "이 세션이 받는다" } else { "지켜보기만 한다" }
        )
    });
    Ok(())
}

/// 지금 디렉터리의 GitHub 레포(`owner/name`) — `gh repo view`.
fn current_repo() -> Result<String, String> {
    let out = std::process::Command::new("gh")
        .args([
            "repo",
            "view",
            "--json",
            "nameWithOwner",
            "-q",
            ".nameWithOwner",
        ])
        .output()
        .map_err(|e| {
            format!("gh 를 실행하지 못했다 — --repo OWNER/NAME 으로 주거나 gh 를 설치한다: {e}")
        })?;
    let repo = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || repo.is_empty() {
        return Err(format!(
            "지금 디렉터리의 레포를 모른다 — --repo OWNER/NAME 으로 준다 ({})",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(repo)
}

fn format_prs(prs: &[rocky_core::prwatch::PrSnapshot]) -> String {
    if prs.is_empty() {
        return "구독한 열린 PR 없음 — rocky pr subscribe N (또는 데몬이 아직 안 봄)".to_string();
    }
    prs.iter()
        .map(|p| {
            let mark = if p.ready {
                "✓ 확인·머지 가능"
            } else if p.merge_state == "DIRTY" {
                "✗ 충돌"
            } else if p.is_draft {
                "· draft"
            } else {
                "· 대기"
            };
            format!(
                "{mark}  {} #{}  {}  [ci {}{}{}]",
                p.repo,
                p.number,
                p.title,
                p.ci.as_str(),
                if p.unhandled > 0 {
                    format!(" · 미처리 {}", p.unhandled)
                } else {
                    String::new()
                },
                if p.decision > 0 {
                    format!(" · 결정 필요 {}", p.decision)
                } else {
                    String::new()
                },
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `spawn` — 그 todo 전용 워크트리에 새 세션.
pub fn cmd_spawn(
    ctx: &CliContext,
    rest: &[String],
    flags: &ParsedFlags,
    board: &str,
    printer: &Printer,
) -> Result<(), String> {
    let Some(id) = rest.first() else {
        return Err("usage: rocky spawn REF [--message \"본문\"]".into());
    };
    let body = match flags.str_flag("message") {
        Some(note) => json!({ "note": note }),
        None => json!({}),
    };
    let raw = request_value(
        ctx,
        "POST",
        &todo_ref_path(id, "/spawn", board),
        Some(&body),
    )?;
    let result: SpawnResult =
        serde_json::from_value(raw.clone()).map_err(|e| format!("응답을 읽지 못했다: {e}"))?;
    printer.emit(&raw, || format_spawn_result(id, &result));
    Ok(())
}

/// `section add|ls|archive`.
pub fn cmd_section(
    ctx: &CliContext,
    rest: &[String],
    board: &str,
    printer: &Printer,
) -> Result<(), String> {
    const USAGE: &str = "usage: rocky section add \"이름\" | section ls | section archive \"이름\"";
    let sub = rest.first().map(String::as_str).unwrap_or("");
    let arg = rest.get(1).map(|s| s.trim());

    match (sub, arg) {
        ("add", Some(title)) if !title.is_empty() => {
            // 보드를 자동 생성하지 않는다 — `--board` 오타로 빈 보드가 조용히 생기면
            // 서버가 없는 보드를 404 로 거절하는 취지가 무너진다.
            let section: Section = request(
                ctx,
                "POST",
                "/api/sections",
                Some(&json!({ "board": board, "title": title })),
            )?;
            printer.line(&format!("✓ 섹션: {}", section.title));
            Ok(())
        }
        ("archive", Some(wanted)) if !wanted.is_empty() => {
            // 서버는 title 을 trim 해 저장한다 — 인자에 공백이 붙어도 같은 섹션을 찾게 맞춘다.
            let sections: Vec<Section> = request(
                ctx,
                "GET",
                &format!("/api/sections?board={}", encode_uri_component(board)),
                None,
            )?;
            let Some(target) = sections.into_iter().find(|s| s.title == wanted) else {
                return Err(format!("섹션 없음: {wanted} (board: {board})"));
            };
            request_value(
                ctx,
                "POST",
                &format!("/api/sections/{}/archive", encode_uri_component(&target.id)),
                None,
            )?;
            printer.line(&format!(
                "✓ 섹션 보관: {} — 속해 있던 작업은 미분류로 돌아간다",
                target.title
            ));
            Ok(())
        }
        ("ls", _) => {
            let raw = request_value(
                ctx,
                "GET",
                &format!("/api/sections?board={}", encode_uri_component(board)),
                None,
            )?;
            let sections: Vec<Section> = serde_json::from_value(raw.clone()).unwrap_or_default();
            printer.emit(&raw, || {
                if sections.is_empty() {
                    "(섹션 없음)".to_string()
                } else {
                    sections
                        .iter()
                        .map(|s| format!("# {}", s.title))
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            });
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

/// `handoff` — 실행 중인 세션에 작업 요청. `--cancel` 이면 대기 중인 요청 취소.
pub fn cmd_handoff(
    ctx: &CliContext,
    rest: &[String],
    flags: &ParsedFlags,
    board: &str,
    printer: &Printer,
) -> Result<(), String> {
    let Some(id) = rest.first() else {
        return Err(
            "usage: rocky handoff REF [--session NAME] [--message \"본문\"] [--cancel]".into(),
        );
    };

    if flags.bool_flag("cancel") {
        // board 로 거르지 않는다 — 해석된 **실제 todo id** 로 찾으므로 필터가 불필요하고
        // 오히려 해롭다: `board` 는 cwd 유추값인데 REF 는 다른 보드를 가리킬 수 있어
        // (`other-12`), 거르면 실재하는 요청을 못 찾는다.
        let pending: Vec<rocky_core::types::Handoff> =
            request(ctx, "GET", "/api/handoffs?status=pending", None)?;
        let detail = request_value(ctx, "GET", &todo_ref_path(id, "", board), None)?;
        let todo_id = detail
            .get("todo")
            .and_then(|t| t.get("id"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("{id} 를 찾지 못했다"))?;
        let Some(target) = pending.into_iter().find(|h| h.todo_id == todo_id) else {
            return Err(format!("{id} 앞으로 대기 중인 요청이 없다"));
        };
        let raw = request_value(
            ctx,
            "POST",
            &format!("/api/handoffs/{}/cancel", encode_uri_component(&target.id)),
            None,
        )?;
        printer.emit(&raw, || format!("✓ {id} 핸드오프 취소"));
        return Ok(());
    }

    // `--session` 이 오면 이름 → sessionId 로 바꾼다. 활성 목록에 없으면 여기서 끊는다 —
    // 없는 세션에 큐잉하면 영영 배달되지 않는다.
    let mut session_id: Option<String> = None;
    if let Some(name) = flags.str_flag("session") {
        let view: SessionsView = request(
            ctx,
            "GET",
            &format!("/api/sessions?board={}", encode_uri_component(board)),
            None,
        )?;
        session_id = view
            .sessions
            .iter()
            .find(|s| s.name == name)
            .map(|s| s.session_id.clone());
        if session_id.is_none() {
            return Err(format!("활성 세션이 아니다: {name}"));
        }
    }

    let body = compact(vec![
        ("sessionId", session_id.map(|v| json!(v))),
        ("note", s(flags.str_flag("message"))),
    ]);
    let raw = request_value(
        ctx,
        "POST",
        &todo_ref_path(id, "/handoff", board),
        Some(&body),
    )?;
    let created: HandoffCreated =
        serde_json::from_value(raw.clone()).map_err(|e| format!("응답을 읽지 못했다: {e}"))?;
    printer.emit(&raw, || render_handoff_created(id, &created));
    Ok(())
}

/// `board ls|show|add|rename|title|desc|repo|path` — 보드 메타.
///
/// `show` 를 뺀 수정 명령은 전부 **cwd 로 유추한 보드**를 고친다. 다른 보드는
/// `--board KEY` 로 지정한다.
pub fn cmd_board(
    ctx: &CliContext,
    rest: &[String],
    board: &str,
    clear: bool,
    printer: &Printer,
) -> Result<(), String> {
    const USAGE: &str = "usage: rocky board ls | board show [KEY] | board add KEY [제목] | board rename NEWKEY | board title \"제목\" | board desc [\"설명\"] | board repo [OWNER/NAME] | board path [절대경로] | board review-fix on|off | board pr-authors @me|LOGIN... | board pr-authors --clear\n  show 를 뺀 수정 명령은 모두 cwd 로 유추한 보드를 고친다 — 다른 보드는 --board KEY 로 지정한다";
    let sub = rest.first().map(String::as_str).unwrap_or("ls");
    let arg = rest.get(1).map(String::as_str);

    let patch = |field: &str, value: Value| -> Result<(Value, Board), String> {
        let mut body = serde_json::Map::new();
        body.insert(field.to_string(), value);
        let raw = request_value(
            ctx,
            "PATCH",
            &board_detail_path(board),
            Some(&Value::Object(body)),
        )?;
        let updated: Board =
            serde_json::from_value(raw.clone()).map_err(|e| format!("응답을 읽지 못했다: {e}"))?;
        Ok((raw, updated))
    };

    match sub {
        "ls" => {
            let raw = request_value(ctx, "GET", "/api/boards", None)?;
            let boards: Vec<Board> = serde_json::from_value(raw.clone()).unwrap_or_default();
            printer.emit(&raw, || {
                if boards.is_empty() {
                    "(보드 없음)".to_string()
                } else {
                    boards
                        .iter()
                        .map(|b| {
                            let desc = b
                                .description
                                .as_deref()
                                .filter(|d| !d.is_empty())
                                .map(|d| format!(" — {d}"))
                                .unwrap_or_default();
                            format!("{}  {}{desc}", b.key, b.title)
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            });
            Ok(())
        }
        "add" => {
            let Some(key) = arg else {
                return Err(USAGE.into());
            };
            let body = compact(vec![
                ("key", Some(json!(key))),
                ("title", rest.get(2).map(|t| json!(t))),
            ]);
            let raw = request_value(ctx, "POST", "/api/boards", Some(&body))?;
            let created: Board = serde_json::from_value(raw.clone())
                .map_err(|e| format!("응답을 읽지 못했다: {e}"))?;
            printer.emit(&raw, || format!("✓ 보드 {}", created.key));
            Ok(())
        }
        "show" => {
            // 인자를 주면 그 보드, 없으면 cwd 로 유추한 보드.
            let wanted = arg.unwrap_or(board);
            let boards: Vec<Board> = request(ctx, "GET", "/api/boards", None)?;
            let found = boards
                .into_iter()
                .find(|b| {
                    b.key == wanted
                        || b.previous_keys
                            .as_ref()
                            .is_some_and(|keys| keys.iter().any(|k| k == wanted))
                })
                .ok_or_else(|| format!("보드 없음: {wanted}"))?;
            let raw = serde_json::to_value(&found).unwrap_or(Value::Null);
            printer.emit(&raw, || render_board(&found));
            Ok(())
        }
        "rename" => {
            let Some(new_key) = arg else {
                return Err(USAGE.into());
            };
            // key 는 참조 접두사(`rocky-12`)이자 cwd 유추 대상이지만, 옛 key 는 서버가
            // 별칭으로 남겨 계속 받는다 — 그래서 여기서 경고하지 않는다.
            let (raw, updated) = patch("key", json!(new_key))?;
            printer.emit(&raw, || {
                format!(
                    "✓ {board} → {} (옛 참조 {board}-N 은 계속 풀린다)",
                    updated.key
                )
            });
            Ok(())
        }
        "title" => {
            let Some(title) = arg else {
                return Err(USAGE.into());
            };
            let (raw, updated) = patch("title", json!(title))?;
            printer.emit(&raw, || format!("✓ {} → {}", updated.key, updated.title));
            Ok(())
        }
        "desc" => {
            // 인자 없이(또는 빈 문자열로) 부르면 설명을 지운다. 사람에게 둘은 같은
            // 의도인데 REST 는 `null` 만 "지운다"로 받고 빈 문자열은 400 이라 여기서 접는다.
            let next = match arg.map(str::trim).filter(|v| !v.is_empty()) {
                Some(value) => json!(value),
                None => Value::Null,
            };
            let (raw, updated) = patch("description", next)?;
            printer.emit(&raw, || {
                format!(
                    "✓ {} → {}",
                    updated.key,
                    updated.description.as_deref().unwrap_or("(설명 없음)")
                )
            });
            Ok(())
        }
        "repo" => {
            // 인자를 주면 그 값, 없으면 cwd 의 git remote 에서 유추한다.
            let inferred = crate::context::git(&["remote", "get-url", "origin"])
                .and_then(|url| parse_repo_from_remote(&url));
            let repo = arg.map(str::to_string).or(inferred);
            let repo = repo.filter(|r| is_repo_slug(r)).ok_or_else(|| {
                "GitHub 레포를 알 수 없다 — OWNER/NAME 을 직접 준다: rocky board repo OWNER/NAME".to_string()
            })?;
            let (raw, updated) = patch("repo", json!(repo))?;
            printer.emit(&raw, || {
                format!(
                    "✓ {} → {}",
                    updated.key,
                    updated.repo.as_deref().unwrap_or("")
                )
            });
            Ok(())
        }
        "path" => {
            // 인자를 주면 그 값, 없으면 지금 있는 자리 — 보통 레포 안에서 부른다.
            let target = match arg {
                Some(value) => value.to_string(),
                None => std::env::current_dir()
                    .map_err(|e| format!("cwd 를 알 수 없다: {e}"))?
                    .to_string_lossy()
                    .to_string(),
            };
            let (raw, updated) = patch("path", json!(target))?;
            printer.emit(&raw, || {
                format!(
                    "✓ {} → {}",
                    updated.key,
                    updated.path.as_deref().unwrap_or("")
                )
            });
            Ok(())
        }
        "review-fix" | "auto-resolve" => {
            // 그 레포의 세션이 자기 보드를 켠다 — 리뷰가 붙으면 데몬이 이 레포의 세션에
            // review-fix 를 시킨다. 설정 파일이 아니라 보드에 두는 이유가 이것이다.
            // 옛 이름 `auto-resolve` 는 한 릴리스 동안 받되 새 이름을 알린다(stderr — --json 출력을 더럽히지 않게).
            if sub == "auto-resolve" {
                eprintln!("rocky board auto-resolve 는 rocky board review-fix 로 이름이 바뀌었다 — 다음 릴리스에서 옛 이름은 빠진다");
            }
            let on = match arg {
                Some("on") => true,
                Some("off") => false,
                _ => return Err(USAGE.into()),
            };
            let (raw, updated) = patch("reviewFix", json!(on))?;
            printer.emit(&raw, || {
                let state = if updated.review_fix { "켬" } else { "끔" };
                let hint = if updated.review_fix && updated.repo.is_none() {
                    " — repo 가 없어 감시하지 않는다: rocky board repo"
                } else {
                    ""
                };
                format!("✓ {} reviewFix {state}{hint}", updated.key)
            });
            Ok(())
        }
        "pr-authors" => {
            // PR 감시가 **알릴** 작성자 — 기록(히스토리·rocky pr)은 필터와 무관하게 전부 남는다.
            let authors: Vec<String> = rest.iter().skip(1).map(|s| s.trim().to_string()).collect();
            // 둘 다 주면 무엇을 원했는지 모른다 — 필터를 건 줄 알았는데 지워지는 일이 없게 거부한다.
            // 로그인들 또는 --clear 중 하나만 — 둘 다(필터를 건 줄 알았는데 지워짐)나 둘 다 없음은 거부한다.
            if authors.is_empty() != clear {
                return Err(USAGE.into());
            }
            let value = if clear { Value::Null } else { json!(authors) };
            let (raw, updated) = patch("prAuthors", value)?;
            printer.emit(&raw, || {
                if updated.pr_authors.is_empty() {
                    format!("✓ {} PR 작성자 필터 없음 — 모든 PR 을 알린다", updated.key)
                } else {
                    format!(
                        "✓ {} PR 작성자 {} 의 PR 만 알린다(기록은 전부)",
                        updated.key,
                        updated.pr_authors.join(", ")
                    )
                }
            });
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

/// `history REF [--limit N] [--global|--note]`.
pub fn cmd_history(
    ctx: &CliContext,
    rest: &[String],
    flags: &ParsedFlags,
    board: &str,
    printer: &Printer,
) -> Result<(), String> {
    let Some(id) = rest.first() else {
        return Err("usage: rocky history REF [--limit N] [--global|--note]".into());
    };
    let limit = flags.str_flag("limit").unwrap_or("20");
    // prefix 로 들어와도 detail 조회로 전체 id 를 확정한 뒤 히스토리를 가져온다.
    let detail = resolve_history_entity(
        ctx,
        id,
        board,
        flags.bool_flag("global"),
        flags.bool_flag("note"),
    )?;
    let entity_id = detail
        .get("todo")
        .or_else(|| detail.get("note"))
        .and_then(|e| e.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or(id)
        .to_string();

    let raw = request_value(
        ctx,
        "GET",
        &format!(
            "/api/history?entityId={}&limit={}",
            encode_uri_component(&entity_id),
            encode_uri_component(limit)
        ),
        None,
    )?;
    let history: Vec<Value> = serde_json::from_value(raw.clone()).unwrap_or_default();
    printer.emit(&raw, || {
        history
            .iter()
            .map(|h| {
                let at = h.get("at").and_then(|v| v.as_str()).unwrap_or("");
                let actor = h.get("actor").and_then(|v| v.as_str()).unwrap_or("");
                let action = h.get("action").and_then(|v| v.as_str()).unwrap_or("");
                // changes 는 있을 때만, 서버가 준 JSON 을 그대로 붙인다.
                let changes = h
                    .get("changes")
                    .filter(|v| !v.is_null())
                    .map(|v| format!(" {}", serde_json::to_string(v).unwrap_or_default()))
                    .unwrap_or_default();
                format!(
                    "{} {actor} {action}{changes}",
                    at.chars().take(16).collect::<String>()
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    });
    Ok(())
}

/// todo 인지 note 인지 모르는 REF 를 상세 조회로 확정한다.
///
/// `--global`/`--note` 가 서면 note 로 확정한다. 아니면 todo 를 먼저 시도하고 실패하면
/// note 로 떨어진다 — 같은 보드에 todo #1 과 note #1 이 동시에 존재할 수 있어(번호 공간이
/// 독립) todo 가 먼저 성공하면 note 히스토리에 닿을 길이 없기 때문이다.
fn resolve_history_entity(
    ctx: &CliContext,
    id: &str,
    board: &str,
    global: bool,
    note: bool,
) -> Result<Value, String> {
    if global || note {
        return request_value(ctx, "GET", &note_ref_path(id, "", board, global), None);
    }
    match request_value(ctx, "GET", &todo_ref_path(id, "", board), None) {
        Ok(value) => Ok(value),
        Err(_) => request_value(ctx, "GET", &note_ref_path(id, "", board, false), None),
    }
}

/// `next` — 착수 후보 랭킹.
pub fn cmd_next(
    ctx: &CliContext,
    flags: &ParsedFlags,
    board: &str,
    printer: &Printer,
) -> Result<(), String> {
    use rocky_core::next::{
        format_next_candidates, rank_next, to_json_candidates, RankNextOptions, NEXT_DEFAULT_LIMIT,
    };

    let all_view = flags.bool_flag("all") && flags.str_flag("board").is_none();
    let qs = if all_view {
        String::new()
    } else {
        format!("?board={}", encode_uri_component(board))
    };
    // `ls` 와 같은 라우트다 — 서버가 doing 항목에 `doingState` 를 얹어 주므로 "주인 없는
    // 진행중" 판정을 클라이언트에서 다시 하지 않는다.
    let todos: Vec<TodoView> = request(ctx, "GET", &format!("/api/todos{qs}"), None)?;
    // 파싱 실패는 기본값으로 떨어진다 — TS 의 `Number.isNaN` 분기와 같다.
    let limit = flags
        .str_flag("limit")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(NEXT_DEFAULT_LIMIT);
    let candidates = rank_next(
        &todos,
        &RankNextOptions {
            now: now_ms(),
            limit: Some(limit),
        },
    );

    if printer.json {
        // JSON 은 컴팩트 형태로만 낸다 — 소비자가 모델이라 payload 가 곧 응답 지연이다.
        // 보드 목록은 ref 를 board key 로 되돌리기 위한 것.
        let boards: Vec<Board> = request(ctx, "GET", "/api/boards", None)?;
        let key_by_id: std::collections::HashMap<&str, &str> = boards
            .iter()
            .map(|b| (b.id.as_str(), b.key.as_str()))
            .collect();
        let json = json!({
            "board": if all_view { Value::Null } else { json!(board) },
            "candidates": to_json_candidates(&candidates, |id| key_by_id.get(id).map(|k| k.to_string())),
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&json).unwrap_or_default()
        );
        return Ok(());
    }
    println!("{}", format_next_candidates(&candidates));
    Ok(())
}

/// `note add|ls|show|edit|append|archive|pin|unpin` — 스크래치패드 메모.
///
/// `--global` 이 서면 board 컨텍스트를 아예 안 보낸다(`note_ref_path` 참고). 맨 번호를
/// 전역 메모로 풀려면 그 플래그가 필요하다 — 안 그러면 같은 번호의 보드 메모가 잡힌다.
pub fn cmd_note(
    ctx: &CliContext,
    rest: &[String],
    flags: &ParsedFlags,
    board: &str,
    printer: &Printer,
) -> Result<(), String> {
    let sub = rest.first().map(String::as_str).unwrap_or("");
    let global = flags.bool_flag("global");
    let arg = rest.get(1).map(String::as_str);

    match sub {
        "add" => {
            let Some(title) = arg.filter(|t| !t.is_empty()) else {
                return Err("usage: rocky note add \"제목\" [--content MD] [--global]".into());
            };
            let body = compact(vec![
                // `--global` 이면 board 키 자체를 안 실어 전역 메모가 된다.
                ("board", (!global).then(|| json!(board))),
                ("title", Some(json!(title))),
                ("content", s(flags.str_flag("content"))),
            ]);
            let raw = request_value(ctx, "POST", "/api/notes", Some(&body))?;
            let note: NoteView = serde_json::from_value(raw.clone())
                .map_err(|e| format!("응답을 읽지 못했다: {e}"))?;
            printer.emit(&raw, || format!("✓ 메모 {}", note.r#ref));
            Ok(())
        }
        "ls" => {
            let mut query: Vec<String> = Vec::new();
            if global {
                query.push("global=true".to_string());
            } else if !flags.bool_flag("all") {
                query.push(format!("board={}", encode_uri_component(board)));
            }
            if flags.bool_flag("archived") {
                query.push("includeArchived=true".to_string());
            }
            let qs = if query.is_empty() {
                String::new()
            } else {
                format!("?{}", query.join("&"))
            };
            let raw = request_value(ctx, "GET", &format!("/api/notes{qs}"), None)?;
            let notes: Vec<NoteView> = serde_json::from_value(raw.clone()).unwrap_or_default();
            printer.emit(&raw, || {
                if notes.is_empty() {
                    "(메모 없음)".to_string()
                } else {
                    notes
                        .iter()
                        .map(|n| {
                            let archived = if n.note.archived_at.is_some() {
                                " (보관됨)"
                            } else {
                                ""
                            };
                            let pin = if n.note.pinned_at.is_some() {
                                "📌 "
                            } else {
                                ""
                            };
                            format!("▤ {}  {pin}{}{archived}", n.r#ref, n.note.title)
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            });
            Ok(())
        }
        "show" => {
            let Some(id) = arg else {
                return Err("usage: rocky note show REF [--global]".into());
            };
            let raw = request_value(ctx, "GET", &note_ref_path(id, "", board, global), None)?;
            let note: NoteView =
                serde_json::from_value(raw.get("note").cloned().unwrap_or(Value::Null))
                    .map_err(|e| format!("응답을 읽지 못했다: {e}"))?;
            printer.emit(&raw, || {
                format!(
                    "▤ {}  {}\n\n{}\n\nid: {}",
                    note.r#ref, note.note.title, note.note.content, note.note.id
                )
            });
            Ok(())
        }
        "edit" => {
            let (Some(id), Some(content)) = (arg, flags.str_flag("content")) else {
                return Err(
                    "usage: rocky note edit REF --content MD [--title 제목] [--global]".into(),
                );
            };
            let body = compact(vec![
                ("title", s(flags.str_flag("title"))),
                ("content", Some(json!(content))),
            ]);
            let raw = request_value(
                ctx,
                "PATCH",
                &note_ref_path(id, "", board, global),
                Some(&body),
            )?;
            let note: NoteView = serde_json::from_value(raw.clone())
                .map_err(|e| format!("응답을 읽지 못했다: {e}"))?;
            printer.emit(&raw, || format!("✓ 메모 {} 수정", note.r#ref));
            Ok(())
        }
        "append" => {
            let (Some(id), Some(text)) = (arg, rest.get(2)) else {
                return Err("usage: rocky note append REF \"텍스트\" [--global]".into());
            };
            let raw = request_value(
                ctx,
                "PATCH",
                &note_ref_path(id, "", board, global),
                Some(&json!({ "content": text, "mode": "append" })),
            )?;
            let note: NoteView = serde_json::from_value(raw.clone())
                .map_err(|e| format!("응답을 읽지 못했다: {e}"))?;
            printer.emit(&raw, || format!("✓ 메모 {} append", note.r#ref));
            Ok(())
        }
        "archive" => {
            let Some(id) = arg else {
                return Err("usage: rocky note archive REF [--global]".into());
            };
            let raw = request_value(
                ctx,
                "POST",
                &note_ref_path(id, "/archive", board, global),
                None,
            )?;
            let note: NoteView = serde_json::from_value(raw.clone())
                .map_err(|e| format!("응답을 읽지 못했다: {e}"))?;
            printer.emit(&raw, || format!("✓ 메모 {} 보관", note.r#ref));
            Ok(())
        }
        "pin" | "unpin" => {
            let Some(id) = arg else {
                return Err(format!("usage: rocky note {sub} REF [--global]"));
            };
            let raw = request_value(
                ctx,
                "POST",
                &note_ref_path(id, &format!("/{sub}"), board, global),
                None,
            )?;
            let note: NoteView = serde_json::from_value(raw.clone())
                .map_err(|e| format!("응답을 읽지 못했다: {e}"))?;
            let done = if sub == "pin" {
                "고정"
            } else {
                "고정 해제"
            };
            printer.emit(&raw, || format!("✓ 메모 {} {done}", note.r#ref));
            Ok(())
        }
        _ => Err("usage: rocky note add|ls|show|edit|append|archive|pin|unpin".into()),
    }
}

// ── issue / open / daemon / mcp / tailscale ────────────────────────────────

/// 서버의 repo 미설정 에러인지 — **접두어만 정확히 맞춘다.** `includes` 로 느슨하게 잡으면
/// `gh` 의 `repo` 스코프 인증 실패까지 걸려, 보드 repo 를 조용히 덮어쓰고 진짜 원인을
/// 가린다.
pub fn is_missing_repo_error(message: &str) -> bool {
    message.starts_with("board has no GitHub repo")
}

/// `board has no GitHub repo: <key> — …` 에서 보드 key 를 꺼낸다.
///
/// cwd 에서 유추한 레포를 그 보드에 써도 되는지 판단하는 데 쓴다 — 다른 보드의 todo
/// 였다면 cwd 는 아무 관계가 없고, 그대로 진행하면 **엉뚱한 레포에 이슈가 올라간다**.
pub fn board_key_from_missing_repo_error(message: &str) -> Option<String> {
    let rest = message.strip_prefix("board has no GitHub repo: ")?;
    let (key, _) = rest.split_once(" — ")?;
    let trimmed = key.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// `issue REF [--repo OWNER/NAME]` — todo 를 GitHub 이슈로.
pub fn cmd_issue(
    ctx: &CliContext,
    rest: &[String],
    flags: &ParsedFlags,
    board: &str,
    printer: &Printer,
) -> Result<(), String> {
    let Some(id) = rest.first() else {
        return Err("usage: rocky issue REF [--repo OWNER/NAME]".into());
    };
    let path = todo_ref_path(id, "/issue", board);

    // repo 를 미리 PATCH 하지 않는다 — 서버가 ref 로 todo 의 진짜 보드를 알아서 그 위에
    // 저장한다. `--board` 로 유추한 board 는 cwd 기준이라 ref 가 다른 보드를 가리키면
    // 엉뚱한 보드가 조용히 바뀐다.
    if let Some(explicit) = flags.str_flag("repo") {
        if !is_repo_slug(explicit) {
            return Err(format!("--repo 는 OWNER/NAME 모양이어야 한다: {explicit}"));
        }
        let raw = request_value(ctx, "POST", &path, Some(&json!({ "repo": explicit })))?;
        let url = raw.get("url").and_then(|v| v.as_str()).unwrap_or("");
        printer.emit(&raw, || format!("✓ {url}"));
        return Ok(());
    }

    match request_value(ctx, "POST", &path, None) {
        Ok(raw) => {
            let url = raw.get("url").and_then(|v| v.as_str()).unwrap_or("");
            printer.emit(&raw, || format!("✓ {url}"));
            Ok(())
        }
        Err(message) => {
            // 보드에 repo 가 없을 때만 cwd 에서 유추해 한 번 더 POST 한다. cwd 유추는
            // cwd 보드와 todo 의 실제 보드가 같을 때만 안전하다 — 서버 메시지가 실토한
            // 보드 key 가 이 CLI 의 board 와 다르면 유추하지 않고 원래 에러를 그대로 던진다.
            let error_board = if is_missing_repo_error(&message) {
                board_key_from_missing_repo_error(&message)
            } else {
                None
            };
            let inferred = if error_board.as_deref() == Some(board) {
                crate::context::git(&["remote", "get-url", "origin"])
                    .and_then(|url| parse_repo_from_remote(&url))
            } else {
                None
            };
            let Some(repo) = inferred else {
                return Err(message);
            };
            let raw = request_value(ctx, "POST", &path, Some(&json!({ "repo": repo })))?;
            let url = raw.get("url").and_then(|v| v.as_str()).unwrap_or("");
            printer.emit(&raw, || {
                format!("✓ {url} (보드 repo 를 {repo} 로 설정했다)")
            });
            Ok(())
        }
    }
}

/// `open` — 접속 주소 출력. 링크를 눌러 여는 용도라 자동 실행은 없다.
pub fn cmd_open(ctx: &CliContext, expose_lan: bool, expose_tailscale: bool) -> Result<(), String> {
    crate::client::ensure_daemon(ctx)?;
    crate::system::print_addresses(&ctx.base_url, ctx.port, expose_lan, expose_tailscale);
    Ok(())
}

/// `daemon run|start|stop|restart|status|install|uninstall`.
pub fn cmd_daemon(
    ctx: &CliContext,
    rest: &[String],
    expose_lan: bool,
    expose_tailscale: bool,
) -> Result<(), String> {
    use crate::launchd::{
        install_launchd, is_launchd_registered, launchd_loaded, launchd_status, uninstall_launchd,
    };

    // 개발용 라벨이 실제 포트를 가리키면 멈춘다 — 그 상태의 restart 는 실제 상주 데몬을 "launchd 밖의 고아" 로
    // 보고 내리고, 개발용 job 이 실제 DB 로 그 자리를 잇는다.
    if rocky_core::config::launchd_label() != crate::launchd::LAUNCHD_LABEL
        && ctx.port == rocky_core::config::DEFAULT_TODO_PORT
    {
        return Err(format!(
            "ROCKY_LAUNCHD_LABEL 은 실제 포트({})가 아닌 전용 ROCKY_CONFIG(다른 port·dir)와 함께 써야 한다",
            ctx.port
        ));
    }
    match rest.first().map(String::as_str) {
        // 포그라운드 실행 — TS 는 daemon.ts 를 in-process import 했고, 여기서는 데몬
        // 바이너리로 프로세스를 교체한다(exec). 반환하면 그 자체가 실패다.
        Some("run") => {
            use std::os::unix::process::CommandExt;
            let error = std::process::Command::new(crate::client::daemon_binary()).exec();
            Err(format!("rockyd 를 실행하지 못했다: {error}"))
        }
        Some("start") => {
            crate::client::ensure_daemon(ctx)?;
            // "✓ daemon on" 만 보면 상주가 복구된 줄 안다 — 누가 띄운 프로세스인지를 적는다.
            // plist 만 있고 로드가 안 된 상태는 재부팅·크래시 뒤 데몬이 사라지는 상태다.
            let note = if launchd_loaded() {
                " (launchd 상주)".to_string()
            } else if is_launchd_registered() {
                "\n  launchd 밖에서 띄웠다 — plist 는 있으나 로드되지 않아 재부팅·크래시 뒤 살아나지 않는다 → rocky daemon install 로 다시 등록".to_string()
            } else {
                " — 온디맨드 프로세스 (로그인 시 상주는 rocky daemon install)".to_string()
            };
            println!("✓ daemon on {}{note}", ctx.base_url);
            Ok(())
        }
        Some("stop") => {
            // health 를 묻지 않고 pid 파일만 본다 — TS 와 같은 동작. 파일이 없으면
            // 이미 꺼져 있거나 다른 dir 로 뜬 것이다.
            let pid_file = ctx.dir.join("daemon.pid");
            match std::fs::read_to_string(&pid_file)
                .ok()
                .and_then(|raw| raw.trim().parse::<i32>().ok())
            {
                Some(pid) => {
                    // SAFETY: kill(2) 는 pid 와 시그널 번호만 받는다.
                    unsafe { libc::kill(pid, libc::SIGTERM) };
                    println!("✓ daemon(pid {pid}) 종료 — launchd install 상태면 곧 재기동된다");
                }
                None => println!(
                    "daemon pid 파일 없음 — 이미 꺼져 있거나 포트만 확인해 보자: daemon status"
                ),
            }
            Ok(())
        }
        // 버전 교체 훅과 같은 경로를 늘 탄다 — launchd 상주면 job 재등록(pid kill 은
        // KeepAlive 가 즉시 되살린다), 아니면 꺼질 때까지 기다린 뒤 띄운다. `stop && start`
        // 를 손으로 하면 이 둘을 사람이 골라야 하고, stop 이 끝나기 전에 start 가 health 를
        // 보고 "이미 떠 있다" 며 지나가기도 한다.
        Some("restart") => {
            use crate::hooks::{ensure_daemon_with_policy, with_live_deps, RestartPolicy};
            let before = crate::client::daemon_health(&ctx.base_url);
            if let Some(error) =
                with_live_deps(|deps| ensure_daemon_with_policy(ctx, deps, RestartPolicy::Always))
            {
                return Err(error);
            }
            // launchd 재등록은 job 이 로드된 것까지만 확인한다 — 새 데몬이 포트를 잡을 때까지
            // 잠깐 기다려야 새 버전을 적을 수 있다.
            let mut after = None;
            for _ in 0..20 {
                after = crate::client::daemon_health(&ctx.base_url);
                if after.is_some() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
            let version = |h: &Option<crate::client::DaemonHealth>| {
                h.as_ref()
                    .and_then(|h| h.version.clone())
                    .map(|v| format!("v{v}"))
                    .unwrap_or_else(|| "?".into())
            };
            let pid = after
                .as_ref()
                .and_then(|h| h.pid)
                .map(|p| format!(", pid {p}"))
                .unwrap_or_default();
            // 목표는 이 CLI 의 버전이다 — 다른 버전이 응답하면(launchd 밖의 고아가 포트를 쥔 채 남는 등)
            // 재시작했다고 말하지 않는다.
            let here = env!("CARGO_PKG_VERSION");
            if after.as_ref().and_then(|h| h.version.as_deref()) != Some(here) {
                return Err(format!(
                    "daemon 재시작 실패 — 목표 v{here}, 포트의 데몬은 {}{pid}. `rocky daemon status` 로 확인하라",
                    version(&after)
                ));
            }
            match before {
                Some(_) => println!(
                    "✓ daemon 재시작 {} → {}{pid}",
                    version(&before),
                    version(&after)
                ),
                None => println!(
                    "✓ daemon on {} — 떠 있지 않아 새로 띄웠다{pid}",
                    version(&after)
                ),
            }
            Ok(())
        }
        Some("status") => {
            let running = crate::client::daemon_health(&ctx.base_url);
            let alive = running.is_some();
            // 포트를 쥔 데몬이 launchd job 의 프로세스가 아니면 ✓ 로 두지 않는다 — 그 데몬은 업데이트가 못 바꾼다.
            let job = crate::launchd::launchd_job();
            let warning = match &running {
                Some(h) => crate::launchd::ownership_warning(h.pid, job.as_ref()),
                None => job.as_ref().and_then(crate::launchd::stalled_job_warning),
            };
            match &running {
                Some(h) => println!(
                    "{} running on {}{}",
                    if warning.is_some() { "⚠" } else { "✓" },
                    ctx.base_url,
                    match (h.version.as_deref(), h.pid) {
                        (Some(v), Some(p)) => format!(" (v{v}, pid {p})"),
                        (Some(v), None) => format!(" (v{v})"),
                        _ => String::new(),
                    }
                ),
                None => println!("✗ not running (port {})", ctx.port),
            }
            println!("{}", launchd_status());
            if let Some(warning) = warning {
                println!("{warning}");
            }
            if alive {
                println!("접속 주소:");
                crate::system::print_addresses(
                    &ctx.base_url,
                    ctx.port,
                    expose_lan,
                    expose_tailscale,
                );
            }
            Ok(())
        }
        Some("install") => {
            println!("{}", install_launchd()?);
            Ok(())
        }
        Some("uninstall") => {
            println!("{}", uninstall_launchd());
            Ok(())
        }
        _ => Err("usage: rocky daemon run|start|stop|restart|status|install|uninstall".into()),
    }
}

/// `mcp setup`.
pub fn cmd_mcp(ctx: &CliContext, rest: &[String]) -> Result<(), String> {
    if rest.first().map(String::as_str) == Some("setup") {
        println!("{}", crate::system::mcp_setup_guide(&ctx.base_url));
        return Ok(());
    }
    Err("usage: rocky mcp setup".into())
}

/// `tailscale on|off|status` — 옵션 기능, 기본 off. 이 커맨드를 쓰지 않으면 rocky
/// 는 tailscale 을 일절 건드리지 않는다.
pub fn cmd_tailscale(ctx: &CliContext, rest: &[String]) -> Result<(), String> {
    use crate::system::{tailscale_serve_off, tailscale_serve_on, tailscale_serve_status};
    match rest.first().map(String::as_str) {
        Some("on") => {
            println!("{}", tailscale_serve_on(ctx.port));
            Ok(())
        }
        Some("off") => {
            println!("{}", tailscale_serve_off());
            Ok(())
        }
        Some("status") | None => {
            println!("{}", tailscale_serve_status());
            Ok(())
        }
        Some(_) => Err("usage: rocky tailscale on|off|status".into()),
    }
}

/// `today [--json]` — 보드 요약 몇 줄. SessionStart 훅이 넣는 것과 같은 문자열이라, Claude Code 의
/// `!` 모드(`! rocky today`)로 LLM 턴 없이 볼 수 있다. 수집함은 어댑터를 실행해(Normal) 캐시를 데운다.
pub fn cmd_today(ctx: &CliContext, printer: &Printer) -> Result<(), String> {
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let summary: rocky_core::summary::Summary = request(
        ctx,
        "GET",
        &format!("/api/summary?cwd={}", encode_query(&cwd)),
        None,
    )?;
    let raw = serde_json::to_value(&summary).unwrap_or(Value::Null);
    printer.emit(&raw, || rocky_core::summary::render_summary(&summary));
    Ok(())
}

/// `inbox [--json]` — 수집함 소스별 항목. `GET /api/inbox` 를 그대로 읽는다(캐시가 비었으면 어댑터를
/// 실제로 돌리므로 몇 초 걸릴 수 있다). ✓ 는 이미 보드에 올라간 것(전 보드·보관 포함의 링크로 데몬이
/// 판정 — `rocky today` 와 같은 값).
pub fn cmd_inbox(ctx: &CliContext, rest: &[String], printer: &Printer) -> Result<(), String> {
    match rest.first().map(String::as_str) {
        None => {
            let inbox: rocky_core::inbox::InboxResponse = request(ctx, "GET", "/api/inbox", None)?;
            let raw = serde_json::to_value(&inbox).unwrap_or(Value::Null);
            printer.emit(&raw, || rocky_core::inbox::render_inbox(&inbox));
            Ok(())
        }
        Some("subscribe") => {
            let source = rest
                .get(1)
                .ok_or("usage: rocky inbox subscribe <소스 이름>")?;
            let (session_id, socket) = this_session()?;
            let res: Value = request(
                ctx,
                "POST",
                "/api/inbox/subscriptions",
                Some(&json!({
                    "source": source,
                    "sessionId": session_id,
                    "socket": socket,
                    "cwd": std::env::current_dir().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
                })),
            )?;
            printer.emit(&res, || {
                format!(
                    "✓ {source} 구독 — 지금 있는 {}건은 본 것으로 두고, 새 항목부터 이 세션에 알린다(5분마다 확인)",
                    res["baseline"].as_i64().unwrap_or(0)
                )
            });
            Ok(())
        }
        Some("unsubscribe") => {
            let (session_id, _) = this_session()?;
            let mut path = format!(
                "/api/inbox/subscriptions?sessionId={}",
                encode_query(&session_id)
            );
            if let Some(source) = rest.get(1) {
                path.push_str(&format!("&source={}", encode_query(source)));
            }
            let res: Value = request(ctx, "DELETE", &path, None)?;
            printer.emit(&res, || {
                format!("✓ 구독 해지 {}건", res["removed"].as_i64().unwrap_or(0))
            });
            Ok(())
        }
        Some(other) => Err(format!(
            "usage: rocky inbox [subscribe <소스>|unsubscribe [소스]] — 모르는 하위 명령: {other}"
        )),
    }
}

/// 이 명령을 부른 Claude Code 세션 — 세션 id 와 받은편지함 소켓. Claude Code 가 Bash 에 export 한다.
fn this_session() -> Result<(String, String), String> {
    let get = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    match (get("CLAUDE_CODE_SESSION_ID"), get("CLAUDE_CODE_MESSAGING_SOCKET")) {
        (Some(id), Some(socket)) => Ok((id, socket)),
        _ => Err("Claude Code 세션 안에서만 구독할 수 있다 — CLAUDE_CODE_SESSION_ID·CLAUDE_CODE_MESSAGING_SOCKET 이 없다".into()),
    }
}

/// `statusline [--cwd P] [--session S] [--full]` — rocky 한 줄(데몬이 렌더). 둘 다 없으면 Claude Code 가 statusline
/// 명령에 주는 stdin JSON(`workspace.current_dir`·`session_id`)에서 읽는다. cc-usage `extra_commands` 에는
/// `["rocky","statusline","--cwd","{{cwd}}","--session","{{session_id}}"]` 로 넣는다. 보여줄 게 없거나 데몬이
/// 없으면 아무것도 출력하지 않고 성공으로 끝난다 — statusline 이 에러 줄을 띄우지 않게.
///
/// `--full` 이면 cc-usage 와 같은 경로·git·모델·한도 줄을 CLI 가 직접 그리고 그 아래에 보드 줄을 붙인다
/// (`statusline_full`).
pub fn cmd_statusline(
    ctx: &CliContext,
    cwd: Option<&str>,
    session: Option<&str>,
    full: bool,
    source: Option<&str>,
) {
    if full {
        statusline_full(ctx, source);
        return;
    }
    let (cwd, session) = if cwd.is_none() && session.is_none() {
        let raw = statusline_stdin().unwrap_or_default();
        let input = serde_json::from_str::<Value>(&raw).unwrap_or(Value::Null);
        let text = |v: Option<&Value>| v.and_then(Value::as_str).map(str::to_string);
        (
            text(
                input
                    .pointer("/workspace/current_dir")
                    .or_else(|| input.get("cwd")),
            ),
            text(input.get("session_id")),
        )
    } else {
        (cwd.map(str::to_string), session.map(str::to_string))
    };
    if let Some(line) = board_line(ctx, cwd.as_deref(), session.as_deref()) {
        println!("{line}");
    }
}

/// 데몬이 렌더한 보드 줄 — 데몬이 없거나 300ms 안에 못 받으면 `None`.
fn board_line(ctx: &CliContext, cwd: Option<&str>, session: Option<&str>) -> Option<String> {
    let mut query = Vec::new();
    if let Some(cwd) = cwd.filter(|c| !c.is_empty()) {
        query.push(format!("cwd={}", encode_query(cwd)));
    }
    if let Some(session) = session.filter(|s| !s.is_empty()) {
        query.push(format!("session={}", encode_query(session)));
    }
    crate::client::statusline_line(&ctx.base_url, &query.join("&"))
}

/// `statusline --full` — cc-usage statusline 과 같은 바이트를 낸다(설계 `docs/design/specs/2026-10-05-cc-usage-mirror-design.md`).
/// 렌더는 CLI 가 하므로 데몬이 없어도 경로·모델·한도 줄은 남는다. 그 아래로 `extraCommands` 의 줄(설정 순서), 맨 아래
/// 보드 줄 — 둘 다 늦거나 실패하면 그 줄만 빠진다. 하위 프로세스·데몬 조회는 렌더와 나란히 돈다(가장 느린 것 하나만
/// 기다린다). 설정은 `rocky.json` 최상위 `statusline` 블록이다.
///
/// `source` 는 이번 실행에서만 설정의 `source` 를 바꾼다(`--source`, 환경 변수 `ROCKY_STATUSLINE_SOURCE` 보다 이긴다).
/// 모르는 값이면 무엇이 틀렸는지 한 줄만 낸다 — statusline 은 비지 않는다. `none` 이거나 Antigravity(`agy`)가 부른
/// 것이면 Claude 쪽(토큰·API·캐시·계정 파일)을 건드리지 않는다(`limits::local_limits`).
fn statusline_full(ctx: &CliContext, source: Option<&str>) {
    use rocky_core::limits::{local_limits, override_source, Input, SOURCE_ENV};
    use rocky_core::statusline::extra::Vars;
    use rocky_core::statusline::full::{lines, Style, View};

    let input = Input::parse(&statusline_stdin().unwrap_or_default());
    let mut cfg =
        rocky_core::config::load_statusline_block(&rocky_core::config::user_config_path());
    let env_source = std::env::var(SOURCE_ENV).ok();
    match override_source(cfg.limits.source, env_source.as_deref(), source) {
        Ok(source) => cfg.limits.source = source,
        Err(e) => {
            println!("[rocky] {e}");
            return;
        }
    }
    let now = statusline_now();
    let vars = Vars {
        session_id: &input.session_id,
        cwd: input.dir(),
    };
    let out = std::thread::scope(|scope| {
        let board = scope.spawn(|| board_line(ctx, Some(input.dir()), Some(&input.session_id)));
        let extras: Vec<_> = cfg
            .extra_commands
            .iter()
            .map(|c| scope.spawn(move || crate::bounded::run_extra(c, vars).1.into_lines()))
            .collect();

        let limits_cfg = &cfg.limits;
        let local = local_limits(limits_cfg, &input, now);
        let observed = match local {
            Some(_) => None,
            None => {
                crate::statusline_cache::observe(limits_cfg, cfg.config_dir.as_deref(), &input, now)
            }
        };
        let limits = local.map_or_else(
            || observed.as_ref().map(|o| o.limits).unwrap_or_default(),
            |(lim, _)| lim,
        );
        let cache = observed.as_ref().map(|o| &o.usage);
        let badge = observed
            .as_ref()
            .and_then(|o| o.email.as_deref())
            .and_then(|email| cfg.badges.get(email));
        let home = std::env::var("HOME").ok();
        let git = crate::git_status::read(input.dir(), crate::git_status::GIT_TIMEOUT);
        let view = View {
            dir: input.dir(),
            git: git.as_ref(),
            model: &input.model,
            effort: &input.effort,
            context_pct: input.context_pct,
            limits,
            usage: cache,
            alert: local.map_or_else(
                || observed.as_ref().map(|o| o.alert).unwrap_or_default(),
                |(_, alert)| alert,
            ),
            credits: observed.as_ref().map(|o| o.credits).unwrap_or_default(),
            currency: limits_cfg.currency(),
            badge,
            credit_glow: observed.as_ref().and_then(|o| o.credit_glow),
            home: home.as_deref(),
            now,
        };
        let style = Style::from_env(|k| std::env::var(k).ok());
        // 다른 도구의 줄은 바이트 그대로 붙여야 해서(UTF-8 이 아닐 수 있다) 줄을 바이트로 모은다.
        let mut out: Vec<Vec<u8>> = lines(&view, &style, &chrono::Local)
            .into_iter()
            .map(String::into_bytes)
            .collect();
        for extra in extras {
            out.extend(extra.join().unwrap_or_default());
        }
        out.extend(board.join().ok().flatten().map(String::into_bytes));
        out
    });
    let mut bytes = out.join(&b'\n');
    bytes.push(b'\n');
    use std::io::Write;
    let _ = std::io::stdout().write_all(&bytes);
}

/// 테스트 전용 — RFC3339 시각을 주면 "지금" 을 고정한다(골든 대조용, cc-usage 의 `CC_USAGE_NOW` 와 같은 자리).
const STATUSLINE_NOW_ENV: &str = "ROCKY_STATUSLINE_NOW";

pub fn statusline_now() -> chrono::DateTime<chrono::Utc> {
    std::env::var(STATUSLINE_NOW_ENV)
        .ok()
        .and_then(|v| chrono::DateTime::parse_from_rfc3339(&v).ok())
        .map_or_else(chrono::Utc::now, |t| t.with_timezone(&chrono::Utc))
}

/// Claude Code 의 statusline 입력 원문 — 터미널에서 그냥 부르면(stdin 이 TTY) 읽지 않는다. 4MB 까지만 읽는다.
fn statusline_stdin() -> Option<String> {
    use std::io::{IsTerminal, Read};
    if std::io::stdin().is_terminal() {
        return None;
    }
    let mut raw = String::new();
    std::io::stdin()
        .take(4 << 20)
        .read_to_string(&mut raw)
        .ok()?;
    Some(raw)
}

/// 플러그인이 설치되는 마켓플레이스 레포 — 최신 릴리스 태그를 여기서 읽는다.
const RELEASE_REPO: &str = "minjun0219/rocky";

/// `update` 에 할 일 수정의 흔적(REF·수정 플래그)이 있나 — 예전 `rocky update REF --title …` 을 습관대로 부르면
/// 업데이트를 돌리지 않고 `edit` 으로 안내한다(플러그인을 올리고 데몬을 교체하는 일은 되돌리기 어렵다).
pub fn looks_like_todo_edit(rest: &[String], flags: &ParsedFlags) -> bool {
    !rest.is_empty()
        || [
            "title", "desc", "section", "parent", "priority", "due", "label", "link",
        ]
        .iter()
        .any(|f| flags.str_flag(f).is_some())
}

/// `update [--check]` — 마켓플레이스 갱신 → 플러그인 올리기 → 새 버전 바이너리로 데몬 교체. 릴리스마다 손으로
/// 하던 네 단계다. `--check` 면 설치·데몬·최신 릴리스 버전만 비교하고 아무것도 바꾸지 않는다.
pub fn cmd_update(ctx: &CliContext, check: bool) -> Result<(), String> {
    let latest = latest_release()?;
    let running = crate::client::daemon_health(&ctx.base_url).and_then(|h| h.version);
    let here = env!("CARGO_PKG_VERSION");
    println!(
        "최신 릴리스 {latest} · 이 CLI {here} · 데몬 {}",
        running.as_deref().unwrap_or("(없음)")
    );
    if check {
        return Ok(());
    }
    if here == latest && running.as_deref() == Some(latest.as_str()) {
        println!("✓ 이미 최신이다");
        return Ok(());
    }
    run_claude(&["plugin", "marketplace", "update", "rocky-marketplace"])?;
    run_claude(&["plugin", "update", "rocky@rocky-marketplace"])?;
    // 지금 도는 이 rocky 는 옛 바이너리다 — 새 버전 폴더의 부트스트랩이 새 바이너리를 받아 데몬을 교체하고
    // `~/.local/bin/rocky` 링크도 새 버전으로 옮긴다.
    let bootstrap = plugin_cache_dir().join(&latest).join("bin/rocky");
    if !bootstrap.is_file() {
        return Err(format!(
            "플러그인은 올렸는데 새 버전 폴더가 없다 — {} (claude plugin list 로 확인)",
            bootstrap.display()
        ));
    }
    let mut child = std::process::Command::new(&bootstrap)
        .args(["hook", "ensure-daemon"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("{} 를 실행하지 못했다: {e}", bootstrap.display()))?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(b"{}");
    }
    let _ = child.wait();
    let after = crate::client::daemon_health(&ctx.base_url).and_then(|h| h.version);
    // 바이너리를 못 받았거나(릴리스 산출물이 아직 없음) launchd 밖의 옛 데몬이 포트를 쥔 채 남으면 데몬은
    // 그대로다 — 그때 ✓ 를 찍으면 업데이트된 줄 안다.
    if after.as_deref() != Some(latest.as_str()) {
        return Err(format!(
            "데몬이 v{latest} 로 바뀌지 않았다 — 지금 {}. 위 메시지(다운로드·교체 실패)를 보고, `rocky daemon status` 로 확인한 뒤 다시 `rocky update`",
            after
                .as_deref()
                .map(|v| format!("v{v}"))
                .unwrap_or_else(|| "데몬 없음".into())
        ));
    }
    println!(
        "✓ 데몬 {} → {}",
        running.as_deref().unwrap_or("(없음)"),
        after.as_deref().unwrap_or("(없음)")
    );
    println!(
        "  열려 있는 세션은 /reload-plugins 또는 재시작해야 새 플러그인(훅·커맨드)이 적용된다."
    );
    Ok(())
}

/// GitHub 의 최신 릴리스 버전(`v` 뺀 것).
fn latest_release() -> Result<String, String> {
    let url = format!("https://api.github.com/repos/{RELEASE_REPO}/releases/latest");
    let mut response = ureq::get(&url)
        .header("user-agent", "rocky-cli")
        .call()
        .map_err(|e| format!("최신 릴리스를 못 읽었다({url}): {e}"))?;
    let body: Value = response
        .body_mut()
        .read_json()
        .map_err(|e| format!("최신 릴리스 응답이 JSON 이 아니다: {e}"))?;
    body.get("tag_name")
        .and_then(Value::as_str)
        .map(|t| t.trim_start_matches('v').to_string())
        .ok_or_else(|| "최신 릴리스에 tag_name 이 없다".to_string())
}

/// 플러그인 캐시 — `CLAUDE_CONFIG_DIR` 가 있으면 그 아래.
fn plugin_cache_dir() -> std::path::PathBuf {
    let base = std::env::var("CLAUDE_CONFIG_DIR")
        .ok()
        .filter(|d| !d.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| rocky_core::config::expand_tilde("~/.claude"));
    base.join("plugins/cache/rocky-marketplace/rocky")
}

/// `claude <args>` — 출력은 그대로 사용자에게. 없거나 실패하면 무엇을 못 했는지 한 줄로.
fn run_claude(args: &[&str]) -> Result<(), String> {
    let status = std::process::Command::new("claude")
        .args(args)
        .status()
        .map_err(|e| format!("claude CLI 를 실행하지 못했다(PATH 확인): {e}"))?;
    if !status.success() {
        return Err(format!("claude {} 실패 ({status})", args.join(" ")));
    }
    Ok(())
}

/// 쿼리 값 인코딩 — 경로에 공백·한글이 올 수 있다.
fn encode_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
