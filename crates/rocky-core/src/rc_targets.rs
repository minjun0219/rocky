//! `rocky.json` 의 `rc` 대상 목록(`targets` · `pinned`)을 고친다 — 순수 판정. 파일 읽기·쓰기와 폴더가 있는지는
//! CLI(`rocky rc add|remove|pin|unpin`)가 본다. 데몬은 파일의 mtime 이 바뀌면 목록을 다시 읽는다(`rockyd::rc::RcConfigLive`).
//!
//! 이름은 설정에 적는 그대로(`notes` · `cabinet/notes` · `~/x` · `/abs`)이거나 `rocky rc` 표의 라벨이다. 같은 폴더를
//! 가리키는 칸은 하나로 본다(`resolve_targets` 와 같은 규칙).

use serde_json::{Map, Value};

use crate::rc::{label_of, resolve_dir, DEFAULT_ROOT};

/// 대상 목록 고치기 한 번.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RcEdit {
    /// 대상에 넣는다. `pin` 이면 고정으로.
    Add { pin: bool },
    /// 대상에서 뺀다(고정이면 고정에서도). 떠 있는 서버는 내리지 않는다 — 대상 밖 서버로 남는다.
    Remove,
    /// 고정한다(꺼지면 감시가 되살린다). 대상이 아니면 대상으로도 넣는다.
    Pin,
    /// 고정을 푼다. 대상으로는 남는다.
    Unpin,
}

/// 고친 결과 — 새 설정 문자열(끝에 줄바꿈), 사람에게 보일 한 줄, 그 칸이 가리키는 폴더.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RcEditOutcome {
    pub json: String,
    pub message: String,
    pub dir: String,
}

/// 설정 문자열에 `edit` 을 적용한다. `rc` 블록이 없거나 JSON 이 깨졌으면, 또는 바꿀 것이 없으면 사유와 함께 Err.
/// `rc` 블록 밖의 키와 순서는 그대로 둔다(`serde_json` 의 `preserve_order`).
pub fn edit_rc_targets(
    raw: &str,
    home: &str,
    entry: &str,
    edit: RcEdit,
) -> Result<RcEditOutcome, String> {
    let entry = entry.trim().trim_end_matches('/');
    if entry.is_empty() {
        return Err("이름이 비었다".into());
    }
    let mut doc: Value =
        serde_json::from_str(raw).map_err(|e| format!("rocky.json 을 읽지 못했다 — {e}"))?;
    let block = doc
        .get_mut("rc")
        .and_then(Value::as_object_mut)
        .ok_or("rocky.json 에 rc 블록이 없다 — rc 를 켜는 것부터(/rocky:config)")?;
    let root_raw = block
        .get("root")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_ROOT)
        .to_string();
    let root = resolve_dir(&root_raw, home, home);
    let pinned = names(block, "pinned");
    let targets = names(block, "targets");

    // 칸을 찾는다 — 같은 폴더를 가리키는 칸, 없으면 라벨이 같은 칸(둘 이상의 폴더면 고르지 않는다).
    let wanted = resolve_dir(entry, home, &root);
    let dir_of = |n: &str| resolve_dir(n.trim(), home, &root);
    let mut hits: Vec<String> = pinned
        .iter()
        .chain(&targets)
        .map(|n| dir_of(n))
        .filter(|d| *d == wanted)
        .collect();
    if hits.is_empty() && !entry.contains('/') && !entry.starts_with('~') {
        hits = pinned
            .iter()
            .chain(&targets)
            .map(|n| dir_of(n))
            .filter(|d| label_of(d) == entry)
            .collect();
    }
    hits.sort();
    hits.dedup();
    if hits.len() > 1 {
        return Err(format!(
            "{entry} 라벨의 대상이 둘 이상이다({}) — 경로로 고른다",
            hits.join(" · ")
        ));
    }
    let dir = hits.pop().unwrap_or(wanted);
    let is_pinned = pinned.iter().any(|n| dir_of(n) == dir);
    let is_target = targets.iter().any(|n| dir_of(n) == dir);
    let label = label_of(&dir);
    // 적는 칸 — 이미 있는 칸이면 그 문자열 그대로(라벨로 찾았을 때 라벨을 적으면 다른 폴더로 풀린다), 새 칸이면
    // root 아래는 상대 이름, 아니면 받은 그대로.
    let existing = pinned
        .iter()
        .chain(&targets)
        .find(|n| dir_of(n) == dir)
        .map(|n| n.trim().to_string());
    let stored = existing.unwrap_or_else(|| new_entry(&dir, &root, entry));

    let without = |list: &[String]| -> Vec<String> {
        list.iter().filter(|n| dir_of(n) != dir).cloned().collect()
    };
    let with = |list: &[String]| -> Vec<String> {
        let mut out = list.to_vec();
        out.push(stored.clone());
        out
    };
    let (new_pinned, new_targets, message) = match edit {
        RcEdit::Add { pin } => {
            if is_pinned || (is_target && !pin) {
                return Err(format!("{label} 은(는) 이미 대상이다 — {dir}"));
            }
            if pin {
                (
                    with(&pinned),
                    targets.clone(),
                    format!("{label}: 고정 대상으로 넣었다 — {dir}"),
                )
            } else {
                (
                    pinned.clone(),
                    with(&targets),
                    format!("{label}: 대상으로 넣었다 — {dir}"),
                )
            }
        }
        RcEdit::Remove => {
            if !is_pinned && !is_target {
                return Err(format!("{entry} 은(는) 대상이 아니다"));
            }
            (
                without(&pinned),
                without(&targets),
                format!("{label}: 대상에서 뺐다 — 떠 있으면 대상 밖 서버로 남는다(닫기는 rocky rc stop {label})"),
            )
        }
        RcEdit::Pin => {
            if is_pinned {
                return Err(format!("{label} 은(는) 이미 고정이다"));
            }
            (
                with(&pinned),
                targets.clone(),
                format!("{label}: 고정했다 — 꺼지면 감시가 되살린다"),
            )
        }
        RcEdit::Unpin => {
            if !is_pinned {
                return Err(format!("{entry} 은(는) 고정이 아니다"));
            }
            // 고정에서만 뺀다 — 대상 목록에 없었으면 대상으로 옮겨 부를 수 있게 남긴다.
            let targets = if is_target {
                targets.clone()
            } else {
                with(&targets)
            };
            (
                without(&pinned),
                targets,
                format!("{label}: 고정을 풀었다 — 대상으로는 남는다"),
            )
        }
    };
    block.insert("pinned".into(), to_array(new_pinned));
    block.insert("targets".into(), to_array(new_targets));
    let mut json = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
    json.push('\n');
    Ok(RcEditOutcome { json, message, dir })
}

/// 새 칸의 문자열 — root 아래면 상대 이름, 아니면 받은 그대로.
fn new_entry(dir: &str, root: &str, entry: &str) -> String {
    dir.strip_prefix(&format!("{}/", root.trim_end_matches('/')))
        .map(str::to_string)
        .unwrap_or_else(|| entry.to_string())
}

fn names(block: &Map<String, Value>, key: &str) -> Vec<String> {
    block
        .get(key)
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn to_array(list: Vec<String>) -> Value {
    Value::Array(list.into_iter().map(Value::String).collect())
}
