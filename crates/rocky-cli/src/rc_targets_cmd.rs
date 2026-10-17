//! `rocky rc targets` · `rocky rc reload` — rc 대상 목록을 rocky.json 에서 고치고, 떠 있는 데몬이 다시 읽게 한다. 판정은
//! `rocky_core::rc_targets`, 여기는 설정 파일 읽기 · 쓰기, 데몬에 알리기, 출력이다.
//!
//! - `rocky rc targets [--json]` — 설정 파일에 적힌 대상(데몬이 쓰는 순서 · 규칙 그대로)
//! - `rocky rc targets add <이름|경로> [--pin]` — 대상을 더한다. `.` 은 지금 폴더, `--pin` 은 고정
//! - `rocky rc targets rm <이름|경로>` — 대상을 뺀다. 떠 있는 서버는 그대로 둔다(대상 밖 서버로 보인다)
//! - `rocky rc reload` — 손으로 고친 rocky.json 을 데몬이 다시 읽는다
//!
//! 더하거나 뺀 뒤에는 파일이 그대로여도 데몬에 다시 읽으라고 한다 — 손으로 고치고 다시 읽히지 않은 목록도 이참에 맞춘다.

use std::path::Path;

use rocky_core::config::user_config_path;
use rocky_core::rc_targets::{self, Change, Edit, TargetList};
use rocky_core::statusline::width::display_width;
use serde_json::{json, Value};

use crate::client::{request_value, CliContext};
use crate::commands::Printer;
use crate::flags::ParsedFlags;

const RELOAD: &str = "/api/rc/reload";
const OLD_DAEMON: &str = "데몬이 다시 읽기를 모르는 옛 버전이다. 반영하려면: rocky daemon restart";

const USAGE: &str = "\
usage: rocky rc targets [--json]                 rocky.json 의 rc 대상(이름 · 고정 · 폴더)
       rocky rc targets add <이름|경로> [--pin]  대상을 더한다(. 은 지금 폴더, --pin 은 고정)
       rocky rc targets rm <이름|경로>           대상을 뺀다(떠 있는 서버는 그대로 둔다)
       rocky rc reload                           손으로 고친 rocky.json 을 데몬이 다시 읽는다";

pub fn cmd_targets(
    ctx: &CliContext,
    rest: &[String],
    flags: &ParsedFlags,
    printer: &Printer,
) -> Result<(), String> {
    let path = user_config_path();
    let home =
        std::env::var("HOME").map_err(|_| "HOME 이 없어 대상 폴더를 풀지 못한다".to_string())?;
    match rest.first().map(String::as_str) {
        None | Some("ls") => {
            let (_, doc) = read_config(&path)?;
            let list = rc_targets::list_targets(&doc, &home, &is_dir);
            let mut raw = json!(list);
            raw["path"] = json!(path.to_string_lossy());
            printer.emit(&raw, || {
                render_list(&list, &tilde(&path.to_string_lossy(), &home), &home)
            });
            Ok(())
        }
        Some(verb @ ("add" | "rm")) => {
            let key = rest
                .get(1)
                .ok_or_else(|| format!("대상 이름이나 경로가 필요하다\n\n{USAGE}"))?;
            if rest.len() > 2 {
                return Err(format!("대상은 하나씩 고친다: {}", rest[1..].join(" ")));
            }
            if verb == "rm" && flags.bool_flag("pin") {
                return Err("--pin 은 add 에만 쓴다".into());
            }
            let cwd = std::env::current_dir()
                .map_err(|e| format!("지금 폴더를 알지 못한다: {e}"))?
                .to_string_lossy()
                .to_string();
            let (original, mut doc) = read_config(&path)?;
            let edit = if verb == "add" {
                let pin = flags.bool_flag("pin");
                rc_targets::add_target(&mut doc, key, pin, &cwd, &home, &is_dir)?
            } else {
                rc_targets::remove_target(&mut doc, key, &cwd, &home)?
            };
            let written = edit.change != Change::Unchanged;
            if written {
                write_config(&path, &rc_targets::render_config(&doc, original.as_deref()))?;
            }
            let reload = request_value(ctx, "POST", RELOAD, Some(&json!({})));
            // 뺀 대상의 서버가 떠 있으면 이제 대상 밖 서버다 — 닫는 법을 알린다. 다시 읽었으면 현황은 새로 잰다.
            let stray = match (&reload, edit.change) {
                (Ok(_), Change::Removed) => request_value(ctx, "GET", "/api/rc/servers", None)
                    .ok()
                    .and_then(|raw| stray_pid(&raw, &edit.dir)),
                _ => None,
            };
            let raw = json!({
                "edit": edit,
                "path": path.to_string_lossy(),
                "written": written,
                "created": written && original.is_none(),
                "reload": reload.as_ref().ok(),
                "reloadError": reload.as_ref().err(),
                "strayPid": stray,
            });
            printer.emit(&raw, || {
                let mut lines = vec![edit_line(&edit, &home)];
                if written && original.is_none() {
                    lines.push(format!(
                        "  설정 파일을 새로 만들었다: {}",
                        tilde(&path.to_string_lossy(), &home)
                    ));
                }
                if edit.disabled {
                    lines.push(
                        "  이 기기에서는 rc 가 꺼져 있다(rc.enabled: false). 목록만 고쳤다".into(),
                    );
                }
                lines.extend(match &reload {
                    Ok(raw) => reload_lines(raw, &path.to_string_lossy(), Some(&edit)),
                    Err(e) => reload_failure(e),
                });
                if let Some(pid) = stray {
                    lines.push(format!(
                        "  서버는 그대로 떠 있다(pid {pid}). 닫으려면: rocky rc stop {pid}"
                    ));
                }
                lines.join("\n")
            });
            Ok(())
        }
        Some(sub) => Err(format!("모르는 하위 명령: targets {sub}\n\n{USAGE}")),
    }
}

