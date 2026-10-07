//! actor 분류 — "이 변경을 사람이 했나 에이전트가 했나". TS 원본 `src/actors.ts`.
//!
//! 사람이 누른 `start` 는 핸드오프에 귀속하지 않는 판정(`store`) 등이 이 한 벌을 쓴다.
//! 목록이 갈라지면 같은 actor 가 표면마다 다르게 분류되므로 단일 출처로 둔다.

/// 에이전트로 간주하는 actor 이름.
pub const AGENT_ACTORS: [&str; 5] = ["claude-code", "codex", "opencode", "agent", "rocky"];

/// Antigravity(`agy`)가 보드 도구에 넣는 actor — `antigravity/rules/AGENTS.md` 가 정한다. 에이전트 목록에는 일부러 없다
/// (아래 테스트).
pub const ANTIGRAVITY_ACTOR: &str = "antigravity";

/// 이 actor 가 에이전트인가. 모르는 이름은 사람으로 본다.
pub fn is_agent_actor(actor: &str) -> bool {
    AGENT_ACTORS.contains(&actor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_agents() {
        assert!(is_agent_actor("claude-code"));
        assert!(is_agent_actor("rocky"));
    }

    #[test]
    fn humans_by_default() {
        assert!(!is_agent_actor("logan"));
        assert!(!is_agent_actor(""));
    }

    /// Antigravity 로 넘긴 작업의 댓글·완료는 Claude Code 세션에 "호출자의 보드 변경"으로 주입돼야
    /// 넘긴 세션이 이어받는다 — 에이전트 목록에 넣으면 그 돌아오는 길이 끊긴다(`docs/antigravity.md`).
    #[test]
    fn antigravity_stays_out_of_agents() {
        assert!(!is_agent_actor(ANTIGRAVITY_ACTOR));
    }
}
