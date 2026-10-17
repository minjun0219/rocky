//! `rocky rc targets` · `rocky rc reload` — rocky.json 의 rc 대상 목록(`pinned` · `targets`)을 고치는 판정과, 떠 있는
//! 데몬이 고친 목록을 다시 읽을 때 무엇을 바로 쓰고 무엇을 데몬 재시작으로 미루는지. 파일 읽기 · 쓰기는 CLI, 다시 읽기는
//! 데몬(`rockyd::rc::RcSettings` — 파일이 바뀌면 알아서, `rocky rc reload` 면 바로)이 한다.

use serde::Serialize;
use serde_json::{Map, Value};

use crate::config::{parse_rc_block, rc_enabled, RcConfig};
use crate::rc::{
    label_of, resolve_dir, resolve_entries, resolve_root, resolve_targets, Target, TargetEntry,
};

/// 대상 목록을 고친 결과.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Change {
    /// 새로 적었다.
    Added,
    /// 일반 대상을 고정으로 옮겼다.
    Pinned,
    /// 고정 대상을 일반 대상으로 옮겼다(`rocky rc targets unpin`).
    Unpinned,
    /// 이미 있어 파일을 고치지 않았다. 더하기는 고정을 풀지 않는다(풀려면 `unpin`).
    Unchanged,
    Removed,
}

/// `add_target` · `remove_target` · `unpin_target` 의 결과 — 무엇을 바꿨고 그 대상이 무엇인지.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Edit {
    pub change: Change,
    pub label: String,
    pub dir: String,
    /// rocky.json 에 적힌(적은) 이름.
    pub entry: String,
    pub pinned: bool,
    /// 이 기기에서 rc 가 꺼져 있다(`enabled: false`) — 목록만 고쳤다.
    pub disabled: bool,
}

/// 사람이 준 대상 → rocky.json 에 적을 이름. `root` 는 푼 기준 폴더(`resolve_root`).
///
/// - `/…` · `~` · `~/…` · 맨 이름(root 아래)은 설정 파일과 같은 뜻으로 받는다. `.` · `./…` · `../…` 는 지금 폴더(`cwd`) 기준이다.
/// - 적는 것은 가장 짧은 꼴이다 — root 아래면 맨 이름, 홈 아래면 `~/…`, 그 밖은 절대 경로. 셸이 `~` 를 먼저 풀어 절대 경로로
///   넘기므로, 그대로 적으면 손으로 쓴 목록과 꼴이 갈리고 홈이 다른 기기에서 맞지 않는다.
///
/// 어느 꼴이든 `.` · `..` · 겹친 `/` 는 글자로 풀어 적는다. 데몬은 서버를 cwd 문자열로 맞대므로(`resolve_dir` 는 정규화하지
/// 않는다) 그런 조각이 남은 이름은 떠 있는 서버와 영영 맞지 않는다.
pub fn entry_for(input: &str, cwd: &str, home: &str, root: &str) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("대상 이름이 비었다".into());
    }
    let relative =
        input == "." || input == ".." || input.starts_with("./") || input.starts_with("../");
    let dir = if relative {
        normalize(&format!("{}/{input}", cwd.trim_end_matches('/')))?
    } else {
        normalize(&resolve_dir(input, home, root))?
    };
    Ok(shortest(&dir, home, root))
}

/// 절대 경로의 `.` · `..` · 겹친 `/` 를 글자로만 푼다 — 링크는 따라가지 않는다(데몬의 맞대기도 글자다).
fn normalize(path: &str) -> Result<String, String> {
    if !path.starts_with('/') {
        return Err(format!("절대 경로로 풀리지 않는다: {path}"));
    }
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    Ok(format!("/{}", parts.join("/")))
}

/// `base` 아래의 상대 이름 — `base` 자신이거나 밖이면 None.
fn under(dir: &str, base: &str) -> Option<String> {
    let rest = dir
        .strip_prefix(base.trim_end_matches('/'))?
        .strip_prefix('/')?;
    (!rest.is_empty()).then(|| rest.to_string())
}

/// 홈이거나 홈 아래면 `~` · `~/…` 꼴.
fn home_form(dir: &str, home: &str) -> Option<String> {
    let home = home.trim_end_matches('/');
    if home.is_empty() {
        return None;
    }
    if dir == home {
        return Some("~".into());
    }
    under(dir, home).map(|rest| format!("~/{rest}"))
}

/// root 아래면 맨 이름, 홈 아래면 `~/…`, 그 밖은 절대 경로.
fn shortest(dir: &str, home: &str, root: &str) -> String {
    under(dir, root)
        .or_else(|| home_form(dir, home))
        .unwrap_or_else(|| dir.to_string())
}

