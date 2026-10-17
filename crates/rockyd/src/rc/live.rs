//! rc 설정을 다시 읽는 자리 — 프로브 · 명령이 설정을 볼 때마다 `rocky.json` 의 mtime 을 재고, 바뀌었으면 목록
//! (`root` · `pinned` · `targets`)만 다시 받는다. 손으로 고치든 `rocky rc add|remove|pin|unpin` 으로 고치든 데몬을 다시
//! 띄우지 않고 반영된다. 감시 · 야간 · `enabled` 는 기동 때 값 그대로다(잡을 띄울지 기동 때 정한다).
//!
//! 기동 때 rc 가 꺼져 있었으면(None) 계속 꺼진 채다 — 켜는 것은 데몬을 다시 띄울 때. 다시 읽다 실패하면(고치는 중의
//! 깨진 JSON · 블록을 지움 · `enabled: false`) 마지막으로 읽은 목록을 그대로 쓴다.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use rocky_core::config::RcConfig;

pub struct RcConfigLive {
    path: Option<PathBuf>,
    state: Mutex<(Option<SystemTime>, Option<RcConfig>)>,
}

impl RcConfigLive {
    /// 다시 읽지 않는 설정 — 테스트와 설정 파일이 없는 자리.
    pub fn fixed(config: Option<RcConfig>) -> Arc<Self> {
        Arc::new(RcConfigLive {
            path: None,
            state: Mutex::new((None, config)),
        })
    }

    /// `path` 의 mtime 을 지켜보는 설정. `initial` 은 기동 때 읽은 값이다.
    pub fn watch(path: PathBuf, initial: Option<RcConfig>) -> Arc<Self> {
        let mtime = mtime(&path);
        Arc::new(RcConfigLive {
            path: Some(path),
            state: Mutex::new((mtime, initial)),
        })
    }

    /// 지금의 설정. 파일이 바뀌었으면 목록을 다시 읽는다 — stat 한 번이라 프로브마다 불러도 싸다.
    pub fn current(&self) -> Option<RcConfig> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let Some(path) = &self.path else {
            return state.1.clone();
        };
        // 기동 때 꺼져 있었으면 계속 꺼진 채다.
        let base = state.1.clone()?;
        let now = mtime(path);
        if now != state.0 {
            state.0 = now;
            if let Some(fresh) = rocky_core::config::load_rc_block(path) {
                if fresh.pinned != base.pinned
                    || fresh.targets != base.targets
                    || fresh.root != base.root
                {
                    eprintln!(
                        "rocky: rc 대상 목록을 다시 읽었다 — {} (고정 {} · 대상 {})",
                        path.display(),
                        fresh.pinned.len(),
                        fresh.targets.len()
                    );
                }
                state.1 = Some(base.with_lists_from(&fresh));
            }
        }
        state.1.clone()
    }
}

fn mtime(path: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}
