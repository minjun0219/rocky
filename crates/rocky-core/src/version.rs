//! 버전 문자열 비교 — `X.Y.Z` 와 `X.Y.Z-next.N` 만. 데몬이 보고한 버전이 이 설치본보다
//! **오래됐는가** 를 판정하는 데 쓴다(매 턴 훅이 구버전 데몬만 올리고, 내려가진 않게).
//!
//! `semver` 크레이트를 안 쓰는 이유: 필요한 건 세 자리 수와 프리릴리스 꼬리 하나뿐이고,
//! 런타임 의존을 늘리지 않는다는 레포 규칙이 있다.

/// 파싱된 버전. 프리릴리스(`-next.1`)는 같은 숫자의 정식 릴리스보다 **앞**이다.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// `None` = 정식. `Some` 은 `-` 뒤를 `.` 로 나눠 숫자는 숫자로, 아니면 문자열로 비교.
    pre: Option<Vec<PreIdent>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum PreIdent {
    Num(u64),
    Text(String),
}

impl Version {
    pub fn is_prerelease(&self) -> bool {
        self.pre.is_some()
    }
}

/// `1.2.3` / `v1.2.3` / `1.2.3-next.4`. 그 밖은 None.
pub fn parse_version(raw: &str) -> Option<Version> {
    let raw = raw.trim().strip_prefix('v').unwrap_or(raw.trim());
    let (core, pre) = match raw.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (raw, None),
    };
    let mut nums = core.split('.').map(|s| s.parse::<u64>().ok());
    let major = nums.next()??;
    let minor = nums.next()??;
    let patch = nums.next()??;
    if nums.next().is_some() {
        return None;
    }
    let pre = match pre {
        None => None,
        Some("") => return None,
        Some(p) => Some(
            p.split('.')
                .map(|s| match s.parse::<u64>() {
                    Ok(n) => PreIdent::Num(n),
                    Err(_) => PreIdent::Text(s.to_string()),
                })
                .collect(),
        ),
    };
    Some(Version {
        major,
        minor,
        patch,
        pre,
    })
}

/// 정식 vs 프리릴리스 순서 — 같은 숫자면 정식이 뒤(더 새롭다).
fn order(v: &Version) -> (u64, u64, u64, bool, Vec<PreIdent>) {
    (
        v.major,
        v.minor,
        v.patch,
        v.pre.is_none(),
        v.pre.clone().unwrap_or_default(),
    )
}

/// `running` 이 `mine` 보다 오래됐는가. 어느 쪽이든 못 읽으면 **false** — 모르는 건
/// 건드리지 않는다(재기동은 한 방향으로만, 확실할 때만).
pub fn is_older(running: &str, mine: &str) -> bool {
    match (parse_version(running), parse_version(mine)) {
        (Some(a), Some(b)) => order(&a) < order(&b),
        _ => false,
    }
}
