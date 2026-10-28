//! `rocky uninstall` 의 순수 판정 — rocky 가 기기에 깐 것 중 무엇을 걷고 무엇을 남길지. 실행(claude·launchctl·
//! kill·파일 삭제)은 `rocky_cli::uninstall_cmd` 가 한다.
//!
//! 걷는 것: 플러그인과 마켓플레이스, launchd job(bootout 하나 — bootstrap 은 하지 않는다), 떠 있는 데몬(pid 로만),
//! `~/.local/bin/rocky` 링크(설치 폴더를 가리킬 때만), 설치 폴더(`$XDG_DATA_HOME/rocky`). 데이터(설정·보드 DB·워크로그·
//! 사용 로그)는 `--purge` 일 때만 지우고, 그때도 홈 안의 rocky 폴더이면서 git 레포가 아닌 것만 지운다.
//! rocky 가 띄웠지만 사람 것이기도 한 것(rc 서버의 claude 세션, tailscale serve, statusLine 설정)은 손대지 않고 알린다.

use std::path::{Path, PathBuf};

/// `--purge` 후보 폴더 하나.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDir {
    pub path: PathBuf,
    /// git 레포이거나 그 안이다 — 설정이 레포를 가리키는 실수를 지우지 않는다.
    pub repo: bool,
}

#[derive(Debug, Clone, Default)]
pub struct UninstallInput {
    pub home: PathBuf,
    /// `$XDG_DATA_HOME/rocky` — 릴리스 바이너리·main 빌드·`current` 링크가 있는 설치 폴더.
    pub install_dir: PathBuf,
    pub install_dir_exists: bool,
    /// 설치 폴더가 git 레포(또는 그 안)다 — `XDG_DATA_HOME` 이 엉뚱한 곳이면 소스 체크아웃일 수 있다.
    pub install_dir_repo: bool,
    /// `~/.local/bin/rocky` 가 있으면 그 경로와 링크 대상(링크가 아닌 실제 파일이면 None).
    pub cli_link: Option<(PathBuf, Option<PathBuf>)>,
    /// launchd plist 가 있으면 그 경로.
    pub plist: Option<PathBuf>,
    pub purge: bool,
    /// 데이터 폴더 후보(설정·보드 DB·워크로그·사용 로그) 중 디스크에 있는 것.
    pub data_dirs: Vec<DataDir>,
    /// `~/.claude/settings.json` 의 statusLine 이 `rocky statusline` 을 부른다.
    pub statusline_wired: bool,
    /// `expose` 에 tailscale serve 가 있다.
    pub tailscale: bool,
    /// rc 서버를 띄운 기록(todo 폴더의 `rc/`)이 있다.
    pub rc: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// `claude plugin uninstall rocky@rocky-marketplace` → `claude plugin marketplace remove rocky-marketplace`.
    Plugin,
    /// launchd job 을 bootout 하고 이 plist 를 지운다.
    Launchd(PathBuf),
    /// launchd 밖에서 떠 있는 데몬을 `/api/health` 의 pid 로 내린다.
    StopDaemon,
    RemoveLink(PathBuf),
    RemoveDir(PathBuf),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub steps: Vec<Step>,
    /// 지우지 않고 남기는 것과 그 까닭.
    pub keep: Vec<String>,
    /// 사람이 손으로 정리할 것.
    pub notes: Vec<String>,
}

/// 지워도 되는 데이터 폴더인가 — 홈 안(홈 자체는 아님)이고, 홈 아래 경로의 어느 이름에 `rocky` 가 들어 있다.
/// `.`·`..` 가 든 경로는 이름만으로 홈 밖을 가리킬 수 있어 받지 않는다(호출자가 canonicalize 해서 넘긴다).
pub fn removable(path: &Path, home: &Path) -> bool {
    use std::path::Component;
    let plain = |p: &Path| {
        p.components()
            .all(|c| !matches!(c, Component::CurDir | Component::ParentDir))
    };
    if !path.is_absolute()
        || home.as_os_str().is_empty()
        || path == home
        || !plain(path)
        || !plain(home)
    {
        return false;
    }
    let Ok(rel) = path.strip_prefix(home) else {
        return false;
    };
    rel.components().any(|c| {
        c.as_os_str()
            .to_string_lossy()
            .to_lowercase()
            .contains("rocky")
    })
}

