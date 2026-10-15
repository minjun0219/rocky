//! cwd 로 보드를 고른다 — 순수 함수. 세션 목록·rocky 채널·훅·받은편지함 등록이 보드를 고를 때 쓴다.

/// `boardKeyForCwd` 가 보는 보드 정보.
#[derive(Debug, Clone)]
pub struct BoardLocation {
    pub key: String,
    pub path: Option<String>,
}

/// 끝의 `/` 를 떼어 비교 가능한 꼴로. 루트는 `/` 로 남긴다.
fn normalize_path(path: &str) -> String {
    let stripped = path.trim_end_matches('/');
    if stripped.is_empty() {
        "/".to_string()
    } else {
        stripped.to_string()
    }
}

/// `/a/b` 가 `/a/bc` 에 걸리지 않도록 경로 경계까지 본다.
/// `cwd` 가 `path` 자신이거나 그 아래인가(끝 `/` 무시). 보드 path 판정의 공통 규약.
pub fn is_under(cwd: &str, path: &str) -> bool {
    let base = normalize_path(path);
    let here = normalize_path(cwd);
    if here == base {
        return true;
    }
    if base == "/" {
        here.starts_with('/')
    } else {
        here.starts_with(&format!("{base}/"))
    }
}

/// cwd 로 보드를 고른다 — `boards.path` 하위(가장 정확) → key 가 cwd 의 경로 세그먼트.
/// 후보가 여럿이면 가장 긴 것(더 구체적인 쪽).
pub fn board_key_for_cwd(boards: &[BoardLocation], cwd: Option<&str>) -> Option<String> {
    let cwd = cwd?;
    let mut by_path: Vec<(&BoardLocation, String)> = boards
        .iter()
        .filter_map(|b| b.path.as_ref().map(|p| (b, normalize_path(p))))
        .filter(|(_, p)| is_under(cwd, p))
        .collect();
    by_path.sort_by_key(|(_, p)| std::cmp::Reverse(p.len()));
    if let Some((board, _)) = by_path.first() {
        return Some(board.key.clone());
    }
    let segments: Vec<&str> = cwd.split('/').collect();
    let mut by_segment: Vec<&BoardLocation> = boards
        .iter()
        .filter(|b| !b.key.is_empty() && segments.contains(&b.key.as_str()))
        .collect();
    by_segment.sort_by_key(|b| std::cmp::Reverse(b.key.len()));
    by_segment.first().map(|b| b.key.clone())
}
