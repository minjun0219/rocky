//! `rocky statusline` 의 이름 묶음 — 구현은 cc-usage 를 라이브러리로 싣고(`cc-usage` 크레이트), 환경 변수·캐시 폴더·
//! 메시지 머리만 rocky 것으로 건다. `main` 첫머리에서 [`install`] 한 번.
//!
//! 이름은 cc-usage 를 싣기 전 rocky 복사본이 쓰던 그대로다 — 바꾸면 사람의 셸 설정(`ROCKY_STATUSLINE_SOURCE`)과 캐시
//! (`<cache>/rocky/statusline/`)를 잃는다.

use cc_usage::profile::Profile;

pub const ROCKY: Profile = Profile {
    name: "rocky",
    cache_dir: &["rocky", "statusline"],
    refresh_args: &["statusline", "refresh"],
    allow_command: "rocky statusline allow",
    extra_commands_hint: "docs/board.md \"statusline\"",
    source_env: "ROCKY_STATUSLINE_SOURCE",
    nested_env: "ROCKY_STATUSLINE_NESTED",
    now_env: "ROCKY_STATUSLINE_NOW",
    usage_url_env: "ROCKY_STATUSLINE_USAGE_URL",
};

/// 이 프로세스의 statusline 이름을 rocky 로 건다 — 처음 한 번만 먹는다.
pub fn install() {
    cc_usage::profile::set(ROCKY);
}
