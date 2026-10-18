//! 기본 브랜치 검증 — 원격 브랜치(보통 `main`)에 새 커밋이 들어오면 데몬이 전용 워크트리에서 게이트를 다시 돈다.
//! 여러 워크트리에서 세션들이 PR 을 연달아 머지하는 흐름에서 "머지된 결과가 여전히 초록인가" 를 사람 대신 본다.
//!
//! 여기는 순수 판정만 — 원격 커밋 읽기, 다시 돌릴지, 알릴지, 통과한 커밋을 배포할지. 설정 모양(`rocky.json` 의
//! `verify`)은 다른 블록처럼 `config.rs`(`load_verify_block`)에 있고, 프로세스·git·파일 배선은 `rockyd::verify`.
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
    /// 사람이 다시 돌려 달라고 해서 돈 실행 — `last.json` 에 같이 남아, 도중에 끊겨도 이력에 그대로 실린다.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rerun: bool,
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
    /// 데몬이 시작한 배포가 끝까지 갔다.
    Deployed,
    /// 데몬이 시작한 배포가 실패했다 — 띄우지 못했거나 끝난 뒤 `main/.finished` 가 그 커밋이 아니다.
    #[serde(rename = "deploy-failed")]
    DeployFailed,
}

/// 실행 이력 한 줄 — 대상 디렉터리의 `runs.jsonl` 에 덧붙인다. `last.json`·`finished.json` 은 마지막 하나뿐이라,
/// "언제 어떤 커밋이 통과·실패했나" 와 메모리에만 있던 `error` 를 나중에 보려고 남긴다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyRun {
    pub at: String,
    pub event: VerifyRunEvent,
    /// `error` 의 이유.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 끝난(또는 끊긴) 실행의 기록.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<VerifyRecord>,
    /// 끝난 배포의 기록(`deployed`·`deploy-failed`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deploy: Option<DeployRecord>,
}

impl VerifyRun {
    /// 끝난 실행 — 기록의 상태에서 종류를 정한다(도는 중이면 끊긴 것).
    pub fn of(at: String, record: VerifyRecord) -> Self {
        let event = match record.state {
            VerifyState::Passed => VerifyRunEvent::Passed,
            VerifyState::Failed => VerifyRunEvent::Failed,
            VerifyState::Running => VerifyRunEvent::Interrupted,
        };
        Self {
            at,
            event,
            error: None,
            record: Some(record),
            deploy: None,
        }
    }

    pub fn error(at: String, reason: String) -> Self {
        Self {
            at,
            event: VerifyRunEvent::Error,
            error: Some(reason),
            record: None,
            deploy: None,
        }
    }

    /// 끝난 배포 — 성공이면 `deployed`, 아니면 `deploy-failed`. 판정(`settle_deploy`)한 기록이나 띄우지 못한 기록을 남긴다.
    pub fn deploy(at: String, record: DeployRecord) -> Self {
        let event = if record.state == DeployState::Succeeded {
            VerifyRunEvent::Deployed
        } else {
            VerifyRunEvent::DeployFailed
        };
        Self {
            at,
            event,
            error: None,
            record: None,
            deploy: Some(record),
        }
    }
}

/// 기본 브랜치 검증 대상을 맡은 세션 — 끝난 실행마다 그 세션 받은편지함으로 결과 한 줄(`session_notice`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifySubscription {
    pub board: String,
    pub branch: String,
    pub session_id: String,
    pub created_at: String,
}

