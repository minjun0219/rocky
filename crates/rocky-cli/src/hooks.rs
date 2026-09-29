//! Claude Code 훅 엔트리 — `rocky hook <이름>`.
//! TS 원본 `hooks/ensure-daemon.ts` / `hooks/notify-todo.ts` / `hooks/handoff-stop.ts`.
//!
//! 세 훅 모두 **fail-open** 이다: 데몬이 죽어 있거나 어떤 에러든 조용히 exit 0 —
//! 훅 실패가 세션 시작·프롬프트 처리·턴 종료를 막지 않는다. 그래서 이 모듈의 함수는
//! `Result` 를 내지 않는다.

use std::io::Read;
use std::time::Duration;

use rocky_core::config::{load_worklog_config, user_config_path};
use rocky_core::handoff::build_handoff_prompt;
use rocky_core::notify::{
    build_notify_context, build_pr_context, filter_human_changes, merge_context,
    pr_entries_for_board, read_cursor, write_cursor,
};
use rocky_core::transcript::{build_turn_content, extract_turn, should_capture};
use rocky_core::types::{ChangesSince, ClaimedHandoff};
use rocky_core::worklog::{Worklog, WorklogAppendInput};
use serde_json::json;

use crate::client::{daemon_health, ensure_daemon, stop_daemon, CliContext};
use crate::launchd::{install_launchd, is_launchd_registered};

/// 훅의 HTTP 는 짧게 끊는다 — 프롬프트 지연이 곧 사용자 체감이다.
const HOOK_TIMEOUT: Duration = Duration::from_millis(1500);

fn read_stdin_json() -> serde_json::Value {
    let mut raw = String::new();
    let _ = std::io::stdin().read_to_string(&mut raw);
    serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null)
}

fn hook_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(HOOK_TIMEOUT))
        .build()
        .into()
}

/// 이 세션 앞의 핸드오프 한 건을 집어온다. 없거나 실패하면 `None` (fail-open).
fn claim_handoff(base_url: &str, session_id: &str, via: &str) -> Option<ClaimedHandoff> {
    let mut response = hook_agent()
        .post(format!("{base_url}/api/handoffs/claim"))
        .header("content-type", "application/json")
        .send_json(json!({ "sessionId": session_id, "via": via }))
        .ok()?;
    if response.status().as_u16() != 200 {
        return None;
    }
    response.body_mut().read_json().ok()
}

/// 이 세션이 일하는 보드 — 훅 입력의 cwd 를 보드 목록에 대 본다(statusline 과 같은 규칙: `boards.path`
/// 하위 → key 세그먼트). 데몬이 없거나 어느 보드로도 안 풀리면 None.
fn session_board_key(base_url: &str, input: &serde_json::Value) -> Option<String> {
    let cwd = input
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().to_string())
        })?;
    let mut response = hook_agent()
        .get(format!("{base_url}/api/boards"))
        .call()
        .ok()?;
    let boards: Vec<serde_json::Value> = response.body_mut().read_json().ok()?;
    let locations: Vec<rocky_core::statusline::BoardLocation> = boards
        .iter()
        .filter_map(|b| {
            Some(rocky_core::statusline::BoardLocation {
                key: b.get("key")?.as_str()?.to_string(),
                path: b.get("path").and_then(|p| p.as_str()).map(str::to_string),
            })
        })
        .collect();
    rocky_core::statusline::board_key_for_cwd(&locations, Some(&cwd))
}

fn fetch_changes(base_url: &str, since_id: i64, limit: i64) -> Option<ChangesSince> {
    let mut response = hook_agent()
        .get(format!(
            "{base_url}/api/changes?sinceId={since_id}&limit={limit}"
        ))
        .call()
        .ok()?;
    if !(200..300).contains(&response.status().as_u16()) {
        return None;
    }
    response.body_mut().read_json().ok()
}