/// `rocky rc targets add` — 대상 하나를 rocky.json 문서(`doc`, 파일 전체)에 더한다. `rc` 블록 · 목록이 없으면 만든다.
///
/// 이미 있는 폴더면(적은 꼴이 달라도 같은 폴더로 풀리면) 고치지 않는다 — `pin` 이면 일반 대상을 고정으로 옮기기만 한다. 라벨(폴더
/// 이름)이 같은 다른 폴더가 있으면 거절한다 — rc 는 라벨로 부르므로 둘 다 부를 수 없게 된다. 새로 적거나 옮기는 폴더는
/// 이 기기에 있어야 한다(`is_dir`) — 오타로 영영 뜨지 않는 대상을 만들지 않게.
pub fn add_target(
    doc: &mut Value,
    input: &str,
    pin: bool,
    cwd: &str,
    home: &str,
    is_dir: &dyn Fn(&str) -> bool,
) -> Result<Edit, String> {
    let block = rc_block_mut(doc)?;
    let config = parse_rc_block(block);
    let disabled = !rc_enabled(block);
    let root = resolve_root(&config, home);
    let entry = entry_for(input, cwd, home, &root)?;
    let dir = resolve_dir(&entry, home, &root);
    let label = label_of(&dir);
    if label.is_empty() {
        return Err(format!("rc 대상으로 쓸 수 없는 폴더다: {dir}"));
    }
    let edit = |change, entry: &str, pinned| Edit {
        change,
        label: label.clone(),
        dir: dir.clone(),
        entry: entry.to_string(),
        pinned,
        disabled,
    };
    let entries = resolve_entries(&config, home);
    let missing = || {
        let hint = if entry == input.trim() && !entry.starts_with(['/', '~']) {
            format!(". 맨 이름은 root 아래로 푼다: {root}")
        } else {
            String::new()
        };
        Err(format!("폴더가 없다: {dir}{hint}"))
    };
    if let Some(found) = entries.iter().find(|e| e.target.dir == dir) {
        if found.target.pinned || !pin {
            return Ok(edit(Change::Unchanged, &found.entry, found.target.pinned));
        }
        if !is_dir(&dir) {
            return missing();
        }
        remove_dir(block, "targets", &dir, home, &root);
        push(block, "pinned", &found.entry)?;
        return Ok(edit(Change::Pinned, &found.entry, true));
    }
    if let Some(clash) = entries.iter().find(|e| e.target.label == label) {
        return Err(format!(
            "이름이 같은 대상이 이미 있다: {label}({}). rc 는 폴더 이름으로 부르므로 둘을 함께 둘 수 없다",
            clash.target.dir
        ));
    }
    if !is_dir(&dir) {
        return missing();
    }
    push(block, if pin { "pinned" } else { "targets" }, &entry)?;
    Ok(edit(Change::Added, &entry, pin))
}

/// `rocky rc targets rm` — 라벨(폴더 이름)이나 경로로 대상을 골라 rocky.json 문서에서 뺀다. 그 폴더로 풀리는 이름은
/// `pinned` · `targets` 양쪽에서 모두 지운다. 라벨이 같은 대상이 둘 이상이면 거절한다(경로로 고르게).
pub fn remove_target(doc: &mut Value, key: &str, cwd: &str, home: &str) -> Result<Edit, String> {
    let Picked {
        block,
        disabled,
        root,
        found,
        ..
    } = pick_in(doc, key, cwd, home)?;
    remove_dir(block, "pinned", &found.target.dir, home, &root);
    remove_dir(block, "targets", &found.target.dir, home, &root);
    Ok(Edit {
        change: Change::Removed,
        label: found.target.label,
        dir: found.target.dir,
        entry: found.entry,
        pinned: found.target.pinned,
        disabled,
    })
}

