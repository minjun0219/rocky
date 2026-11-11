//! 레포 목록 — PR 감시와 기본 브랜치 검증이 보는 레포(`repo` · `path` · PR 작성자 필터 · reviewFix). 순수 판정만.
//!
//! 이 목록은 보드 행이 겸해 왔다. 보드를 rocky 에서 걷어내므로(`docs/design/specs/2026-11-11-board-and-rc-removal-design.md`)
//! 정본을 `rocky.json` 의 `repos` 블록으로 옮긴다. 보드가 남아 있는 동안에는 두 곳을 합친다 — 같은 key 면 설정이 이기고,
//! 설정에 없는 보드는 그대로 실린다. 보드가 빠지면 `effective` 의 보드 몫과 `board_id` 가 같이 빠진다.

use std::path::Path;

use crate::types::Board;

/// 레포 하나. key 는 보드 key 와 같은 이름 공간이다 — 그래서 `verify.targets[].board` 가 고치지 않고 이어진다.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoEntry {
    pub key: String,
    /// `owner/name`.
    pub repo: Option<String>,
    /// 메인 체크아웃의 절대경로(`~` 는 풀어 둔다).
    pub path: Option<String>,
    /// PR 감시가 **알릴** 작성자 — `@me` 또는 login. 비면 전부.
    pub pr_authors: Vec<String>,
    /// 리뷰가 붙으면 그 PR 을 구독한 세션에 review-fix 를 시킨다.
    pub review_fix: bool,
    /// 같은 key 의 보드 id — 보드에 묶인 필터(세션 훅 주입)를 찾는 데만 쓴다. 보드를 걷어내면 빠진다.
    pub board_id: Option<String>,
}

/// `rocky.json` 의 `repos` 블록(`{ "<key>": { repo, path, prAuthors, reviewFix } }`)을 읽는다. 파일 없음 · 파싱 실패 ·
/// 블록 없음은 빈 목록(fail-open). 모양이 틀린 칸은 버리고 나머지는 살린다 — key 가 비면 그 항목만 버린다.
/// 데몬은 전역 하나라 사용자 설정에서만 읽는다(`rc` 블록과 같다).
pub fn load_repos_block(config_path: &Path) -> Vec<RepoEntry> {
    let Ok(raw) = std::fs::read_to_string(config_path) else {
        return Vec::new();
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Vec::new();
    };
    parsed
        .get("repos")
        .and_then(|v| v.as_object())
        .map(parse_repos_block)
        .unwrap_or_default()
}

/// 블록 객체를 key 순서대로 읽는다.
pub fn parse_repos_block(block: &serde_json::Map<String, serde_json::Value>) -> Vec<RepoEntry> {
    let mut out: Vec<RepoEntry> = block
        .iter()
        .filter_map(|(key, value)| {
            let key = key.trim();
            let obj = value.as_object()?;
            (!key.is_empty()).then(|| RepoEntry {
                key: key.to_string(),
                repo: obj
                    .get("repo")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|r| crate::github::is_repo_slug(r))
                    .map(str::to_string),
                path: obj
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .map(|p| crate::config::expand_tilde(p).to_string_lossy().to_string()),
                pr_authors: obj
                    .get("prAuthors")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|a| a.as_str())
                            .map(str::trim)
                            .filter(|a| crate::prwatch::is_pr_author(a))
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default(),
                review_fix: obj
                    .get("reviewFix")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                board_id: None,
            })
        })
        .collect();
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out
}

/// 보드 행을 레포 항목으로 — 옮겨 가는 동안만 쓴다.
pub fn from_board(board: &Board) -> RepoEntry {
    RepoEntry {
        key: board.key.clone(),
        repo: board.repo.clone(),
        path: board.path.clone(),
        pr_authors: board.pr_authors.clone(),
        review_fix: board.review_fix,
        board_id: Some(board.id.clone()),
    }
}

/// 지금 쓰는 레포 목록 — 설정(`repos`)과 보관되지 않은 보드를 합친다. 같은 key 면 설정 항목이 통째로 이기고(보드의
/// `board_id` 만 붙는다), 설정에 없는 보드는 보드 값 그대로 실린다. 설정에만 있는 항목도 실린다. 순서는 설정 다음 보드.
pub fn effective(config: &[RepoEntry], boards: &[Board]) -> Vec<RepoEntry> {
    let mut out: Vec<RepoEntry> = config
        .iter()
        .map(|entry| RepoEntry {
            board_id: boards
                .iter()
                .find(|b| b.key == entry.key)
                .map(|b| b.id.clone()),
            ..entry.clone()
        })
        .collect();
    out.extend(
        boards
            .iter()
            .filter(|b| b.archived_at.is_none() && !config.iter().any(|c| c.key == b.key))
            .map(from_board),
    );
    out
}

/// 설정에 없는 보드 중 레포가 붙은 것 — `rocky doctor` 가 "`repos` 로 옮길 것" 으로 알린다.
pub fn boards_missing_from_config<'a>(config: &[RepoEntry], boards: &'a [Board]) -> Vec<&'a Board> {
    boards
        .iter()
        .filter(|b| {
            b.archived_at.is_none()
                && (b.repo.is_some() || b.path.is_some())
                && !config.iter().any(|c| c.key == b.key)
        })
        .collect()
}

/// 레포 이름(대소문자 무시)으로 고른 항목들.
pub fn for_repo<'a>(
    repos: &'a [RepoEntry],
    repo: &str,
) -> impl Iterator<Item = &'a RepoEntry> + 'a {
    let repo = repo.to_string();
    repos.iter().filter(move |r| {
        r.repo
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(&repo))
    })
}
