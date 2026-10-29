//! main 빌드 모드의 순수 판정 — 이 기기가 릴리스 대신 GitHub main 을 직접 빌드해 쓰는지, 다시 빌드할 필요가
//! 있는지, 플러그인 마켓플레이스가 어디를 가리키는지를 정한다. 실행(git·cargo·bun·claude)은 CLI 가 한다.
//!
//! 자리(`$XDG_DATA_HOME/rocky/` 아래):
//! - `main-src/` — main 을 따라가는 전용 clone. 마켓플레이스 `rocky-marketplace` 가 이 폴더를 가리킨다.
//! - `main/` — release 빌드 `rocky`·`rockyd` + 웹 UI `dist/` + 빌드한 커밋 `.sha`.
//! - `current` → `main` — 이 링크가 곧 모드다. 부트스트랩(`plugin/bin/rocky`)은 이걸 보면 릴리스를 받지 않고
//!   `main/rocky` 를 실행한다. 릴리스 모드면 `current` → `v<version>`.

use serde_json::Value;

/// 마켓플레이스 이름 — `.claude-plugin/marketplace.json` 의 `name`.
pub const MARKETPLACE: &str = "rocky-marketplace";
/// 플러그인 설치 id.
pub const PLUGIN: &str = "rocky@rocky-marketplace";
/// main-src 를 처음 만들 때의 원격 — 공개 레포라 인증 없이 받는다.
pub const CLONE_URL: &str = "https://github.com/minjun0219/rocky.git";
/// `CLONE_URL` 의 레포(owner/name) — 보드 `repo` 와 견준다(검증 통과 커밋 자동 배포는 이 레포만).
pub const REPO: &str = "minjun0219/rocky";

/// 다른 main 빌드가 `main-sync.lock` 을 쥐고 있나 — `rocky update --main` 의 잠금 규칙과 같다. 잠금 폴더가 있고 안의
/// `pid` 가 살아 있으면 쥔 것이다. pid 가 없거나 읽지 못하면(기기의 다른 빌드 스크립트) 살아 있다고 본다. 죽은 pid 의
/// 잠금은 update 가 넘겨받으므로 비어 있는 것으로 본다.
pub fn lock_busy(exists: bool, pid: Option<&str>, alive: impl Fn(u32) -> bool) -> bool {
    exists
        && pid
            .and_then(|p| p.trim().parse::<u32>().ok())
            .is_none_or(alive)
}

/// `current` 링크가 가리키는 이름이 main 빌드 모드인가.
pub fn is_main_mode(current_target: Option<&str>) -> bool {
    current_target == Some("main")
}

/// 이번 실행이 할 일.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildPlan {
    /// 원하는 커밋이 깔려 있고 지난 실행이 끝까지 갔다 — 아무것도 안 한다.
    Skip,
    /// 원하는 커밋의 빌드는 있다 — 빌드는 건너뛰고 뒤 단계(링크·데몬 재시작·플러그인)만 한다. 지난 실행이 빌드 뒤에
    /// 실패했거나, 모드를 벗어났다가 같은 커밋으로 돌아온 경우다. 몇 분짜리 빌드를 다시 하지 않는다.
    Finish,
    /// 빌드부터.
    Build,
}

/// `have` 는 `main/.sha`(깔린 빌드의 커밋), `finished` 는 `main/.finished`(지난 실행이 끝까지 간 커밋).
/// `bundle_missing` 은 원하는 커밋에 TUI 번들 스크립트가 있는데 깔린 `main/` 에 `tui.js` 가 없다는 뜻이다 — TUI 를 모르던
/// 옛 rocky 가 그 커밋을 빌드해 번들 없이 깔았다. 커밋이 같아도 다시 빌드한다(안 그러면 다음 커밋까지 `rocky tui` 가 못 뜬다).
pub fn plan(
    want: &str,
    have: Option<&str>,
    finished: Option<&str>,
    force: bool,
    main_mode: bool,
    bundle_missing: bool,
) -> BuildPlan {
    let is_want = |s: Option<&str>| s.map(str::trim) == Some(want.trim());
    if force || bundle_missing || !is_want(have) {
        BuildPlan::Build
    } else if main_mode && is_want(finished) {
        BuildPlan::Skip
    } else {
        BuildPlan::Finish
    }
}