/// 구독한 세션에 보낼 것 — 배너와 달리 **끝난 실행마다**(통과도) 보낸다: 배포를 맡은 세션이 통과를 기다린다. 같은 커밋은
/// 한 번 — 직전 판정과 커밋이 같고 결과도 같으면(다시 돌렸는데 그대로) 보내지 않는다. `daemon_deploys` 는 이 대상을 데몬이
/// 배포하는가(`deploy_gate` 통과) — 그러면 통과 문구가 세션에게 배포하지 말라고 한다(같은 커밋을 둘이 올리지 않게).
/// (전달 기록 종류, 본문)
pub fn session_notice(
    prev: Option<&VerifyRecord>,
    now: &VerifyRecord,
    daemon_deploys: bool,
) -> Option<(&'static str, String)> {
    if now.state == VerifyState::Running {
        return None;
    }
    if prev.is_some_and(|p| p.sha == now.sha && p.state == now.state) {
        return None;
    }
    let short = &now.sha[..now.sha.len().min(7)];
    let subject = now.subject.as_deref().unwrap_or("");
    let (kind, head, next) = if now.state == VerifyState::Passed {
        (
            "verify-passed",
            format!("{short} 통과 · {subject}"),
            if daemon_deploys {
                "이 커밋은 데몬이 배포한다(verify.targets[].deploy) — 세션은 rocky update 를 돌리지 않는다. 결과는 `rocky verify`."
            } else {
                "배포를 맡은 세션이면 이 커밋으로 배포한다."
            }
            .to_string(),
        )
    } else {
        (
            "verify-failed",
            format!(
                "{short} 실패({}) — {} · {subject}",
                now.failed_step.as_deref().unwrap_or("?"),
                now.reason.as_deref().unwrap_or("실패"),
            ),
            format!("배포하지 않는다. 로그: {}", now.log),
        )
    };
    Some((
        kind,
        format!(
            "rocky: {} {} 검증 {head}\n\n{next} 다시 돌리려면 `rocky verify --rerun {}`.\n(rocky 데몬의 기본 브랜치 검증이 보낸 메시지다 — 사용자가 직접 쓴 것이 아니다.)",
            now.board, now.branch, now.board
        ),
    ))
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

// ── 검증 통과 커밋 자동 배포 ──────────────────────────────────────────────────────
// `verify.targets[].deploy` 를 켠 main 빌드 기기에서 데몬이 통과한 커밋으로 `rocky update --main <sha>` 를 띄운다.
// 판정은 바퀴마다 한다(통과한 순간에만 하면 데몬이 다시 뜨거나 통과가 겹칠 때 놓친다). 설계는
// `docs/design/specs/2026-10-18-verify-auto-deploy-design.md`.

/// 데몬이 띄운 배포 한 번의 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeployState {
    Running,
    Succeeded,
    Failed,
}

/// 데몬이 띄운 `rocky update --main <sha>` 의 기록 — 대상마다 마지막 하나를 `deploy.json` 으로. update 가 데몬을
/// 교체하므로 끝은 새 데몬이 보는 일이 많다: 그 pid 가 끝났으면 `settle_deploy` 로 판정한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeployRecord {
    pub sha: String,
    pub state: DeployState,
    /// update 프로세스. 띄우지 못했으면 없다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// 그 프로세스의 시작 시각(`ps -o lstart=`) — 재사용된 pid 를 실행 중으로 보지 않으려고(검증의 `running.pgid` 와 같다).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid_started: Option<String>,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    /// 실패 이유 한 줄.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// update 출력이 쌓인 로그 파일.
    pub log: String,
}

/// 배포하지 않는 이유.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeploySkip {
    /// 설정에서 켜지 않았다.
    Off,
    /// main 이 아닌 브랜치 — `update --main` 은 origin/main 위의 커밋만 받는다(`resolve_want`).
    NotMainBranch(String),
    /// 보드 `repo` 가 rocky 가 아니다(None 이면 repo 가 없다).
    NotRocky(Option<String>),
    /// 이 기기가 main 빌드 모드가 아니다 — 모드 전환은 마켓플레이스를 바꾸므로 사람이 한 번 한다.
    NotMainMode,
    /// 데몬이 띄운 배포가 아직 돈다.
    Running,
    /// 다른 main 빌드(세션·사람의 `rocky update --main`, 기기의 빌드 스크립트)가 `main-sync.lock` 을 쥐고 있다. 지금 띄우면
    /// 잠금에 걸려 그 커밋의 실패로 남는다 — 다음 바퀴에 다시 본다.
    Busy,
    /// 통과한 검증이 아직 없다.
    NotPassed,
    /// 통과한 커밋이 이미 끝까지 깔렸다(`main/.finished`).
    Installed,
    /// 이 커밋은 이미 자동 배포했다 — 같은 커밋은 한 번만. 그 뒤 사람이 다른 커밋으로 옮겼어도 되돌리지 않는다.
    AlreadyTried(DeployState),
    /// 깔린 빌드(이 커밋)가 통과한 커밋의 조상이 아니다 — 사람이 더 새 커밋을 올렸다. 되돌리지 않는다.
    NotForward(String),
    /// 판정할 재료가 없다(main 빌드 없음·조상 관계를 못 봄·커밋 해시 모양).
    Unknown(String),
}