/// `hook_ensure_daemon` 의 주입점 — TS 의 `EnsureDeps` 대응. 테스트가 실제 spawn/
/// SIGTERM/launchd 없이 stale 분기들을 검증할 수 있게 한다.
pub struct EnsureDeps<'a> {
    /// 이 설치본의 버전 — 데몬이 보고한 값과 다르면 stale 로 본다.
    pub version: &'a str,
    pub check_health: &'a dyn Fn(&str) -> Option<crate::client::DaemonHealth>,
    /// detached spawn + health 대기. 실패 사유를 돌려준다 — 경고에 실린다.
    pub spawn: &'a dyn Fn(&CliContext) -> Result<(), String>,
    /// 구버전 데몬 종료. 성공 여부를 돌려준다.
    pub stop: &'a dyn Fn(&CliContext, Option<u32>) -> bool,
    /// launchd(KeepAlive) 상주 등록 여부.
    pub is_managed: &'a dyn Fn() -> bool,
    /// 상주 job 을 현재 설치 경로로 교체 (plist 갱신→bootout→bootstrap). `Err` 면 옛 job 은
    /// 이미 내려갔을 수 있다 — 호출자가 health 를 다시 보고 판단한다.
    pub replace_managed: &'a dyn Fn() -> Result<(), String>,
}

/// SessionStart(startup): 데몬이 없으면 띄우고, **구버전이면** 내리고 현재 버전으로
/// 재기동한다. 실제 배선 — 판정은 `ensure_daemon_with` 에 있다.
pub fn hook_ensure_daemon(ctx: &CliContext, session_summary: Option<bool>) {
    let warning = with_live_deps(|deps| ensure_daemon_with(ctx, deps));
    // 재기동이 반쯤 실패한 것(launchd 재등록 실패 등)은 조용히 넘기지 않는다 — SessionStart 의
    // stdout 은 세션 컨텍스트라 요약 위에 한 줄 얹으면 에이전트도 사람도 본다. 훅은 여전히 0.
    if let Some(warning) = warning {
        eprintln!("rocky: {warning}");
        println!("⚠ rocky 데몬: {warning}");
    }
    // 세션 컨텍스트에 보드 요약 몇 줄 — `rocky today` 와 같은 문자열. SessionStart 의 stdout 은
    // 컨텍스트로 들어간다. 데몬이 아직 안 떴거나 실패하면 조용히 넘어간다(fail-open).
    if session_summary.unwrap_or(true) {
        print_session_summary(ctx);
    }
}

fn print_session_summary(ctx: &CliContext) {
    let input = read_stdin_json();
    let cwd = input
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().to_string())
        })
        .unwrap_or_default();
    let mut encoded = String::with_capacity(cwd.len());
    for b in cwd.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                encoded.push(b as char)
            }
            _ => encoded.push_str(&format!("%{b:02X}")),
        }
    }
    let Ok(summary) = crate::client::request::<rocky_core::summary::Summary>(
        ctx,
        "GET",
        &format!("/api/summary?cwd={encoded}"),
        None,
    ) else {
        return;
    };
    println!("{}", rocky_core::summary::render_summary(&summary));
}

/// 실제 배선 — SessionStart 와 매 턴 훅이 같은 의존을 쓴다.
fn with_live_deps<R>(f: impl FnOnce(&EnsureDeps) -> R) -> R {
    f(&EnsureDeps {
        version: env!("CARGO_PKG_VERSION"),
        check_health: &daemon_health,
        spawn: &ensure_daemon,
        stop: &stop_daemon,
        is_managed: &is_launchd_registered,
        replace_managed: &|| install_launchd().map(|_| ()),
    })
}

/// 매 턴 훅 — `hook_ensure_daemon` 과 같은 의존, 정책만 `OnlyIfOlder`. 경고는 호출자가
/// additionalContext 로 싣는다.
fn upgrade_daemon_if_older(ctx: &CliContext) -> Option<String> {
    with_live_deps(|deps| ensure_daemon_with_policy(ctx, deps, RestartPolicy::OnlyIfOlder))
}

/// 버전 비교는 정확 문자열 일치다 — 데몬 프로세스는 자기를 띄운 설치본보다 오래 살아,
/// 플러그인이 갱신돼도 옛 코드가 계속 돈다. version 미보고(≤0.1.0)도 stale 취급.
/// launchd(KeepAlive) 상주면 PID kill 은 무의미하다(즉시 되살아난다) — job 자체를 현재
/// 설치 경로로 교체한다. 못 내리면 재기동하지 않는다: 보드가 없는 것보다 구버전이라도
/// 있는 게 낫다.
pub fn ensure_daemon_with(ctx: &CliContext, deps: &EnsureDeps) -> Option<String> {
    ensure_daemon_with_policy(ctx, deps, RestartPolicy::ExactVersion)
}