/// `rocky-marketplace` 가 가리키는 곳.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarketplaceSource {
    /// 폴더 마켓플레이스 — main 빌드 모드의 정상 상태(그 폴더가 `main-src` 면).
    Directory(String),
    /// GitHub 마켓플레이스 — 릴리스 모드.
    Github(String),
    /// 그 밖의 원천(git URL 등) — 손대지 않는다.
    Other(String),
    /// 등록돼 있지 않다.
    Missing,
}

/// `claude plugin marketplace list --json` 에서 `rocky-marketplace` 를 찾는다. JSON 이 아니면 None — 호출자는
/// 마켓플레이스를 건드리지 않는다(모르는 건 바꾸지 않는다).
pub fn parse_marketplace(json: &str) -> Option<MarketplaceSource> {
    let list: Value = serde_json::from_str(json).ok()?;
    let Some(entry) = list
        .as_array()?
        .iter()
        .find(|m| m.get("name").and_then(Value::as_str) == Some(MARKETPLACE))
    else {
        return Some(MarketplaceSource::Missing);
    };
    let field = |k: &str| {
        entry
            .get(k)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    Some(match entry.get("source").and_then(Value::as_str) {
        Some("directory") => MarketplaceSource::Directory(field("path")),
        Some("github") => MarketplaceSource::Github(field("repo")),
        other => MarketplaceSource::Other(other.unwrap_or("").to_string()),
    })
}

/// main 빌드 뒤 플러그인(훅·커맨드·스킬)을 어떻게 맞출지.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginStep {
    /// 이미 `main-src` 폴더 마켓플레이스다 — 빼고 다시 깐다. 폴더 마켓플레이스도 설치 때 캐시로 복사하고, 버전이
    /// 같으면 `claude plugin update` 가 "이미 최신" 이라 새 커밋을 안 가져온다(2026-10-03 실측).
    Reinstall,
    /// 릴리스(GitHub) 마켓플레이스거나 등록돼 있지 않다 — `main-src` 폴더 마켓플레이스로 바꾼다. 마켓플레이스를
    /// 빼면 그 플러그인의 저장된 옵션·데이터도 지워지므로 사람이 확인한 뒤에만 한다.
    Switch,
    /// 다른 폴더·원천을 가리킨다(개발용 `--plugin-dir` 연결 등) — 손대지 않는다. 이유를 한 줄로.
    Leave(String),
}

/// `main_src` 는 호출자가 정규화한 `main-src` 경로.
pub fn plugin_step(source: &MarketplaceSource, main_src: &str) -> PluginStep {
    let same = |p: &str| p.trim_end_matches('/') == main_src.trim_end_matches('/');
    match source {
        MarketplaceSource::Directory(p) if same(p) => PluginStep::Reinstall,
        MarketplaceSource::Directory(p) => PluginStep::Leave(format!(
            "{MARKETPLACE} 가 다른 폴더({p})를 가리킨다 — 손대지 않았다"
        )),
        MarketplaceSource::Github(_) | MarketplaceSource::Missing => PluginStep::Switch,
        MarketplaceSource::Other(kind) => {
            PluginStep::Leave(format!("{MARKETPLACE} 의 원천이 {kind} 다 — 손대지 않았다"))
        }
    }
}

/// `rocky update --release` 가 마켓플레이스를 어떻게 되돌릴지.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseStep {
    /// `main-src` 폴더 마켓플레이스다 — 빼고 GitHub 를 더한다(빼면 플러그인도 같이 빠진다).
    FromFolder,
    /// 등록돼 있지 않다 — GitHub 를 더한다.
    Add,
    /// 이미 GitHub 다(예: 지난 `--main` 의 전환이 실패) — 갱신한 뒤 플러그인만 다시 깐다. 갱신 없이 깔면 옛 클론의
    /// 버전이 깔린다.
    Refresh,
    /// 다른 폴더·원천(개발용 연결) — 손대지 않고 멈춘다. 이유를 한 줄로.
    Leave(String),
}