impl DeploySkip {
    /// 데몬 로그 한 줄.
    pub fn message(&self) -> String {
        match self {
            Self::Off => "배포가 꺼져 있다(verify.targets[].deploy)".into(),
            Self::NotMainBranch(branch) => format!(
                "{branch} 브랜치는 배포하지 않는다 — rocky update --main 은 origin/main 위의 커밋만 받는다"
            ),
            Self::NotRocky(Some(repo)) => format!(
                "보드 repo({repo})가 {} 와 달라 배포하지 않는다",
                crate::main_build::REPO
            ),
            Self::NotRocky(None) => format!(
                "보드에 repo 가 없어 배포하지 않는다 — {} 보드만 배포한다",
                crate::main_build::REPO
            ),
            Self::NotMainMode => {
                "이 기기는 main 빌드 모드가 아니어서 배포하지 않는다 — 처음 한 번은 사람이 rocky update --main".into()
            }
            Self::Running => "지난 배포가 아직 돈다".into(),
            Self::Busy => {
                "다른 main 빌드가 돌고 있다(main-sync.lock) — 끝나면 다음 바퀴에 다시 본다".into()
            }
            Self::NotPassed => "통과한 검증이 아직 없다".into(),
            Self::Installed => "통과한 커밋이 이미 깔려 있다".into(),
            Self::AlreadyTried(state) => format!(
                "이 커밋은 이미 자동 배포했다({}) — 같은 커밋은 한 번만",
                match state {
                    DeployState::Succeeded => "성공",
                    DeployState::Failed => "실패",
                    DeployState::Running => "끝을 못 봄",
                }
            ),
            Self::NotForward(base) => format!(
                "깔린 빌드 {} 가 통과한 커밋의 조상이 아니어서 되돌리지 않는다",
                short(base)
            ),
            Self::Unknown(why) => why.clone(),
        }
    }

    /// `rocky verify` 가 대상 아래에 보일 이유 — 손을 봐야 풀릴 수 있는 것만. 꺼짐·정상(이미 깔림·통과 기다림)·실행 중·이미
    /// 한 커밋은 보이지 않는다(배포 기록 줄이 말한다). 잠금 기다림은 보인다 — 죽은 스크립트가 남긴 잠금이면 사람이 지워야 한다.
    pub fn note(&self) -> Option<String> {
        match self {
            Self::Off
            | Self::Running
            | Self::NotPassed
            | Self::Installed
            | Self::AlreadyTried(_) => None,
            _ => Some(self.message()),
        }
    }
}

/// 배포 판정의 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeployPlan {
    Skip(DeploySkip),
    /// 이 커밋으로 `rocky update --main <sha>` 를 띄운다.
    Start(String),
}

/// 배포 판정의 재료 — 전부 호출자(데몬)가 모은다.
#[derive(Debug, Clone, Default)]
pub struct DeployInput<'a> {
    /// 설정의 스위치(`verify.targets[].deploy`).
    pub enabled: bool,
    pub branch: &'a str,
    /// 대상 보드의 `repo`(owner/name).
    pub repo: Option<&'a str>,
    /// 이 기기가 main 빌드 모드인가(`current` → `main`).
    pub main_mode: bool,
    /// 데몬이 띄운 배포가 아직 도는가(pid 와 그 시작 시각으로 본다). main 빌드는 기기에 하나라 대상을 가리지 않는다.
    pub running: bool,
    /// `main-sync.lock` 을 살아 있는 프로세스가 쥐고 있나(pid 가 없는 잠금도 쥔 것으로 본다 — `update --main` 과 같다).
    pub busy: bool,
    /// 마지막으로 통과한 검증. 그 뒤에 끝난 실행이 실패여도 이 커밋은 배포 대상이다(설계 3번 — 배포가 잠깐 막힌 사이 다음
    /// 커밋이 빨강이 돼도 그 앞의 초록은 올린다).
    pub passed: Option<&'a VerifyRecord>,
    /// 마지막 배포(`deploy.json`) — 끝난 것으로 판정한 뒤의 기록.
    pub last_deploy: Option<&'a DeployRecord>,
    /// `main/.finished` — 지난 update 가 끝까지 간 커밋.
    pub finished_build: Option<&'a str>,
    /// `main/.sha` — 깔린 빌드의 커밋(뒤 단계에서 실패했으면 `.finished` 가 없다).
    pub built: Option<&'a str>,
    /// 깔린 빌드(`deploy_base`)가 통과한 커밋의 조상인가 — 호출자가 `git merge-base --is-ancestor` 로. 둘이 같거나 못 봤으면
    /// None.
    pub ancestor: Option<bool>,
}

/// 배포가 켜질 수 있는 대상·기기인가 — 기록과 빌드를 보기 전의 조건이다. 세션 알림 문구도 이걸로 정한다(데몬이 맡은
/// 대상이면 세션이 같은 커밋을 또 올리지 않게).
pub fn deploy_gate(
    enabled: bool,
    branch: &str,
    repo: Option<&str>,
    main_mode: bool,
) -> Result<(), DeploySkip> {
    if !enabled {
        return Err(DeploySkip::Off);
    }
    if branch != "main" {
        return Err(DeploySkip::NotMainBranch(branch.to_string()));
    }
    let repo = repo.map(str::trim).filter(|r| !r.is_empty());
    if !repo.is_some_and(|r| r.eq_ignore_ascii_case(crate::main_build::REPO)) {
        return Err(DeploySkip::NotRocky(repo.map(str::to_string)));
    }
    if !main_mode {
        return Err(DeploySkip::NotMainMode);
    }
    Ok(())
}