/// `rocky rc targets unpin` — 고정 대상을 라벨(폴더 이름)이나 경로로 골라(`rm` 과 같은 규칙) 일반 대상으로 옮긴다. 적힌
/// 이름은 그대로 옮겨 적는다. 떠 있는 서버는 그대로이고, 감시가 고정으로 되살리지 않을 뿐이다. 고정 대상이 아니면
/// 거절한다 — 고칠 것이 없다.
pub fn unpin_target(doc: &mut Value, key: &str, cwd: &str, home: &str) -> Result<Edit, String> {
    let Picked {
        block,
        config,
        disabled,
        root,
        entries,
        found,
    } = pick_in(doc, key, cwd, home)?;
    if !found.target.pinned {
        let pinned: Vec<&str> = entries
            .iter()
            .filter(|e| e.target.pinned)
            .map(|e| e.target.label.as_str())
            .collect();
        let hint = if pinned.is_empty() {
            "고정 대상이 하나도 없다".to_string()
        } else {
            format!("고정 대상: {}", pinned.join(", "))
        };
        return Err(format!(
            "고정 대상이 아니다: {}. {hint}",
            found.target.label
        ));
    }
    let dir = found.target.dir;
    remove_dir(block, "pinned", &dir, home, &root);
    // 손으로 고치다 일반 목록에도 같은 폴더가 들어가 있으면 하나 더 적지 않는다.
    if !config
        .targets
        .iter()
        .any(|name| resolve_dir(name, home, &root) == dir)
    {
        push(block, "targets", &found.entry)?;
    }
    Ok(Edit {
        change: Change::Unpinned,
        label: found.target.label,
        dir,
        entry: found.entry,
        pinned: false,
        disabled,
    })
}

/// `rm` · `unpin` 이 고를 자리 — 문서의 rc 블록과 그 설정, 꺼짐 여부, root, 풀린 대상들, 고른 대상.
struct Picked<'a> {
    block: &'a mut Map<String, Value>,
    config: RcConfig,
    disabled: bool,
    root: String,
    entries: Vec<TargetEntry>,
    found: TargetEntry,
}

/// rocky.json 문서에서 대상 하나를 고른다(`rm` · `unpin`). rc 블록이 없으면 거절한다.
fn pick_in<'a>(doc: &'a mut Value, key: &str, cwd: &str, home: &str) -> Result<Picked<'a>, String> {
    let key = key.trim();
    let Some(block) = doc.get_mut("rc").and_then(Value::as_object_mut) else {
        return Err(format!(
            "rc 대상이 아니다: {key}. rocky.json에 rc 블록이 없다"
        ));
    };
    let config = parse_rc_block(block);
    let disabled = !rc_enabled(block);
    let root = resolve_root(&config, home);
    let entries = resolve_entries(&config, home);
    let found = pick_target(&entries, key, cwd, home, &root)?;
    Ok(Picked {
        block,
        config,
        disabled,
        root,
        entries,
        found,
    })
}

/// 라벨(폴더 이름)이나 경로로 대상 하나를 고른다. 경로가 든 열쇠는 라벨로 맞대지 않는다. 라벨이 같은 대상이 둘 이상이면
/// 거절한다(경로로 고르게).
fn pick_target(
    entries: &[TargetEntry],
    key: &str,
    cwd: &str,
    home: &str,
    root: &str,
) -> Result<TargetEntry, String> {
    let by_dir = entry_for(key, cwd, home, root)
        .ok()
        .map(|e| resolve_dir(&e, home, root));
    let matched: Vec<_> = entries
        .iter()
        .filter(|e| {
            by_dir.as_deref() == Some(e.target.dir.as_str())
                || (!key.contains('/') && e.target.label == key)
        })
        .collect();
    match matched.as_slice() {
        [one] => Ok((*one).clone()),
        [] if entries.is_empty() => Err(format!("rc 대상이 아니다: {key}. 대상이 하나도 없다")),
        [] => {
            let labels: Vec<&str> = entries.iter().map(|e| e.target.label.as_str()).collect();
            Err(format!(
                "rc 대상이 아니다: {key}. 지금 대상: {}",
                labels.join(", ")
            ))
        }
        many => {
            let dirs: Vec<&str> = many.iter().map(|e| e.target.dir.as_str()).collect();
            Err(format!(
                "이름이 같은 대상이 둘 이상이다: {key}. 경로로 고른다: {}",
                dirs.join(", ")
            ))
        }
    }
}

/// 문서의 `rc` 블록 — 없으면 만든다(파일 끝에 붙는다).
fn rc_block_mut(doc: &mut Value) -> Result<&mut Map<String, Value>, String> {
    doc.as_object_mut()
        .ok_or_else(|| "rocky.json의 맨 위가 객체가 아니다".to_string())?
        .entry("rc")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| "rocky.json의 rc가 객체가 아니다. 손으로 고친 뒤 다시 한다".to_string())
}

fn push(block: &mut Map<String, Value>, key: &str, entry: &str) -> Result<(), String> {
    match block.entry(key).or_insert_with(|| Value::Array(Vec::new())) {
        Value::Array(items) => {
            items.push(Value::String(entry.to_string()));
            Ok(())
        }
        _ => Err(format!(
            "rocky.json의 rc.{key}가 배열이 아니다. 손으로 고친 뒤 다시 한다"
        )),
    }
}

