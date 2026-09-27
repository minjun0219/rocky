/**
 * actor 분류 — "이 변경을 사람이 했나 에이전트가 했나". doing 뱃지의 온도(warm/cool)를 가른다.
 * 정본은 Rust `rocky_core::actors`(주입 필터·핸드오프 귀속이 같은 목록을 쓴다) — 갈라지면
 * 화면에서는 에이전트인데 주입 필터에서는 사람이 되는 식으로 어긋나니 여기 목록을 그쪽과 맞춘다.
 */

/** 에이전트로 간주하는 actor 이름. */
export const AGENT_ACTORS: ReadonlySet<string> = new Set([
  'claude-code',
  'codex',
  'opencode',
  'agent',
  'rocky',
]);

/** 이 actor 가 에이전트인가. 모르는 이름은 사람으로 본다. */
export function isAgentActor(actor: string): boolean {
  return AGENT_ACTORS.has(actor);
}