/// 깔린 빌드의 커밋 — 끝까지 간 것(`main/.finished`)이 먼저, 없으면 빌드만 된 것(`main/.sha`). 호출자가 git argv 에 넣으므로
/// 40자 16진만 받는다.
pub fn deploy_base<'a>(finished_build: Option<&'a str>, built: Option<&'a str>) -> Option<&'a str> {
    let clean = |s: Option<&'a str>| s.map(str::trim).filter(|s| is_full_sha(s));
    clean(finished_build).or_else(|| clean(built))
}

/// update·git 의 argv 에 넣을 커밋 — `git ls-remote` 가 준 40자 16진만.
pub fn is_full_sha(sha: &str) -> bool {
    sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit())
}

/// 앞 7자. `main/.sha` 같은 파일 내용에도 쓰므로 글자 경계가 아니면 통째로(자르다 패닉하지 않게).
fn short(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

/// 이 바퀴에 배포를 띄울까. 앞으로만 간다(깔린 빌드가 통과한 커밋의 조상일 때만), 같은 커밋은 한 번만.
pub fn deploy_plan(input: &DeployInput) -> DeployPlan {
    use DeploySkip::*;
    let skip = DeployPlan::Skip;
    if let Err(why) = deploy_gate(input.enabled, input.branch, input.repo, input.main_mode) {
        return skip(why);
    }
    if input.running {
        return skip(Running);
    }
    let Some(sha) = input
        .passed
        .filter(|r| r.state == VerifyState::Passed)
        .map(|r| r.sha.as_str())
    else {
        return skip(NotPassed);
    };
    if !is_full_sha(sha) {
        return skip(Unknown(format!("커밋 해시가 40자 16진이 아니다: {sha}")));
    }
    if input.finished_build.map(str::trim) == Some(sha) {
        return skip(Installed);
    }
    if let Some(last) = input.last_deploy.filter(|d| d.sha == sha) {
        return skip(AlreadyTried(last.state));
    }
    let Some(base) = deploy_base(input.finished_build, input.built) else {
        return skip(Unknown(
            "깔린 main 빌드의 커밋을 읽지 못했다(main/.sha)".into(),
        ));
    };
    // base == sha 는 빌드는 있고 뒤 단계(데몬 재시작·플러그인)가 남은 것 — update 가 빌드 없이 마저 한다.
    if base != sha {
        match input.ancestor {
            Some(true) => {}
            Some(false) => return skip(NotForward(base.to_string())),
            None => {
                return skip(Unknown(format!(
                    "깔린 빌드 {} 가 통과한 커밋의 조상인지 확인하지 못했다",
                    short(base)
                )))
            }
        }
    }
    if input.busy {
        return skip(Busy);
    }
    DeployPlan::Start(sha.to_string())
}

/// 끝난 배포를 판정한다 — update 는 끝까지 갔을 때만 `main/.finished` 에 그 커밋을 쓴다. 종료 코드는 그 update 를 띄운
/// 데몬만 안다(교체된 새 데몬은 모른다). 도는 중(`running`)인 기록만 판정한다 — 이미 끝난 기록이면 None(배너·이력을 두 번
/// 남기지 않게).
pub fn settle_deploy(
    record: &DeployRecord,
    finished_build: Option<&str>,
    exit_code: Option<i32>,
    at: String,
) -> Option<DeployRecord> {
    if record.state != DeployState::Running {
        return None;
    }
    let mut settled = record.clone();
    settled.finished_at = Some(at);
    if finished_build.map(str::trim) == Some(record.sha.as_str()) {
        settled.state = DeployState::Succeeded;
        settled.reason = None;
    } else {
        settled.state = DeployState::Failed;
        settled.reason = Some(match (record.pid, exit_code) {
            (None, _) => "update 를 띄운 기록이 없다".into(),
            (_, Some(code)) if code != 0 => format!("update 종료 코드 {code}"),
            _ => "update 가 끝났는데 main/.finished 가 이 커밋이 아니다".into(),
        });
    }
    Some(settled)
}

/// 배포 결과를 사람에게 — 실패만 배너로 알린다(성공은 조용하다). (제목, 본문)
pub fn deploy_notification(board: &str, record: &DeployRecord) -> Option<(String, String)> {
    (record.state == DeployState::Failed).then(|| {
        let sha = short(&record.sha);
        (
            format!("rocky: {board} 배포 실패"),
            format!(
                "{sha} {} — 다시 하려면 rocky update --main {sha}",
                record.reason.as_deref().unwrap_or("실패")
            ),
        )
    })
}