/// `key` 목록에서 `dir` 로 풀리는 이름을 모두 지운다. 배열이 아니면 그대로 둔다(설정을 읽을 때도 빈 목록으로 읽었다).
fn remove_dir(block: &mut Map<String, Value>, key: &str, dir: &str, home: &str, root: &str) {
    if let Some(Value::Array(items)) = block.get_mut(key) {
        items.retain(|v| {
            v.as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .is_none_or(|name| resolve_dir(name, home, root) != dir)
        });
    }
}

/// `rocky rc targets` 의 한 줄.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetLine {
    pub label: String,
    pub dir: String,
    /// rocky.json 에 적힌 이름.
    pub entry: String,
    pub pinned: bool,
    /// 폴더가 이 기기에 있나.
    pub exists: bool,
    /// 라벨이 같은 대상이 또 있다 — 이름으로 부르면 거절된다.
    pub ambiguous: bool,
}

/// `rocky rc targets` — 설정 파일에 적힌 대상 목록(데몬이 쓰는 것과 같은 순서 · 규칙).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetList {
    /// `rc` 블록이 있나.
    pub configured: bool,
    /// 이 기기에서 켜져 있나 — 블록이 있고 `enabled: false` 가 아니다.
    pub enabled: bool,
    /// 상대 이름의 기준 폴더(푼 것).
    pub root: String,
    pub targets: Vec<TargetLine>,
}

pub fn list_targets(doc: &Value, home: &str, is_dir: &dyn Fn(&str) -> bool) -> TargetList {
    let block = doc.get("rc").and_then(Value::as_object);
    let config = block.map(parse_rc_block).unwrap_or_default();
    let entries = resolve_entries(&config, home);
    let targets = entries
        .iter()
        .map(|e| TargetLine {
            label: e.target.label.clone(),
            dir: e.target.dir.clone(),
            entry: e.entry.clone(),
            pinned: e.target.pinned,
            exists: is_dir(&e.target.dir),
            ambiguous: entries
                .iter()
                .filter(|o| o.target.label == e.target.label)
                .count()
                > 1,
        })
        .collect();
    TargetList {
        configured: block.is_some(),
        enabled: block.is_some_and(rc_enabled),
        root: resolve_root(&config, home),
        targets,
    }
}

/// 설정 파일이 없을 때 고쳐 쓸 시작 문서 — `rocky config init` 이 만드는 것과 같다.
pub fn new_config() -> Value {
    serde_json::from_str(&crate::setup::default_config_json()).expect("기본 설정은 JSON 이다")
}

/// 고친 문서를 원래 파일의 들여쓰기 단위(처음 들여쓴 줄의 공백 · 탭)로 다시 쓴다. 키 순서는 원래대로(`preserve_order`),
/// 끝은 줄바꿈 하나. 원래 파일이 없거나 들여쓰기를 못 찾으면 두 칸이다. 한 줄로 적은 배열은 여러 줄로 펴진다.
pub fn render_config(doc: &Value, original: Option<&str>) -> String {
    let indent = original.and_then(indent_unit).unwrap_or("  ");
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
    let mut ser = serde_json::Serializer::with_formatter(&mut out, formatter);
    doc.serialize(&mut ser)
        .expect("Value 의 직렬화는 실패하지 않는다");
    let mut text = String::from_utf8(out).expect("serde_json 은 UTF-8 만 쓴다");
    text.push('\n');
    text
}

/// 처음 들여쓴 줄의 앞 공백 — 공백만이거나 탭만일 때.
fn indent_unit(text: &str) -> Option<&str> {
    text.lines().find_map(|line| {
        let body = line.trim_start_matches([' ', '\t']);
        let ws = &line[..line.len() - body.len()];
        let uniform = ws.bytes().all(|b| b == b' ') || ws.bytes().all(|b| b == b'\t');
        (!ws.is_empty() && !body.is_empty() && uniform).then_some(ws)
    })
}

