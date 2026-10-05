//! 기본 브랜치 검증 — 원격 브랜치(보통 `main`)에 새 커밋이 들어오면 데몬이 전용 워크트리에서 게이트를 다시 돈다.
//! 여러 워크트리에서 세션들이 PR 을 연달아 머지하는 흐름에서 "머지된 결과가 여전히 초록인가" 를 사람 대신 본다.
//!
//! 여기는 순수 판정만 — 원격 커밋 읽기, 다시 돌릴지, 알릴지. 설정 모양(`rocky.json` 의 `verify`)은 다른 블록처럼
//! `config.rs`(`load_verify_block`)에 있고, 프로세스·git·파일 배선은 `rockyd::verify`.
//!
//! **명령은 설정 파일에만 있다**(수집함 어댑터와 같은 원칙) — 화면이나 REST 가 실행할 명령을 바꾸지 않는다.

use serde::{Deserialize, Serialize};

pub use crate::config::{load_verify_block, VerifyConfig, VerifyTarget};

/// 단계 하나의 기본 상한 — `cargo test --workspace` 가 수 분 걸린다.
pub const DEFAULT_STEP_TIMEOUT_MS: u64 = 30 * 60 * 1000;
/// 원격을 보는 기본 주기.
pub const DEFAULT_INTERVAL_SECONDS: u64 = 60;
/// 한 커밋을 몇 번까지 돌아 보나 — 실패하면 그 자리에서 한 번 더 돌고, 두 번 연속 실패일 때만 빨강·배너.
/// 머신이 바빠 시간 초과로 떨어지는 거짓 실패를 줄인다(진짜 실패는 그만큼 늦게 알린다).
pub const MAX_ATTEMPTS: u32 = 2;

/// git 인자로 넘겨도 안전한 브랜치 이름 — 옵션처럼 보이거나 공백·`..` 가 든 값은 받지 않는다.
pub fn is_branch_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.contains("..")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
}

/// 보드 key·브랜치를 디렉터리 이름으로 — 읽기 쉬운 부분(영숫자·`-`·`_` 만) 뒤에 원래 이름의 SHA-256 앞 8자를 붙인다.
/// 치환만 하면 `release/a` 와 `release_a` 가 같은 디렉터리가 되어 기록·트리를 나눠 쓴다.
pub fn dir_name(name: &str) -> String {
    let readable: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let digest = ring::digest::digest(&ring::digest::SHA256, name.as_bytes());
    let hash: String = digest.as_ref()[..4]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("{readable}-{hash}")
}

/// `git ls-remote origin refs/heads/<branch>` 출력에서 그 브랜치의 커밋. 없으면 `None`.
pub fn parse_ls_remote(output: &str, branch: &str) -> Option<String> {
    let want = format!("refs/heads/{branch}");
    output.lines().find_map(|line| {
        let (sha, name) = line.split_once('\t')?;
        let sha = sha.trim();
        (name.trim() == want && sha.len() >= 7 && sha.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| sha.to_string())
    })
}

/// 한 번의 검증 결과.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VerifyState {
    Running,
    Passed,
    Failed,
}

/// 검증 기록 — 대상마다 마지막 하나를 `last.json` 으로 남긴다(데몬이 다시 떠도 같은 커밋을 또 돌지 않게).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyRecord {
    pub board: String,
    pub branch: String,
    pub sha: String,
    /// 커밋 제목 한 줄.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub state: VerifyState,
    /// 실패한 단계 이름(또는 준비 단계 `fetch` · `worktree`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed_step: Option<String>,
    /// 실패 이유 한 줄(종료 코드·시간 초과).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    /// 단계 출력이 쌓인 로그 파일.
    pub log: String,
    /// 몇 번째 시도인가(1 부터). 실패한 첫 시도는 `runs.jsonl` 에만 남고, 두 번째 결과가 기록이 된다.
    #[serde(default = "first_attempt", skip_serializing_if = "is_first_attempt")]
    pub attempt: u32,
}

