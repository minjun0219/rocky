//! `rocky statusline --full` 의 git 세그먼트 재료 — `git status --porcelain=v2` 를 한 번 부른다. 파싱은
//! `rocky_core::statusline::git`, 실행(마감·그룹 kill)은 `crate::bounded`.

use std::time::Duration;

use rocky_core::statusline::git::GitStatus;

/// cc-usage 와 같은 마감.
pub const GIT_TIMEOUT: Duration = Duration::from_millis(500);

/// `dir` 의 브랜치·변경 수. git repo 가 아니거나 실패하거나 마감을 넘기면 `None`. untracked 는 스캔 비용 때문에 세지
/// 않는다(`--untracked-files=no`), 인덱스 잠금을 잡지 않는다(`--no-optional-locks`).
pub fn read(dir: &str, timeout: Duration) -> Option<GitStatus> {
    if dir.is_empty() {
        return None;
    }
    let argv = [
        "git",
        "-C",
        dir,
        "--no-optional-locks",
        "status",
        "--porcelain=v2",
        "--branch",
        "--untracked-files=no",
    ]
    .map(String::from);
    crate::bounded::run(&argv, timeout).map(|out| GitStatus::parse(&String::from_utf8_lossy(&out)))
}
