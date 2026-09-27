//! rocky-tui — 보드를 터미널에 띄워 두고 보는 화면. 데몬(`rockyd`)의 REST/SSE 클라이언트일
//! 뿐이며 DB 도 어댑터도 모른다. 설계: `docs/design/specs/2026-09-27-bridges-and-tui-design.md`.
//!
//! 층: `api`(HTTP) · `events`(SSE 스레드) · `app`(순수 상태·키 매핑) · `ui`(렌더).
//! 순수 판정은 전부 `app` 에 있어 터미널 없이 테스트한다.

pub mod api;
pub mod app;
pub mod events;
pub mod github;
pub mod ui;
