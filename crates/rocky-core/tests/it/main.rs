//! 통합 테스트 — 크레이트당 실행 파일 하나. 파일마다 실행 파일이 따로면 링크와 macOS 의 새 실행 파일 검사
//! (`syspolicyd`)가 파일 수만큼 돌아 테스트가 20~30분 걸렸다(2026-10-02). 새 테스트 파일은 여기 `mod` 로 단다.

mod actor_test;
mod config_test;
mod doing_test;
mod handoff_test;
mod inbox_test;
mod local_request_test;
mod logindex_test;
mod migrations_test;
mod next_test;
mod note_doc_test;
mod notify_test;
mod peer_inbox_test;
mod prwatch_test;
mod refs_test;
mod sessions_test;
mod setup_test;
mod statusline_test;
mod store_test;
mod summary_test;
mod transcript_test;
mod usage_test;
mod version_test;
mod worklog_test;