fn first_attempt() -> u32 {
    1
}

fn is_first_attempt(n: &u32) -> bool {
    *n <= 1
}

/// 이 원격 커밋을 돌아야 하나 — 기록이 없거나, 다른 커밋이거나, 같은 커밋인데 도는 중에 끊겼으면(데몬 재시작) 돈다.
/// 같은 커밋의 실패는 다시 돌지 않는다 — 환경 탓 거짓 실패는 사람이 다시 돌려 달라고 할 때(`rerun`)만.
pub fn should_run(last: Option<&VerifyRecord>, remote_sha: &str, rerun: bool) -> bool {
    match last {
        None => true,
        Some(r) => rerun || r.sha != remote_sha || r.state == VerifyState::Running,
    }
}

/// `runs.jsonl` 한 줄의 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VerifyRunEvent {
    Passed,
    Failed,
    /// 도는 중에 데몬이 내려가 끝을 못 본 실행 — 다음 실행이 시작될 때 남긴다.
    Interrupted,
    /// 검증을 하지 못했다(원격을 못 읽음·준비 실패·보드 path 없음). 같은 이유가 이어지면 한 번만.
    Error,
}

/// 실행 이력 한 줄 — 대상 디렉터리의 `runs.jsonl` 에 덧붙인다. `last.json`·`finished.json` 은 마지막 하나뿐이라,
/// "언제 어떤 커밋이 통과·실패했나" 와 메모리에만 있던 `error` 를 나중에 보려고 남긴다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyRun {
    pub at: String,
    pub event: VerifyRunEvent,
    /// 사람이 다시 돌려 달라고 해서 돈 실행.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rerun: bool,
    /// `error` 의 이유.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 끝난(또는 끊긴) 실행의 기록.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<VerifyRecord>,
}

impl VerifyRun {
    /// 끝난 실행 — 기록의 상태에서 종류를 정한다(도는 중이면 끊긴 것).
    pub fn of(at: String, record: VerifyRecord, rerun: bool) -> Self {
        let event = match record.state {
            VerifyState::Passed => VerifyRunEvent::Passed,
            VerifyState::Failed => VerifyRunEvent::Failed,
            VerifyState::Running => VerifyRunEvent::Interrupted,
        };
        Self {
            at,
            event,
            rerun,
            error: None,
            record: Some(record),
        }
    }

    pub fn error(at: String, reason: String) -> Self {
        Self {
            at,
            event: VerifyRunEvent::Error,
            rerun: false,
            error: Some(reason),
            record: None,
        }
    }
}

/// 사람에게 알릴 것 — 실패는 늘, 통과는 직전이 실패였을 때만(복구). 통과가 이어지면 조용하다. (제목, 본문)
pub fn notification(prev: Option<&VerifyRecord>, now: &VerifyRecord) -> Option<(String, String)> {
    let short = &now.sha[..now.sha.len().min(7)];
    let subject = now.subject.as_deref().unwrap_or("");
    match now.state {
        VerifyState::Running => None,
        VerifyState::Failed => Some((
            format!("rocky: {} {} 검증 실패", now.board, now.branch),
            format!(
                "{short} {} — {}{}{}",
                now.failed_step.as_deref().unwrap_or("?"),
                now.reason.as_deref().unwrap_or("실패"),
                if now.attempt > 1 {
                    format!(" (다시 돌려도 실패 — {}번 연속)", now.attempt)
                } else {
                    String::new()
                },
                if subject.is_empty() {
                    String::new()
                } else {
                    format!(" · {subject}")
                }
            ),
        )),
        VerifyState::Passed => prev.filter(|p| p.state == VerifyState::Failed).map(|_| {
            (
                format!("rocky: {} {} 다시 초록", now.board, now.branch),
                format!("{short} 게이트 전부 통과 · {subject}"),
            )
        }),
    }
}
