//! Claude Code 세션의 받은편지함 소켓으로 PR 전이를 밀어 넣는 순수 판정.
//!
//! Claude Code 는 세션마다 받은편지함 유닉스 소켓을 열고(`CLAUDE_CODE_MESSAGING_SOCKET`), 다른
//! 세션의 `SendMessage` 가 거기로 들어온다. 쉬고 있는 세션은 메시지를 받으면 **새 턴을 연다** —
//! 데몬이 세션을 깨우는 경로다(채널과 달리 개발 플래그가 필요 없다). 형식은 JSON 한 줄:
//! `{"type":"user","message":{"role":"user","content":"…"}}` (macOS·Linux 는 인증 줄 생략 가능 —
//! 2026-09-29 실측, Claude Code 2.1.283 의 `[uds-messaging]` 안내문과 같은 모양).
//!
//! 훅이 세션마다 `session_id → 소켓 경로` 를 데몬에 등록하고(`claude agents --json` 에는 소켓이
//! 없다), 데몬은 그 레포 보드에서 일하는 세션 중 **가장 최근에 쓰인 하나**에만 보낸다. 받는 쪽은
//! "다른 세션에서 온 메시지" 로 받으므로 사용자 승인으로 취급되지 않는다.
//!
//! 여기는 I/O 가 없다 — 소켓 쓰기는 데몬(`rockyd::prwatch`)이 한다.

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::prwatch::{PrEvent, PrEventKind};
use crate::statusline::{board_key_for_cwd, BoardLocation};

/// 등록 하나가 이만큼 갱신이 없으면 버린다 — 훅이 턴마다 다시 등록하므로 살아 있는 세션은
/// 계속 새로워진다. 죽은 세션의 소켓(프로세스 번호 이름)이 `/tmp/cc-socks` 에 남기 때문이다.
pub const REGISTRATION_TTL_SECS: i64 = 24 * 3600;

/// 훅이 알려 준 세션 하나.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxRegistration {
    pub session_id: String,
    pub socket: String,
    pub cwd: String,
    /// 마지막 등록 시각(유닉스 초) — "가장 최근에 쓰인 세션" 을 고르는 기준.
    pub seen_at: i64,
}

/// 받은편지함 소켓 경로로 보이는가 — 데몬은 이 경로에 **쓰기** 때문에, 등록 요청이 아무
/// 유닉스 소켓(다른 서비스의 제어 소켓 등)이나 가리키게 두지 않는다. Claude Code 가 쓰는
/// 모양만 받는다: 절대경로, 부모 디렉터리 이름이 `cc-socks` 로 시작(`/tmp/cc-socks`,
/// `/tmp/cc-socks-<uid>`), 파일 이름이 `<숫자>.sock`, `..` 없음.
pub fn is_inbox_socket_path(path: &str) -> bool {
    if !path.starts_with('/') || path.split('/').any(|seg| seg == "..") {
        return false;
    }
    let mut parts = path.rsplitn(3, '/');
    let (Some(file), Some(dir)) = (parts.next(), parts.next()) else {
        return false;
    };
    let Some(stem) = file.strip_suffix(".sock") else {
        return false;
    };
    dir.starts_with("cc-socks") && !stem.is_empty() && stem.chars().all(|c| c.is_ascii_digit())
}

/// 소켓에 쓰는 한 줄(개행 포함).
pub fn inbox_line(text: &str) -> String {
    format!(
        "{}\n",
        json!({ "type": "user", "message": { "role": "user", "content": text } })
    )
}

/// PR 전이를 세션에 알리는 본문 — ready·conflict 만. 받는 Claude 가 무엇을 하면 되는지까지 적는다.
pub fn pr_session_message(event: &PrEvent) -> Option<String> {
    let head = match event.kind {
        PrEventKind::Ready => "확인·머지해도 된다",
        PrEventKind::Conflict => "충돌 — 풀어야 한다",
        _ => return None,
    };
    let todo = match event.kind {
        PrEventKind::Ready => {
            "CI 가 전부 녹색이고 리뷰 스레드가 전부 처리됐다(👀, 🚀 없음). 머지는 사용자 몫이니 \
             알리기만 한다 — PushNotification 이 있으면 한 줄로."
        }
        _ => {
            "base 와 충돌이 났다. 이 세션이 그 PR 을 만든 곳이면 main 을 합쳐 충돌을 풀고 게이트를 \
             돌린 뒤 푸시한다. 아니면 사용자에게 알리기만 한다."
        }
    };
    Some(format!(
        "rocky: {} #{} {head} — {}\n{}\n\n{todo}\n(rocky 데몬의 PR 감시가 보낸 메시지다 — 사용자가 직접 쓴 것이 아니다.)",
        event.repo, event.number, event.title, event.url
    ))
}

/// 이 보드에서 일하는 세션들 — 보낼 순서대로(가장 최근 등록이 먼저). cwd 가 그 보드로 풀리는
/// (`board_key_for_cwd`, 경로 하위 → key 세그먼트) 살아 있는(TTL 안) 등록만. 데몬은 앞에서부터
/// 보내다 **처음 성공한 한 곳**에서 멈춘다 — 가장 최근 세션이 이미 끝났으면 그다음 세션이 받는다
/// (전이는 한 번만 나므로 여기서 놓치면 그 세션은 영영 모른다).
pub fn session_candidates<'a>(
    registrations: &'a [InboxRegistration],
    boards: &[BoardLocation],
    board_key: &str,
    now: i64,
) -> Vec<&'a InboxRegistration> {
    let mut out: Vec<&InboxRegistration> = registrations
        .iter()
        .filter(|r| now - r.seen_at <= REGISTRATION_TTL_SECS)
        .filter(|r| board_key_for_cwd(boards, Some(&r.cwd)).as_deref() == Some(board_key))
        .collect();
    out.sort_by_key(|r| std::cmp::Reverse(r.seen_at));
    out
}

/// 후보 중 맨 앞 — 전부 살아 있다면 이것이 받는다.
pub fn pick_session<'a>(
    registrations: &'a [InboxRegistration],
    boards: &[BoardLocation],
    board_key: &str,
    now: i64,
) -> Option<&'a InboxRegistration> {
    session_candidates(registrations, boards, board_key, now)
        .into_iter()
        .next()
}