/// 다시 읽기(`rocky rc reload` · 파일이 바뀌었을 때)의 보고 — 데몬이 대상 목록을 바꿨나, 무엇이 바뀌었나(라벨), 데몬을 다시
/// 띄워야 반영되는 칸은 무엇인가.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reload {
    /// 데몬이 rc 를 쓰고 있나 — 기동 때 rc 블록이 켜져 있었나. 아니면 대상 목록을 고쳐도 데몬을 다시 띄워야 본다.
    pub active: bool,
    /// 데몬의 감시가 켜져 있나(기동 때 값) — 켜져 있으면 꺼진 고정 대상을 감시가 띄운다.
    pub supervise: bool,
    pub applied: bool,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub pinned: Vec<String>,
    pub unpinned: Vec<String>,
    /// 파일과 다르지만 데몬을 다시 띄워야 반영되는 칸 — `enabled` · `supervise` · `nightly`. 기동기와 감시 · 야간 루프는
    /// 기동 때 정해진다.
    pub restart_needed: Vec<&'static str>,
}

impl Reload {
    /// 바뀐 대상 목록을 사람이 읽을 꼴로(`더함: a, b`) — 빈 칸은 뺀다. `rocky rc reload` 의 보고와 데몬이 알아서 다시 읽었을
    /// 때의 로그가 같이 쓴다.
    pub fn changes(&self) -> Vec<String> {
        [
            ("더함", &self.added),
            ("뺌", &self.removed),
            ("고정", &self.pinned),
            ("고정 풂", &self.unpinned),
        ]
        .iter()
        .filter(|(_, labels)| !labels.is_empty())
        .map(|(name, labels)| format!("{name}: {}", labels.join(", ")))
        .collect()
    }
}

/// 데몬이 바뀐 파일을 알아서 다시 읽었을 때의 로그 한 줄 — 대상 목록을 바꿨거나, 데몬을 다시 띄워야 반영되는 칸이 기동 때와
/// 다를 때(그동안은 파일을 저장할 때마다 다시 알린다). 둘 다 아니면(다른 블록만 고쳤으면) None.
pub fn auto_reload_line(report: &Reload) -> Option<String> {
    let mut parts = Vec::new();
    if report.applied {
        let changes = report.changes();
        parts.push(if changes.is_empty() {
            "바뀐 rocky.json 의 rc 대상 목록을 다시 읽었다".to_string()
        } else {
            format!(
                "바뀐 rocky.json 의 rc 대상 목록을 다시 읽었다({})",
                changes.join(" · ")
            )
        });
    }
    if !report.restart_needed.is_empty() {
        parts.push(format!(
            "rc 의 {} 는 데몬을 다시 띄워야 반영된다",
            report.restart_needed.join(" · ")
        ));
    }
    (!parts.is_empty()).then(|| parts.join(". "))
}

/// 지금 쓰는 설정(`current` — 기동 때 rc 가 꺼져 있었으면 None)에 파일에서 다시 읽은 블록(`file` — 없거나 꺼졌으면 None)을
/// 얹는다. 대상 목록(`root` · `pinned` · `targets`)만 바로 바꾸고 기능 스위치는 그대로 둔다. 바꿀 설정이 있으면 첫째 값이 Some.
pub fn plan_reload(
    current: Option<&RcConfig>,
    file: Option<&RcConfig>,
    home: &str,
) -> (Option<RcConfig>, Reload) {
    let (Some(current), Some(file)) = (current, file) else {
        let restart_needed = if current.is_some() == file.is_some() {
            Vec::new()
        } else {
            vec!["enabled"]
        };
        return (
            None,
            Reload {
                active: current.is_some(),
                supervise: current.is_some_and(|c| c.supervise),
                restart_needed,
                ..Reload::default()
            },
        );
    };
    let next = RcConfig {
        root: file.root.clone(),
        pinned: file.pinned.clone(),
        targets: file.targets.clone(),
        ..current.clone()
    };
    let before = resolve_targets(current, home);
    let after = resolve_targets(&next, home);
    let mut report = Reload {
        active: true,
        supervise: current.supervise,
        ..Reload::default()
    };
    for t in &after {
        match by_dir(&before, &t.dir) {
            None => report.added.push(t.label.clone()),
            Some(old) if !old.pinned && t.pinned => report.pinned.push(t.label.clone()),
            Some(old) if old.pinned && !t.pinned => report.unpinned.push(t.label.clone()),
            Some(_) => {}
        }
    }
    report.removed = before
        .iter()
        .filter(|t| by_dir(&after, &t.dir).is_none())
        .map(|t| t.label.clone())
        .collect();
    if current.supervise != file.supervise {
        report.restart_needed.push("supervise");
    }
    if current.nightly != file.nightly {
        report.restart_needed.push("nightly");
    }
    report.applied = next != *current;
    (report.applied.then_some(next), report)
}

fn by_dir<'a>(list: &'a [Target], dir: &str) -> Option<&'a Target> {
    list.iter().find(|t| t.dir == dir)
}