/// `main_src` 는 호출자가 정규화한 `main-src` 경로.
pub fn release_step(source: &MarketplaceSource, main_src: &str) -> ReleaseStep {
    match plugin_step(source, main_src) {
        PluginStep::Reinstall => ReleaseStep::FromFolder,
        PluginStep::Leave(why) => ReleaseStep::Leave(why),
        PluginStep::Switch if *source == MarketplaceSource::Missing => ReleaseStep::Add,
        PluginStep::Switch => ReleaseStep::Refresh,
    }
}

/// nightly 모드의 `current` 대상 — CI 가 매일 밤 낸 프리릴리스(`v<version>-nightly.<YYYYMMDD>`)를 받아 `nightly/` 에 둔다
/// (`rocky update --nightly`). 설계는 `docs/design/specs/2026-10-29-nightly-channel-design.md`.
pub const NIGHTLY: &str = "nightly";

/// `current` 링크가 가리키는 이름이 nightly 모드인가.
pub fn is_nightly_mode(current_target: Option<&str>) -> bool {
    current_target == Some(NIGHTLY)
}

/// 플러그인 버전과 상관없이 고정 폴더(`main/`·`nightly/`)의 바이너리를 쓰는 모드인가 — 부트스트랩이 링크를 버전으로
/// 되돌리지 않고, launchd plist 가 그 폴더의 `rockyd` 를 그대로 굽는 모드다.
pub fn is_pinned_mode(current_target: Option<&str>) -> bool {
    is_main_mode(current_target) || is_nightly_mode(current_target)
}

/// nightly 태그 하나 — `v0.54.0-nightly.20261029` 또는 같은 날 두 번째 `….20261029.2`(scripts/nightly.ts 와 같은 규칙).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NightlyTag {
    pub tag: String,
    pub date: String,
    pub seq: u32,
}

/// nightly 태그를 읽는다. 정식·`-next` 태그나 모양이 다르면 None.
pub fn parse_nightly_tag(tag: &str) -> Option<NightlyTag> {
    let tag = tag.trim();
    let (version, tail) = tag.strip_prefix('v')?.split_once("-nightly.")?;
    let nums: Vec<&str> = version.split('.').collect();
    if nums.len() != 3
        || !nums
            .iter()
            .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let (date, seq) = match tail.split_once('.') {
        Some((date, seq)) => (date, seq.parse::<u32>().ok()?),
        None => (tail, 1),
    };
    (date.len() == 8 && date.bytes().all(|b| b.is_ascii_digit())).then(|| NightlyTag {
        tag: tag.to_string(),
        date: date.to_string(),
        seq,
    })
}

/// `git ls-remote --tags` 출력에서 가장 최근 nightly 의 (태그, 커밋). 버전이 아니라 날짜·순번으로 고른다. annotated 태그는
/// `^{}` 줄의 커밋을 쓴다. 커밋은 40자 16진만 받는다 — git argv 에 넣는다.
pub fn latest_nightly(ls_remote: &str) -> Option<(String, String)> {
    let mut tags: Vec<(NightlyTag, String)> = Vec::new();
    for line in ls_remote.lines() {
        let mut parts = line.split_whitespace();
        let (Some(sha), Some(name)) = (parts.next(), parts.next()) else {
            continue;
        };
        let Some(name) = name.strip_prefix("refs/tags/") else {
            continue;
        };
        if sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let (name, peeled) = match name.strip_suffix("^{}") {
            Some(n) => (n, true),
            None => (name, false),
        };
        let Some(tag) = parse_nightly_tag(name) else {
            continue;
        };
        match tags.iter_mut().find(|(t, _)| t.tag == tag.tag) {
            Some(entry) if peeled => entry.1 = sha.to_string(),
            Some(_) => {}
            None => tags.push((tag, sha.to_string())),
        }
    }
    tags.into_iter()
        .max_by(|(a, _), (b, _)| (&a.date, a.seq).cmp(&(&b.date, b.seq)))
        .map(|(t, sha)| (t.tag, sha))
}

/// 그 nightly 의 tarball 이름 — 정식 릴리스와 같은 규칙(`scripts/package-macos.sh`).
pub fn nightly_asset(tag: &str) -> String {
    format!("rocky-{tag}-aarch64-apple-darwin.tar.gz")
}
