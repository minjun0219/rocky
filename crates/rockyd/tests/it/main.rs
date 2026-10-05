//! 통합 테스트 — 크레이트당 실행 파일 하나. 파일마다 실행 파일이 따로면 링크와 macOS 의 새 실행 파일 검사
//! (`syspolicyd`)가 파일 수만큼 돌아 테스트가 20~30분 걸렸다(2026-10-02). 새 테스트 파일은 여기 `mod` 로 단다.

mod common;
mod github_test;
mod inbox_subscribe_test;
mod logindex_test;
mod mcp_test;
mod note_doc_test;
mod prwatch_test;
mod rc_test;
mod server_board_inbox_test;
mod server_handoff_test;
mod server_inbox_test;
mod server_issue_test;
mod server_rest_test;
mod server_spawn_test;
mod server_statusline_test;
mod sessions_swr_test;
mod shutdown_test;
mod spawnctl_test;
mod sweep_test;
mod tailscale_test;
mod tokens_test;
mod usage_test;
mod verify_test;
mod ws_test;
