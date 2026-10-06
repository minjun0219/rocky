//! 통합 테스트 — 크레이트당 실행 파일 하나. 파일마다 실행 파일이 따로면 링크와 macOS 의 새 실행 파일 검사
//! (`syspolicyd`)가 파일 수만큼 돌아 테스트가 20~30분 걸렸다(2026-10-02). 새 테스트 파일은 여기 `mod` 로 단다.

mod client_test;
mod commands_test;
mod common;
mod config_cmd_test;
mod flags_test;
mod format_test;
mod hook_wiring_test;
mod hooks_test;
mod launchd_test;
mod rc_cmd_test;
mod statusline_cmd_test;
mod statusline_guard_test;
mod statusline_refresh_test;
mod tokens_cmd_test;
mod verify_cmd_test;
mod worklog_mcp_test;