/// 언제 재기동하는가.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartPolicy {
    /// SessionStart: 버전이 **다르면** 전부 — 없으면 띄운다. 의도적 다운그레이드(옛 플러그인
    /// 재설치)도 따라간다.
    ExactVersion,
    /// 매 턴(UserPromptSubmit): 데몬이 나보다 **오래됐을 때만** 올린다. 없으면 건드리지 않고
    /// (`rocky daemon stop` 뒤 개발자가 자기 데몬을 띄우는 흐름을 안 깨려고), 나보다 새 데몬도
    /// 그대로 둔다 — 안 그러면 옛 플러그인으로 도는 세션과 새 세션이 턴마다 서로 뒤집는다.
    OnlyIfOlder,
}

/// 판정 + 배선. 돌려주는 문자열은 **사람에게 알려야 할 것**이다 — 재기동의 어느 단계가
/// 실패해 데몬이 없거나 구버전으로 남았을 때. `None` 이면 조용히 지나간 것(정상).
///
/// launchd 교체가 실패한 경우가 핵심이다: `install_launchd` 는 옛 job 을 먼저 내리고 새
/// job 을 올리므로, 실패하면 데몬이 **통째로 사라진** 채일 수 있다. 그때는 launchd 밖에서라도
/// 띄운다(보드가 없는 것보다 낫다) — 대신 그 사실을 돌려줘 `rocky daemon install` 로 다시
/// 등록하게 한다. 실제 사고(0.27→0.28)에서 이 실패가 삼켜져 plist 만 새 경로이고 서비스도
/// 데몬도 없는 상태가 남았다.
pub fn ensure_daemon_with_policy(
    ctx: &CliContext,
    deps: &EnsureDeps,
    policy: RestartPolicy,
) -> Option<String> {
    let Some(running) = (deps.check_health)(&ctx.base_url) else {
        if policy == RestartPolicy::ExactVersion {
            return (deps.spawn)(ctx)
                .err()
                .map(|error| format!("데몬을 띄우지 못했다 — {error}"));
        }
        return None;
    };
    let stale = match policy {
        RestartPolicy::ExactVersion => running.version.as_deref() != Some(deps.version),
        // version 미보고(≤0.1.0)는 확실히 옛것. 못 읽는 문자열은 건드리지 않는다.
        RestartPolicy::OnlyIfOlder => match running.version.as_deref() {
            None => true,
            Some(v) => rocky_core::version::is_older(v, deps.version),
        },
    };
    if !stale {
        return None;
    }
    let old = running.version.as_deref().unwrap_or("?");
    let new = deps.version;
    if (deps.is_managed)() {
        let Err(error) = (deps.replace_managed)() else {
            return None;
        };
        if (deps.check_health)(&ctx.base_url).is_some() {
            return Some(format!(
                "v{old} → v{new} 교체 중 launchd 재등록 실패: {error}\n  구버전 데몬(v{old})이 그대로 돈다 — `rocky daemon install` 로 다시 등록하라."
            ));
        }
        // 옛 job 은 내려갔는데 새 job 이 안 올라왔다 — launchd 밖에서라도 띄운다. 그것마저
        // 실패하면 "띄웠다" 고 말하면 안 된다: 데몬이 없다는 사실과 사유를 그대로 싣는다.
        return Some(match (deps.spawn)(ctx) {
            Ok(()) => format!(
                "v{old} → v{new} 교체 중 launchd 재등록 실패: {error}\n  데몬은 launchd 밖에서 띄웠다 — 재부팅·크래시 뒤 살아나지 않는다. `rocky daemon install` 로 다시 등록하라."
            ),
            Err(spawn_error) => format!(
                "v{old} → v{new} 교체 중 launchd 재등록 실패: {error}\n  launchd 밖에서 띄우는 것도 실패했다 — {spawn_error}\n  지금 데몬이 없다. `rocky daemon install` (또는 `rocky daemon start`) 로 올려라."
            ),
        });
    }
    if (deps.stop)(ctx, running.pid) {
        return (deps.spawn)(ctx).err().map(|error| {
            format!("구버전 데몬(v{old})은 내렸는데 v{new} 를 띄우지 못했다 — {error}\n  `rocky daemon start` 로 올려라.")
        });
    }
    Some(format!(
        "구버전 데몬(v{old}{})을 내리지 못해 v{new} 로 재기동하지 않았다 — `rocky daemon stop && rocky daemon start`",
        running
            .pid
            .map(|p| format!(", pid {p}"))
            .unwrap_or_default()
    ))
}