/// `rocky rc reload` — 데몬이 설정 파일을 다시 읽는다. 파일을 못 읽으면 데몬은 하던 목록을 그대로 쓰고 실패로 끝난다.
pub fn cmd_reload(ctx: &CliContext, printer: &Printer) -> Result<(), String> {
    let raw = request_value(ctx, "POST", RELOAD, Some(&json!({}))).map_err(|e| {
        if old_daemon(&e) {
            OLD_DAEMON.to_string()
        } else {
            e
        }
    })?;
    let path = user_config_path();
    printer.emit(&raw, || {
        reload_lines(&raw, &path.to_string_lossy(), None)
            .join("\n")
            .trim_start()
            .to_string()
    });
    Ok(())
}

/// 설정 파일과 그 문서 — 파일이 없으면 (None, 기본 문서). JSON 이 아니면 거절한다(고치다 만 파일을 덮어쓰지 않게).
fn read_config(path: &Path) -> Result<(Option<String>, Value), String> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let doc = serde_json::from_str(&text).map_err(|e| {
                format!(
                    "설정 파일이 JSON이 아니다({}): {e}. 손으로 고친 뒤 다시 한다",
                    path.display()
                )
            })?;
            Ok((Some(text), doc))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((None, rc_targets::new_config())),
        Err(e) => Err(format!("설정 파일을 읽지 못했다({}): {e}", path.display())),
    }
}

/// 설정 파일을 통째로 바꾼다. 있던 파일은 같은 폴더의 임시 파일에 쓴 뒤 이름을 바꿔 덮고(쓰다 끊겨도 반쪽 파일이 남지 않게)
/// 권한은 그대로 둔다. 링크면 링크가 가리키는 파일을 바꾼다(dotfiles 링크가 일반 파일로 바뀌지 않게). 없던 파일은
/// `rocky config init` 처럼 그냥 쓴다.
pub fn write_config(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    let fail = |at: &Path, e: &dyn std::fmt::Display| {
        format!("설정 파일을 쓰지 못했다({}): {e}", at.display())
    };
    let Ok(target) = std::fs::canonicalize(path) else {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| fail(dir, &e))?;
        }
        return std::fs::write(path, text).map_err(|e| fail(path, &e));
    };
    let dir = target.parent().unwrap_or(Path::new("/"));
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(|e| fail(dir, &e))?;
    tmp.write_all(text.as_bytes())
        .map_err(|e| fail(tmp.path(), &e))?;
    if let Ok(meta) = std::fs::metadata(&target) {
        tmp.as_file()
            .set_permissions(meta.permissions())
            .map_err(|e| fail(tmp.path(), &e))?;
    }
    tmp.persist(&target).map_err(|e| fail(&target, &e.error))?;
    Ok(())
}

