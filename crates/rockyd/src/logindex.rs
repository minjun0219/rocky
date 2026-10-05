//! 로그 색인 스레드 — `rocky_core::logindex` 를 주기적으로 돌린다(작업로그·사용 로그·Claude Code 트랜스크립트).
//!
//! **전용 OS 스레드**다(tokio 작업이 아니다): 수천 줄을 읽어 넣는 동안 tokio 워커를 쥐지 않고, 자기 연결로
//! `logs.db` 를 쓰므로 보드 DB(`todo.db`)의 잠금과도 겹치지 않는다. 실패는 다음 바퀴에 다시 — 색인이 없어도
//! 보드는 돈다(작업로그 탭만 비어 보인다).

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::time::Duration;

use rocky_core::logindex::LogIndex;
use rocky_core::tokens::{recommendation_for, RecommendConfig};
use tokio::sync::broadcast;

/// 토큰 추천 피드 — 새 메시지가 들어간 세션의 추천을 다시 세어, **낸 규칙이 바뀐** 세션만 `GET /api/tokens/events`
/// 로 민다. 근거 수치는 턴마다 바뀌므로 비교하지 않는다(같은 추천을 턴마다 다시 알리지 않게). 기준선은 메모리에만
/// 두므로, 데몬이 다시 뜬 뒤 첫 바퀴에 움직이지 않은 세션은 다음에 움직일 때 같은 추천이 한 번 더 나갈 수 있다 —
/// 놓치는 것보다 한 번 더 알리는 쪽을 택했다.
pub struct RecommendationFeed {
    sender: broadcast::Sender<String>,
    cfg: RecommendConfig,
    last: HashMap<String, String>,
    seeded: bool,
}

impl RecommendationFeed {
    pub fn new(sender: broadcast::Sender<String>, cfg: RecommendConfig) -> Self {
        RecommendationFeed {
            sender,
            cfg,
            last: HashMap::new(),
            seeded: false,
        }
    }

    /// 민 건수를 돌려준다. 첫 바퀴는 과거 트랜스크립트를 통째로 가져오는 바퀴라 기준선만 잡고 보내지 않는다.
    pub fn publish(&mut self, index: &LogIndex, touched: &BTreeSet<String>) -> usize {
        let mut sent = 0;
        for id in touched {
            let Ok(rec) = recommendation_for(index.conn(), id, &self.cfg) else {
                continue;
            };
            let key = rec.rules_key();
            let before = self
                .last
                .insert(id.clone(), key.clone())
                .unwrap_or_default();
            if !self.seeded || before == key {
                continue;
            }
            if let Ok(payload) = serde_json::to_string(&rec) {
                let _ = self.sender.send(payload); // 구독자 없음은 정상
                sent += 1;
            }
        }
        self.seeded = true;
        sent
    }
}

/// 기동 직후 한 번, 그 뒤 `every` 마다. 스레드를 띄우지 못하면 경고만 남긴다.
pub fn spawn_indexer(
    db: PathBuf,
    worklog_root: PathBuf,
    usage_dir: Option<PathBuf>,
    transcripts_dir: Option<PathBuf>,
    mut feed: Option<RecommendationFeed>,
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
                        let ingest = idx.ingest_transcripts(dir);
                        if let Some(feed) = feed.as_mut() {
                            feed.publish(idx, &ingest.touched);
                        }
                        if let Some(first) = ingest.errors.first() {
                            result = Err(format!(
                                "트랜스크립트 {}개 실패(나머지는 옮김) — {first}",
                                ingest.errors.len()
                            ));
                        }
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