/// env/설정의 watch 토글 — env `ROCKY_TODO_WATCH` 가 있으면 그 값이 이기고, 없으면
/// `todo.watch`(기본 on).
fn watch_enabled(watch_config: Option<bool>) -> bool {
    if let Ok(raw) = std::env::var("ROCKY_TODO_WATCH") {
        let value = raw.trim().to_lowercase();
        if !value.is_empty() {
            return !matches!(value.as_str(), "0" | "false" | "off" | "no");
        }
    }
    watch_config != Some(false)
}

/// UserPromptSubmit: 마지막 확인 이후 사람이 보드에서 바꾼 내용 + 이 세션 앞의
/// 핸드오프를 additionalContext 로 주입한다.
///
/// 훅에서 데몬을 새로 띄우지는 않는다 — 기동은 SessionStart/CLI/launchd 몫. 다만 **도는
/// 데몬이 이 설치본보다 오래됐으면 올린다**(`RestartPolicy::OnlyIfOlder`): `/reload-plugins` 로
/// 플러그인만 갈아 끼운 세션은 SessionStart 가 다시 돌지 않아, 이 자리가 새 버전이 처음
/// 데몬을 만나는 곳이다. 커서는 세션별이고 첫 프롬프트에서는 현재 위치만 기록한다(과거
/// 히스토리 덤프 방지).
pub fn hook_notify_todo(ctx: &CliContext, watch_config: Option<bool>) {
    // 업그레이드가 반쯤 실패했으면 그 사실을 이 턴의 컨텍스트에 싣는다 — 실패한 턴에만 뜬다
    // (그 뒤로는 데몬이 없거나 새 버전이라 `OnlyIfOlder` 가 다시 건드리지 않는다).
    let upgrade_warning = upgrade_daemon_if_older(ctx).map(|w| format!("⚠ rocky 데몬: {w}"));
    if !watch_enabled(watch_config) {
        emit_prompt_context(merge_context(&[upgrade_warning]));
        return;
    }
    let input = read_stdin_json();
    let Some(session_id) = input.get("session_id").and_then(|v| v.as_str()) else {
        emit_prompt_context(merge_context(&[upgrade_warning]));
        return;
    };

    let cursor_file = ctx.dir.join("hook-cursors.json");
    let cursor = read_cursor(&cursor_file, session_id);

    // claim 과 changes 조회는 서로 독립이라 순차로 기다리면 최악(연결은 되는데
    // 응답이 늦는 데몬)에 1.5s 타임아웃이 두 번 더해져 프롬프트 지연이 배가된다 —
    // TS 의 Promise.all 대응으로 claim 을 스레드에 띄워 둘을 겹친다. 커서 읽기/쓰기
    // 순서와 "첫 프롬프트엔 과거 히스토리를 주입하지 않는다"는 동작은 그대로다.
    let claim_thread = {
        let base_url = ctx.base_url.clone();
        let session_id = session_id.to_string();
        std::thread::spawn(move || claim_handoff(&base_url, &session_id, "prompt"))
    };

    let mut change_context: Option<String> = None;
    let mut pr_context: Option<String> = None;
    match cursor {
        None => {
            // 첫 프롬프트 — 현재 watermark 만 기록하고 과거 히스토리는 주입하지 않는다.
            if let Some(feed) = fetch_changes(&ctx.base_url, 0, 1) {
                write_cursor(&cursor_file, session_id, feed.last_id);
            }
        }
        Some(cursor) => {
            if let Some(feed) = fetch_changes(&ctx.base_url, cursor, 100) {
                if feed.last_id != cursor {
                    write_cursor(&cursor_file, session_id, feed.last_id);
                }
                // 데몬의 PR 감시 전이(actor rocky)는 사람 변경 필터에 걸리므로 먼저 따로 뽑는다 —
                // **이 세션의 보드 것만**(다른 레포 세션에 남의 PR 을 알리지 않는다).
                if feed
                    .entries
                    .iter()
                    .any(|e| e.history.action.starts_with("pr-"))
                {
                    let board = session_board_key(&ctx.base_url, &input);
                    pr_context =
                        build_pr_context(&pr_entries_for_board(&feed.entries, board.as_deref()));
                }
                change_context = build_notify_context(&filter_human_changes(feed.entries));
            }
        }
    }

    // 패닉한 스레드는 "요청 없음"과 같게 본다 — fail-open.
    let claimed = claim_thread.join().unwrap_or(None);
    let handoff_context = claimed.as_ref().map(build_handoff_prompt);

    emit_prompt_context(merge_context(&[
        upgrade_warning,
        change_context,
        pr_context,
        handoff_context,
    ]));
}

