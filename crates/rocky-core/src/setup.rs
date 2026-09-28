//! 설치·설정 점검 — "새 기기에서 뭐가 빠졌나" 를 한 번에. 순수 판정만.
//!
//! `rocky config show` 가 재료(설정 파일·데몬 health·launchd·Claude Code statusline·보드)를
//! 모아 넘기면 여기서 체크 목록과 다음 할 일을 만든다. `/rocky:config` 커맨드는 이 결과를
//! 보고 빠진 항목을 하나씩 채운다. 파일을 쓰거나 프로세스를 띄우는 일은 여기 없다.

use serde::{Deserialize, Serialize};

use crate::config::{ExposeValue, TodoConfig};
use crate::statusline::BoardLocation;

/// 기본 `rocky.json` — `rocky config init` 이 파일이 없을 때 쓴다. 값은 전부 기본값과
/// 같아서 있어도 동작이 바뀌지 않는다; `$schema` 로 편집기 자동완성이 붙고 어디를
/// 고치면 되는지가 보이는 게 목적이다.
pub fn default_config_json() -> String {
    let value = serde_json::json!({
        "$schema": "https://raw.githubusercontent.com/minjun0219/rocky/main/rocky.schema.json",
        "todo": {
            "expose": "off",
            "sessionSummary": true
        }
    });
    let mut text = serde_json::to_string_pretty(&value).unwrap_or_default();
    text.push('\n');
    text
}

/// Claude Code statusline 스크립트 끝에 붙일 조각 — `docs/board.md` "statusline 에 얹기" 와
/// 같은 내용. 포트를 바꿔 썼으면 여기도 따라간다.
pub fn statusline_snippet(port: u16) -> String {
    format!(
        "cwd=$(echo \"$input\" | jq -r '.workspace.current_dir // empty')\n\
         sid=$(echo \"$input\" | jq -r '.session_id // empty')\n\
         rt=$(curl -sf --max-time 0.3 \"http://127.0.0.1:{port}/api/statusline?cwd=$cwd&session=$sid\")\n\
         [ -n \"$rt\" ] && printf '%s\\n' \"$rt\""
    )
}

/// `settings.json` 의 `statusLine.command` 가 rocky 세그먼트를 부르는가. 명령이 스크립트
/// 경로 하나면(보통 그렇다) 그 파일 내용을 `script_body` 로 같이 넘긴다.
pub fn statusline_wired(command: Option<&str>, script_body: Option<&str>) -> bool {
    let needle = "/api/statusline";
    command.is_some_and(|c| c.contains(needle)) || script_body.is_some_and(|b| b.contains(needle))
}

/// 설정 파일 상태.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFileState {
    pub path: String,
    pub exists: bool,
    /// 있는데 JSON 이 깨졌으면 false — 로더는 fail-open 이라 조용히 기본값으로 가므로
    /// 여기서라도 알려야 한다.
    pub valid_json: bool,
}

/// 데몬 health 의 요약.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonState {
    pub version: Option<String>,
    pub pid: Option<u32>,
}