pub fn plan(input: &UninstallInput) -> Plan {
    let mut out = Plan {
        steps: vec![Step::Plugin],
        ..Plan::default()
    };
    if let Some(plist) = &input.plist {
        out.steps.push(Step::Launchd(plist.clone()));
    }
    out.steps.push(Step::StopDaemon);

    match &input.cli_link {
        Some((link, Some(target))) if target.starts_with(&input.install_dir) => {
            out.steps.push(Step::RemoveLink(link.clone()));
        }
        Some((link, Some(target))) => out.keep.push(format!(
            "{} — rocky 설치 폴더가 아닌 {} 를 가리킨다",
            link.display(),
            target.display()
        )),
        Some((link, None)) => out.keep.push(format!(
            "{} — 링크가 아닌 실제 파일이다(rocky 가 만든 것이 아니다)",
            link.display()
        )),
        None => {}
    }
    // 지우면 안 되는 폴더 — 레포이거나 안에 레포 후보를 품었거나 홈 안의 rocky 폴더가 아니다. 바깥 폴더를 지우면
    // 안의 것도 같이 지워지므로, 안에 레포가 있으면 바깥도 남긴다.
    let refusal = |d: &DataDir| -> Option<String> {
        if d.repo {
            return Some("git 레포라 지우지 않는다".into());
        }
        if let Some(inner) = input
            .data_dirs
            .iter()
            .find(|o| o.repo && o.path != d.path && o.path.starts_with(&d.path))
        {
            return Some(format!(
                "안에 git 레포({})가 있어 지우지 않는다",
                inner.path.display()
            ));
        }
        (!removable(&d.path, &input.home))
            .then(|| "홈 안의 rocky 폴더가 아니라 지우지 않는다".into())
    };
    // 설치 폴더 안에 데이터 폴더(예: `ROCKY_WORKLOG_DIR` 을 그 아래 둔 것)가 있으면, 그 데이터를 지우는 실행(--purge 이고
    // 지워도 되는 폴더)이 아닌 한 설치 폴더째 지우지 않는다 — 남긴다고 적은 데이터가 재귀 삭제로 사라진다.
    let holds_data = input
        .data_dirs
        .iter()
        .find(|d| d.path.starts_with(&input.install_dir) && (!input.purge || refusal(d).is_some()));
    if input.install_dir_exists {
        if input.install_dir_repo || !removable(&input.install_dir, &input.home) {
            out.keep.push(format!(
                "{} — git 레포이거나 홈 안의 rocky 폴더가 아니라 지우지 않는다(XDG_DATA_HOME 확인)",
                input.install_dir.display()
            ));
        } else if let Some(data) = holds_data {
            out.keep.push(format!(
                "{} — 안에 남길 데이터 폴더({})가 있어 지우지 않는다",
                input.install_dir.display(),
                data.path.display()
            ));
        } else {
            out.steps.push(Step::RemoveDir(input.install_dir.clone()));
        }
    }
    // 바깥 폴더가 지워지면(purge) 또는 남는다고 이미 적으면(기본) 안의 폴더는 따로 적지 않는다.
    let covered = |d: &DataDir| {
        input.data_dirs.iter().any(|o| {
            o.path != d.path
                && d.path.starts_with(&o.path)
                && (!input.purge || refusal(o).is_none())
        })
    };
    let mut kept = Vec::new();
    for dir in input.data_dirs.iter().filter(|d| !covered(d)) {
        if !input.purge {
            kept.push(dir.path.display().to_string());
        } else if let Some(why) = refusal(dir) {
            out.keep.push(format!("{} — {why}", dir.path.display()));
        } else {
            out.steps.push(Step::RemoveDir(dir.path.clone()));
        }
    }
    if !kept.is_empty() {
        out.keep.push(format!(
            "{} — 설정·보드·워크로그·사용 로그. 다시 깔면 그대로 이어진다(지우려면 --purge)",
            kept.join(", ")
        ));
    }

    if input.rc {
        out.notes.push(
            "rc 서버로 떠 있는 claude 세션은 내리지 않았다 — 필요하면 앱이나 pid 로 닫는다".into(),
        );
    }
    if input.tailscale {
        out.notes.push(
            "tailscale serve 설정은 그대로다 — 더 안 쓰면 `tailscale serve --https=443 off`".into(),
        );
    }
    if input.statusline_wired {
        out.notes.push(
            "~/.claude/settings.json 의 statusLine 이 rocky statusline 을 부른다 — 그 항목을 지운다".into(),
        );
    }
    out.notes.push(
        "열린 Claude Code 세션은 먼저 닫는다 — 열린 세션의 훅은 바이너리가 없으면 다시 받아 데몬을 띄운다".into(),
    );
    out.notes
        .push("Codex·opencode·Antigravity 에 rocky MCP 를 등록했으면 그 설정에서 뺀다".into());
    out
}

/// 확인 전에 보여 줄 계획. `--dry-run` 의 출력이기도 하다.
pub fn render_plan(plan: &Plan) -> String {
    let mut lines = vec!["rocky uninstall — 걷어낼 것:".to_string()];
    for step in &plan.steps {
        lines.push(format!("  - {}", describe(step)));
    }
    if !plan.keep.is_empty() {
        lines.push("남길 것:".into());
        lines.extend(plan.keep.iter().map(|k| format!("  - {k}")));
    }
    if !plan.notes.is_empty() {
        lines.push("손으로 정리할 것:".into());
        lines.extend(plan.notes.iter().map(|n| format!("  - {n}")));
    }
    lines.join("\n")
}

pub fn describe(step: &Step) -> String {
    match step {
        Step::Plugin => {
            "Claude Code 플러그인 rocky@rocky-marketplace 와 마켓플레이스 rocky-marketplace".into()
        }
        Step::Launchd(plist) => format!(
            "launchd 상주 job — bootout 하고 {} 를 지운다",
            plist.display()
        ),
        Step::StopDaemon => "떠 있는 데몬(pid 로 SIGTERM)".into(),
        Step::RemoveLink(path) => format!("{} 링크", path.display()),
        Step::RemoveDir(path) => format!("{} 폴더", path.display()),
    }
}