/// UserPromptSubmit 의 additionalContext 출력 — 실을 게 없으면 아무것도 내지 않는다.
fn emit_prompt_context(context: Option<String>) {
    let Some(context) = context else {
        return;
    };
    println!(
        "{}",
        json!({
            "hookSpecificOutput": {
                "hookEventName": "UserPromptSubmit",
                "additionalContext": context,
            }
        })
    );
}

/// Stop: 이 세션 앞으로 온 보드 작업 요청이 있으면 턴을 끝내지 못하게 막고
/// (`decision: "block"`) 그 자리에서 착수시킨다.
///
/// **서브에이전트에서는 빠진다** — 서브에이전트가 보드 요청을 가로채면 사용자가 보낸
/// 대상과 실제 처리 주체가 갈린다. 무한 루프는 구조적으로 없다: claim 된 건은
/// delivered 라 다시 나오지 않고, 큐가 비면 block 하지 않는다.
pub fn hook_handoff_stop(ctx: &CliContext) {
    let input = read_stdin_json();
    let Some(session_id) = input.get("session_id").and_then(|v| v.as_str()) else {
        return;
    };
    let is_subagent = input
        .get("agent_id")
        .and_then(|v| v.as_str())
        .is_some_and(|v| !v.is_empty())
        || input
            .get("agent_type")
            .and_then(|v| v.as_str())
            .is_some_and(|v| !v.is_empty());
    if is_subagent {
        return;
    }
    let Some(claimed) = claim_handoff(&ctx.base_url, session_id, "stop") else {
        return;
    };
    println!(
        "{}",
        json!({ "decision": "block", "reason": build_handoff_prompt(&claimed) })
    );
}

/// Stop: 트랜스크립트에서 이번 턴을 뽑아 `kind:"turn"` 한 줄을 워크로그에 append 한다.
/// TS 원본 `src/hooks/log-turn.ts`. 결정론적(LLM 0), 어떤 실패도 턴을 막지 않는다.
///
/// 저널 위치·키는 훅 입력의 `cwd`(세션 프로젝트) 기준 — `worklog_*` MCP 서버가 같은
/// cwd 로 뜨므로 같은 앵커에 쌓인다.
pub fn hook_log_turn() {
    let input = read_stdin_json();
    let cwd = input
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_dir().ok());
    let Some(cwd) = cwd else {
        return;
    };
    let config = load_worklog_config(&user_config_path(), &cwd);
    let env_toggle = std::env::var("ROCKY_WORKLOG_AUTO_CAPTURE").ok();
    if !should_capture(env_toggle.as_deref(), config.auto_capture) {
        return;
    }
    let Some(path) = input.get("transcript_path").and_then(|v| v.as_str()) else {
        return;
    };
    let Ok(transcript) = std::fs::read_to_string(path) else {
        return;
    };
    let Some(parts) = extract_turn(&transcript) else {
        return;
    };
    let content = build_turn_content(&parts, config.capture_max_chars.unwrap_or(800));
    let env_dir = std::env::var("ROCKY_WORKLOG_DIR").ok();
    let worklog = Worklog::from_env(env_dir.as_deref(), config.dir.as_deref(), Some(cwd));
    let _ = worklog.append(&WorklogAppendInput {
        content,
        kind: Some("turn".into()),
        tags: Some(vec!["turn".into()]),
        page_id: None,
    });
}