fn is_dir(dir: &str) -> bool {
    Path::new(dir).is_dir()
}

/// 홈 아래 경로를 `~` 꼴로 줄인다(보여 줄 때만).
pub fn tilde(dir: &str, home: &str) -> String {
    let home = home.trim_end_matches('/');
    match dir.strip_prefix(home) {
        Some("") if !home.is_empty() => "~".into(),
        Some(rest) if !home.is_empty() && rest.starts_with('/') => format!("~{rest}"),
        _ => dir.to_string(),
    }
}

/// 현황에서 그 폴더의 대상 밖 서버 pid — 뺀 대상의 서버가 아직 떠 있나.
pub fn stray_pid(raw: &Value, dir: &str) -> Option<u64> {
    raw.get("strays")?
        .as_array()?
        .iter()
        .find(|s| s.get("dir").and_then(Value::as_str) == Some(dir))?
        .get("pid")?
        .as_u64()
}

/// 고친 결과 한 줄 — `✓ 대상을 더했다: ogpeek (~/dev/workspaces/ogpeek)`.
pub fn edit_line(edit: &Edit, home: &str) -> String {
    let what = match (edit.change, edit.pinned) {
        (Change::Added, false) => "✓ 대상을 더했다",
        (Change::Added, true) => "✓ 고정 대상을 더했다",
        (Change::Pinned, _) => "✓ 고정 대상으로 바꿨다",
        (Change::Unchanged, false) => "= 이미 대상이다",
        (Change::Unchanged, true) => "= 이미 고정 대상이다",
        (Change::Removed, _) => "✓ 대상을 뺐다",
    };
    format!("{what}: {} ({})", edit.label, tilde(&edit.dir, home))
}

/// 데몬의 다시 읽기 응답 → 사람이 읽을 줄. `edit` 이 있으면(targets add · rm) 그 대상에 맞춘 다음 할 일을 붙인다.
/// 줄은 두 칸 들여 쓴다(앞의 결과 줄 아래에 붙는다).
pub fn reload_lines(raw: &Value, cli_path: &str, edit: Option<&Edit>) -> Vec<String> {
    let mut lines = Vec::new();
    let labels = |key: &str| -> Vec<String> {
        raw.get(key)
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };
    if let Some(daemon_path) = raw.get("path").and_then(Value::as_str) {
        if daemon_path != cli_path {
            lines.push(format!(
                "  ⚠ 데몬은 다른 설정 파일을 읽는다: {daemon_path}. 이 셸의 설정 파일: {cli_path}"
            ));
            // 데몬은 제 파일을 다시 읽었을 뿐이라 고친 대상이 닿지 않았다 — 반영했다 · 띄우라고 하지 않는다.
            if edit.is_some() {
                lines.push("  이 셸의 파일을 고쳐도 데몬에는 반영되지 않는다. 데몬이 읽는 파일을 고치려면 ROCKY_CONFIG 를 그 경로로 설정하고 다시 한다".into());
                return lines;
            }
        }
    }
    let restart = labels("restartNeeded");
    let active = raw.get("active").and_then(Value::as_bool) == Some(true);
    if restart.iter().any(|f| f == "enabled") {
        lines.push(if active {
            "  rc 를 끄는 설정은 데몬을 다시 띄워야 반영된다: rocky daemon restart".to_string()
        } else {
            "  데몬이 rc 없이 떠 있다. 다시 띄워야 대상이 반영된다: rocky daemon restart"
                .to_string()
        });
        return lines;
    }
    if !active {
        if edit.is_none() {
            lines.push(
                "= 데몬이 rc 를 쓰지 않는다. rocky.json 에 rc 블록이 없거나 rc.enabled 가 false 다"
                    .into(),
            );
        }
        return lines;
    }
    let applied = raw.get("applied").and_then(Value::as_bool) == Some(true);
    let supervise = raw.get("supervise").and_then(Value::as_bool) == Some(true);
    match edit {
        Some(edit) => lines.push(match edit.change {
            Change::Removed => "  데몬에 반영했다".to_string(),
            _ if edit.pinned && supervise => {
                "  데몬에 반영했다. 꺼져 있으면 감시가 띄운다(2분마다 잰다)".to_string()
            }
            _ => format!("  데몬에 반영했다. 띄우려면: rocky rc start {}", edit.label),
        }),
        None if applied => {
            lines.push("✓ 데몬이 대상 목록을 다시 읽었다".into());
            let changes: Vec<String> = [
                ("더함", "added"),
                ("뺌", "removed"),
                ("고정", "pinned"),
                ("고정 풂", "unpinned"),
            ]
            .iter()
            .filter_map(|(name, key)| {
                let list = labels(key);
                (!list.is_empty()).then(|| format!("{name}: {}", list.join(", ")))
            })
            .collect();
            if !changes.is_empty() {
                lines.push(format!("  {}", changes.join(" · ")));
            }
        }
        None => lines.push("= 대상 목록은 그대로다".into()),
    }
    let rest: Vec<&str> = restart.iter().map(String::as_str).collect();
    if !rest.is_empty() {
        lines.push(format!(
            "  데몬을 다시 띄워야 반영되는 설정이 바뀌었다({}): rocky daemon restart",
            rest.join(" · ")
        ));
    }
    lines
}

