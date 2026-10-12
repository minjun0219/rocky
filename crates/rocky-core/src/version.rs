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

/// 이 바이너리를 빌드한 커밋의 짧은 해시 — `build.rs` 가 **release 빌드에만** 굽는다. dev 빌드·git 밖이면 None.
/// 훅의 데몬 교체 판단에는 쓰지 않는다 — 그건 지금처럼 semver 문자열(`CARGO_PKG_VERSION`)로만 한다.
pub const GIT_COMMIT: Option<&str> = option_env!("ROCKY_GIT_COMMIT");

/// 사람에게 보이는 버전 — `0.49.0 (abc1234)`. 커밋을 모르면(빈 값 포함) `0.49.0`.
pub fn label(version: &str, commit: Option<&str>) -> String {
    match commit.filter(|c| !c.is_empty()) {
        Some(c) => format!("{version} ({c})"),
        None => version.to_string(),
    }
}

/// 이 바이너리 자신의 [`label`].
pub fn build_label() -> String {
    label(env!("CARGO_PKG_VERSION"), GIT_COMMIT)
}

/// 포트의 데몬이 이 바이너리와 같은 빌드인가 — `daemon restart` 의 확인용. 버전은 정확히 같아야 하고, 커밋은
/// **둘 다 알 때만** 비교한다(main 빌드끼리는 버전이 같아 커밋으로만 갈린다). 한쪽이 dev 빌드·구버전이라
/// 커밋을 모르면 버전만 본다.
pub fn same_build(
    running: Option<&str>,
    running_commit: Option<&str>,
    mine: &str,
    mine_commit: Option<&str>,
) -> bool {
    running == Some(mine)
        && match (running_commit, mine_commit) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        }
}