/// 점검 재료 — 전부 호출자가 모은다.
#[derive(Debug, Clone)]
pub struct SetupInput {
    pub config: ConfigFileState,
    pub todo: TodoConfig,
    /// env 까지 반영된 실제 포트.
    pub port: u16,
    pub cli_version: String,
    /// 데이터 홈(`$XDG_DATA_HOME` > `~/.local/share`)의 `rocky/current` 가 가리키는 곳. 레포에서 직접
    /// 빌드해 쓰면 None.
    pub install_current: Option<String>,
    pub daemon: Option<DaemonState>,
    pub launchd_registered: bool,
    /// `~/.claude/settings.json` 의 `statusLine.command`.
    pub statusline_command: Option<String>,
    /// 그 command 가 가리키는 스크립트 본문(읽을 수 있으면).
    pub statusline_script: Option<String>,
    pub cwd: Option<String>,
    /// cwd 에서 유추한 보드 key(레포 이름).
    pub repo_key: Option<String>,
    pub boards: Vec<BoardLocation>,
    /// PATH 에서 찾은 `rocky` 실행 파일. 없으면 터미널에서 `rocky` 가 안 불린다.
    pub cli_on_path: Option<String>,
    /// `~/.local/bin/rocky` 링크가 가리키는 곳(있으면).
    pub cli_link: Option<String>,
    /// `~/.local/bin` 이 PATH 에 있는가.
    pub local_bin_on_path: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckKind {
    /// 없으면 기능이 빠진다.
    Required,
    /// 있으면 좋다 — 없어도 돈다.
    Optional,
    /// 현재 값 표시.
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub id: String,
    pub kind: CheckKind,
    pub ok: bool,
    pub detail: String,
    /// 고치는 명령 또는 안내. ok 면 보통 None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupReport {
    pub checks: Vec<Check>,
    /// ok 가 아닌 Required → Optional 순의 fix 목록.
    pub next_steps: Vec<String>,
}

impl SetupReport {
    pub fn check(&self, id: &str) -> Option<&Check> {
        self.checks.iter().find(|c| c.id == id)
    }
}

fn check(
    id: &str,
    kind: CheckKind,
    ok: bool,
    detail: impl Into<String>,
    fix: Option<String>,
) -> Check {
    Check {
        id: id.into(),
        kind,
        ok,
        detail: detail.into(),
        fix,
    }
}

/// 재료 → 체크 목록. 순서가 곧 표시 순서다.
pub fn build_report(input: &SetupInput) -> SetupReport {
    let mut checks = Vec::new();

    // 설정 파일 — 없어도 돌지만, 깨진 건 조용히 무시되므로 Required 로 알린다.
    checks.push(match (input.config.exists, input.config.valid_json) {
        (false, _) => check(
            "config",
            CheckKind::Optional,
            false,
            format!("{} 없음 — 기본값으로 동작 중", input.config.path),
            Some("rocky config init".into()),
        ),
        (true, false) => check(
            "config",
            CheckKind::Required,
            false,
            format!(
                "{} 이 JSON 으로 읽히지 않는다 — 전부 기본값으로 무시되고 있다",
                input.config.path
            ),
            Some(format!("{} 의 JSON 문법을 고친다", input.config.path)),
        ),
        (true, true) => check(
            "config",
            CheckKind::Optional,
            true,
            input.config.path.clone(),
            None,
        ),
    });

    // 설치본 — 부트스트랩이 받아 둔 버전 디렉터리.
    checks.push(match &input.install_current {
        Some(target) => check(
            "install",
            CheckKind::Info,
            true,
            format!("설치본 current → {target}"),
            None,
        ),
        None => check(
            "install",
            CheckKind::Info,
            true,
            "설치본 current 링크 없음 — 레포 빌드나 다른 경로의 바이너리를 쓰는 중",
            None,
        ),
    });

    // 데몬.
    checks.push(match &input.daemon {
        Some(d) => {
            let version = d.version.clone().unwrap_or_else(|| "?".into());
            let same = version == input.cli_version;
            check(
                "daemon",
                CheckKind::Required,
                same,
                format!(
                    "실행 중 — v{version} (pid {}){}",
                    d.pid.map(|p| p.to_string()).unwrap_or_else(|| "?".into()),
                    if same {
                        String::new()
                    } else {
                        format!(
                            ", CLI 는 v{} — 다음 세션 시작 때 재기동된다",
                            input.cli_version
                        )
                    }
                ),
                (!same).then(|| "rocky daemon stop && rocky daemon start".to_string()),
            )
        }
        None => check(
            "daemon",
            CheckKind::Required,
            false,
            format!("127.0.0.1:{} 에 데몬 없음", input.port),
            Some("rocky daemon start".into()),
        ),
    });

    checks.push(check(
        "launchd",
        CheckKind::Optional,
        input.launchd_registered,
        if input.launchd_registered {
            "상주 등록됨 (KeepAlive)"
        } else {
            "상주 등록 안 됨 — 세션이 열릴 때만 뜬다"
        },
        (!input.launchd_registered).then(|| "rocky daemon install".to_string()),
    ));

    // 세션 요약 — 기본 on.
    let summary_on = input.todo.session_summary.unwrap_or(true);
    checks.push(check(
        "session-summary",
        CheckKind::Optional,
        summary_on,
        if summary_on {
            "세션 시작 때 보드 요약을 넣는다"
        } else {
            "꺼짐 (todo.sessionSummary: false)"
        },
        (!summary_on).then(|| format!("{} 의 todo.sessionSummary 를 true 로", input.config.path)),
    ));

    // 노출 — 정보만. 기본 off 가 안전한 쪽이라 ok 로 둔다.
    let expose = match &input.todo.expose {
        None | Some(ExposeValue::Off) => "off (이 머신만)".to_string(),
        Some(ExposeValue::Channels(channels)) => channels
            .iter()
            .map(|c| c.as_str().to_string())
            .collect::<Vec<_>>()
            .join(", "),
    };
    checks.push(check("expose", CheckKind::Info, true, expose, None));

    // 수집함.
    let names: Vec<&str> = input.todo.inbox.iter().map(|s| s.name.as_str()).collect();
    checks.push(check(
        "inbox",
        CheckKind::Info,
        true,
        if names.is_empty() {
            "어댑터 없음 — 외부 투두를 읽지 않는다".to_string()
        } else {
            names.join(", ")
        },
        None,
    ));

    // statusline.
    let wired = statusline_wired(
        input.statusline_command.as_deref(),
        input.statusline_script.as_deref(),
    );
    checks.push(check(
        "statusline",
        CheckKind::Optional,
        wired,
        match (&input.statusline_command, wired) {
            (None, _) => "Claude Code statusLine 미설정".to_string(),
            (Some(cmd), true) => format!("연결됨 — {cmd}"),
            (Some(cmd), false) => format!("statusLine 은 있으나 rocky 세그먼트 없음 — {cmd}"),
        },
        (!wired).then(|| {
            format!(
                "statusline 스크립트 끝에 아래를 붙인다:\n{}",
                statusline_snippet(input.port)
            )
        }),
    ));

    // 터미널에서 `rocky` 가 불리는가 — 링크(SessionStart 가 건다)와 PATH 둘 다 있어야 한다.
    checks.push(match (&input.cli_on_path, &input.cli_link, input.local_bin_on_path) {
        (Some(path), _, _) => check("cli", CheckKind::Optional, true, format!("`rocky` → {path}"), None),
        (None, Some(_), false) => check(
            "cli",
            CheckKind::Optional,
            false,
            "~/.local/bin/rocky 링크는 있는데 ~/.local/bin 이 PATH 에 없다",
            Some("셸 rc 에: export PATH=\"$HOME/.local/bin:$PATH\"".into()),
        ),
        (None, None, _) => check(
            "cli",
            CheckKind::Optional,
            false,
            "터미널에서 `rocky` 가 안 불린다 — ~/.local/bin/rocky 링크 없음(다음 SessionStart 가 건다)",
            Some("rocky config link".into()),
        ),
        (None, Some(target), true) => check(
            "cli",
            CheckKind::Optional,
            false,
            format!("~/.local/bin/rocky → {target} 인데 PATH 에서 안 잡힌다 — 링크가 깨졌을 수 있다"),
            Some("rocky config link".into()),
        ),
    });

    // 보드 ↔ 레포.
    checks.push(board_check(input));

    let mut next_steps: Vec<String> = Vec::new();
    for kind in [CheckKind::Required, CheckKind::Optional] {
        for c in checks.iter().filter(|c| c.kind == kind && !c.ok) {
            if let Some(fix) = &c.fix {
                next_steps.push(fix.clone());
            }
        }
    }
    SetupReport { checks, next_steps }
}

fn board_check(input: &SetupInput) -> Check {
    let Some(repo_key) = input.repo_key.as_deref() else {
        return check(
            "board",
            CheckKind::Info,
            true,
            "cwd 가 레포가 아니라 보드 판정 없음",
            None,
        );
    };
    let Some(board) = input.boards.iter().find(|b| b.key == repo_key) else {
        return check(
            "board",
            CheckKind::Optional,
            false,
            format!("보드 `{repo_key}` 없음 — 첫 todo 를 만들면 생긴다"),
            Some(format!("rocky add \"첫 항목\" --board {repo_key}")),
        );
    };
    match &board.path {
        None => check(
            "board",
            CheckKind::Optional,
            false,
            format!("보드 `{repo_key}` 에 path 없음 — 새 세션 띄우기·statusline 보드 판정이 경로를 못 쓴다"),
            Some("rocky board path".into()),
        ),
        Some(path) => {
            // key 세그먼트 매칭은 어차피 되므로 여기서 보는 건 path 가 정말 이 레포인가 —
            // spawn 이 워크트리 cwd 로 쓰는 값이라 어긋나면 엉뚱한 레포에 세션이 뜬다.
            let covers = input
                .cwd
                .as_deref()
                .is_some_and(|cwd| path_covers(path, cwd));
            check(
                "board",
                CheckKind::Optional,
                covers,
                if covers {
                    format!("보드 `{repo_key}` ↔ {path}")
                } else {
                    format!("보드 `{repo_key}` 의 path({path})가 cwd 를 덮지 않는다")
                },
                (!covers).then(|| "rocky board path".to_string()),
            )
        }
    }
}

/// `cwd` 가 `path` 자신이거나 그 아래인가. 끝 `/` 는 무시.
fn path_covers(path: &str, cwd: &str) -> bool {
    let base = path.trim_end_matches('/');
    let base = if base.is_empty() { "/" } else { base };
    let cwd = cwd.trim_end_matches('/');
    cwd == base || cwd.starts_with(&format!("{}/", base.trim_end_matches('/')))
}

/// 사람이 읽는 표.
pub fn render_report(report: &SetupReport) -> String {
    let mut lines: Vec<String> = Vec::new();
    for c in &report.checks {
        let mark = match (c.kind, c.ok) {
            (_, true) => "✓",
            (CheckKind::Required, false) => "✗",
            (CheckKind::Optional, false) => "·",
            (CheckKind::Info, false) => "·",
        };
        lines.push(format!("{mark} {:<16} {}", c.id, c.detail));
    }
    if !report.next_steps.is_empty() {
        lines.push(String::new());
        lines.push("다음 할 일:".into());
        for (i, step) in report.next_steps.iter().enumerate() {
            let mut it = step.lines();
            lines.push(format!("  {}. {}", i + 1, it.next().unwrap_or("")));
            for rest in it {
                lines.push(format!("     {rest}"));
            }
        }
    }
    lines.join("\n")
}