/// 다시 읽기 요청이 실패했을 때의 줄(targets add · rm) — 파일은 이미 고쳤다. 옛 데몬(라우트 없음)이면 재시작을 권한다.
pub fn reload_failure(err: &str) -> Vec<String> {
    if old_daemon(err) {
        return vec![format!("  {OLD_DAEMON}")];
    }
    vec![
        format!("  데몬에 반영하지 못했다: {err}"),
        "  다시 해 보려면: rocky rc reload".into(),
    ]
}

/// 다시 읽기를 모르는 옛 데몬의 404 — 데몬은 모르는 라우트를 `not found: <메서드> <경로>` 로 답한다.
fn old_daemon(err: &str) -> bool {
    err == format!("not found: POST {RELOAD}")
}

fn pad(s: &str, width: usize) -> String {
    format!("{s}{}", " ".repeat(width.saturating_sub(display_width(s))))
}

/// `rocky rc targets` 표 — 이름 · 고정 · 폴더, 메모(폴더 없음 · 이름 겹침). 끝에 설정 파일과 root.
pub fn render_list(list: &TargetList, path: &str, home: &str) -> String {
    let mut out = Vec::new();
    if list.configured && !list.enabled {
        out.push(
            "⚠ 이 기기에서는 rc 가 꺼져 있다(rc.enabled: false). 목록만 남아 있다".to_string(),
        );
    }
    if list.targets.is_empty() {
        out.push("rc 대상이 없다. 더하려면: rocky rc targets add <이름|경로>".into());
    } else {
        let head = ["이름", "고정"];
        let name_w = list
            .targets
            .iter()
            .map(|t| display_width(&t.label))
            .chain([display_width(head[0])])
            .max()
            .unwrap_or(0);
        let pin_w = display_width(head[1]);
        out.push(format!(
            "{}  {}  폴더",
            pad(head[0], name_w),
            pad(head[1], pin_w)
        ));
        for t in &list.targets {
            let mut memo = Vec::new();
            if !t.exists {
                memo.push("⚠ 폴더가 없다");
            }
            if t.ambiguous {
                memo.push("⚠ 이름이 겹쳐 이름으로 부를 수 없다");
            }
            let line = format!(
                "{}  {}  {}  {}",
                pad(&t.label, name_w),
                pad(if t.pinned { "고정" } else { "" }, pin_w),
                tilde(&t.dir, home),
                memo.join(" · ")
            );
            out.push(line.trim_end().to_string());
        }
    }
    out.push(String::new());
    out.push(format!(
        "설정 파일: {path} · 맨 이름은 root 아래: {}",
        tilde(&list.root, home)
    ));
    out.join("\n")
}
