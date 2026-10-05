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
}

/// 이 원격 커밋을 돌아야 하나 — 기록이 없거나, 다른 커밋이거나, 같은 커밋인데 도는 중에 끊겼으면(데몬 재시작) 돈다.
pub fn should_run(last: Option<&VerifyRecord>, remote_sha: &str) -> bool {
    match last {
        None => true,
        Some(r) => r.sha != remote_sha || r.state == VerifyState::Running,
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
                "{short} {} — {}{}",
                now.failed_step.as_deref().unwrap_or("?"),
                now.reason.as_deref().unwrap_or("실패"),
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
