//! 로그 색인 스레드 — `rocky_core::logindex` 를 주기적으로 돌린다(작업로그·사용 로그·Claude Code 트랜스크립트).
//!
//! **전용 OS 스레드**다(tokio 작업이 아니다): 수천 줄을 읽어 넣는 동안 tokio 워커를 쥐지 않고, 자기 연결로
//! `logs.db` 를 쓰므로 보드 DB(`todo.db`)의 잠금과도 겹치지 않는다. 실패는 다음 바퀴에 다시 — 색인이 없어도
//! 보드는 돈다(작업로그 탭만 비어 보인다).

use std::path::PathBuf;
use std::time::Duration;

use rocky_core::logindex::LogIndex;

/// 기동 직후 한 번, 그 뒤 `every` 마다. 스레드를 띄우지 못하면 경고만 남긴다.
pub fn spawn_indexer(
    db: PathBuf,
    worklog_root: PathBuf,
    usage_dir: Option<PathBuf>,
    transcripts_dir: Option<PathBuf>,
    every: Duration,
) {
    let spawned = std::thread::Builder::new()
        .name("rocky-logindex".into())
        .spawn(move || {
            let mut index: Option<LogIndex> = None;
            let mut last_error: Option<String> = None;
            loop {
                if index.is_none() {
                    match LogIndex::open(&db) {
                        Ok(opened) => index = Some(opened),
                        Err(e) => report(&mut last_error, format!("{}: {e}", db.display())),
                    }
                }
                if let Some(idx) = index.as_mut() {
                    let mut result = idx.ingest_worklog_root(&worklog_root).map(|_| ());
                    if let (Ok(()), Some(dir)) = (&result, &usage_dir) {
                        result = idx.ingest_usage_dir(dir).map(|_| ());
                    }
                    if let (Ok(()), Some(dir)) = (&result, &transcripts_dir) {
                        result = idx.ingest_transcripts(dir).map(|_| ());
                    }
                    match result {
                        Ok(()) => last_error = None,
                        Err(e) => report(&mut last_error, e),
                    }
                }
                std::thread::sleep(every);
            }
        });
    if let Err(e) = spawned {
        eprintln!("rocky: 로그 색인 스레드를 띄우지 못했다 — {e}");
    }
}

/// 같은 실패를 1분마다 다시 찍지 않는다 — 바뀌었을 때만.
fn report(last: &mut Option<String>, error: String) {
    if last.as_deref() != Some(error.as_str()) {
        eprintln!("rocky: 로그 색인 실패 — {error}");
        *last = Some(error);
    }
}
