//! rocky 도메인 코어 — TodoStore + migrations + 순수 판정 모듈.
//!
//! TS 원본(`src/*.ts`)과의 동작 동일성이 계약이다 — `docs/rewrite/contract.md` 참고.

pub mod actor;
pub mod actors;
pub mod config;
pub mod doing;
pub mod github;
pub mod handoff;
pub mod ids;
pub mod inbox;
pub mod limits;
pub mod local_request;
pub mod logindex;
pub mod migrations;
pub mod next;
pub mod note_doc;
pub mod notify;
pub mod peer_inbox;
pub mod prwatch;
pub mod rc;
pub mod refs;
pub mod sessions;
pub mod setup;
pub mod statusline;
pub mod store;
pub mod summary;
pub mod tokens;
pub mod transcript;
pub mod types;
pub mod usage;
pub mod verify;
pub mod version;
pub mod worklog;

pub use ids::{new_id, ID_LENGTH};
pub use store::{StoreError, StoreResult, TodoStore};
