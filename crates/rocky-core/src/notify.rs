//! UserPromptSubmit 훅의 순수 로직 — TS 원본 `src/notify.ts`.
//!
//! "마지막 확인 이후 호출자(사람)가 보드에서 무엇을 바꿨나"를 컴팩트한 한국어
//! 컨텍스트로 만든다. 훅 엔트리(CLI 의 `hook notify-todo`)는 데몬 HTTP 호출 +
//! stdin/stdout 배선만 담당한다.
//!
//! 커서는 세션별 — `<dir>/hook-cursors.json` 에 `{ sessionId: { lastId, at } }` 로
//! 저장하고 최근 100 세션만 유지한다 (무한 성장 방지).

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::actors::is_agent_actor;
use crate::peer_inbox::Delivery;
use crate::prwatch::PrSubscription;
use crate::types::{ChangeFeedEntry, ChangesSince, HistoryEntity};

/// 사람이 낸 변경만 남긴다 (에이전트 자신의 변경을 주입하는 자기 반향 방지).
///
/// handoff 계열 액션은 여기까지 오지 않는다 — `list_changes_since` 가 쿼리에서 이미
/// 뺀다. `handoff-delivered` 의 actor 는 **대상 세션 이름**(`eelpout-a3`)이라 이름만
/// 보면 사람으로 분류될 값인데, 그 필터 덕에 여기서 한 번 더 막을 필요가 없다.
pub fn filter_human_changes(entries: Vec<ChangeFeedEntry>) -> Vec<ChangeFeedEntry> {
    entries
        .into_iter()
        .filter(|e| !is_agent_actor(&e.history.actor))
        .collect()
}

fn action_label(action: &str) -> &str {
    match action {
        "create" => "생성",
        "update" => "수정",
        "start" => "시작",
        "stop" => "중단",
        "done" => "완료",
        "reopen" => "다시 열기",
        "archive" => "보관",
        "unarchive" => "보관 해제",
        "pin" => "고정",
        "unpin" => "고정 해제",
        "comment-archive" => "댓글 보관",
        "comment-unarchive" => "댓글 보관 해제",
        other => other,
    }
}

/// 본문을 실어 보여주는 액션 — 나머지는 `field: old → new` 렌더를 탄다.
///
/// `DETAIL_HISTORY_EXCLUDED` 와 값이 우연히 같지만 여기는 별개의 결정("본문을 한 줄로
/// 인라인 렌더할까")을 인코딩한다 — 저쪽은 "상세 화면에서 뺄까"다. 커플링하지 않는다.
fn is_comment_action(action: &str) -> bool {
    action == "comment" || action == "comment-edit"
}

/// 주입 컨텍스트가 길어지지 않게 본문 길이를 제한한다.
const COMMENT_MAX_CHARS: usize = 200;

/// 댓글 본문을 한 줄로 접고 길면 자른다.
///
/// 길이는 JS 원본(`String#slice`)과 같은 **UTF-16 코드유닛** 기준이다 — char 로 세면
/// 서로게이트 페어(이모지 등)가 섞인 본문에서 자르는 위치가 달라진다. 딱 하나 다른
/// 점: JS 는 페어 한가운데를 갈라 깨진 서로게이트를 남길 수 있는데 Rust 문자열은
/// 그걸 표현할 수 없어, 경계에 걸린 문자는 통째로 앞에서 끊는다(최대 한 글자 차이).
fn condense_body(body: &str) -> String {
    let one_line = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut units = 0usize;
    let mut head = String::new();
    for ch in one_line.chars() {
        units += ch.len_utf16();
        if units > COMMENT_MAX_CHARS {
            head.push('…');
            return head;
        }
        head.push(ch);
    }
    one_line
}

/// JS `String(value)` 대응 — 문자열은 따옴표 없이, 나머지는 JSON 표기로.
fn js_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

fn format_line(entry: &ChangeFeedEntry) -> String {
    let board = entry
        .board_key
        .as_deref()
        .map(|k| format!("[{k}] "))
        .unwrap_or_default();
    let short_id: String = entry.history.entity_id.chars().take(6).collect();

    if is_comment_action(&entry.history.action) {
        // 댓글은 문장이라 `field: old → new` 렌더가 맞지 않는다 — 본문을 그대로 보여준다.
        let body = entry
            .history
            .changes
            .as_ref()
            .and_then(|c| c.get("comment"))
            .and_then(|pair| pair.get(1))
            .and_then(|v| v.as_str())
            .map(condense_body)
            .unwrap_or_default();
        let label = if entry.history.action == "comment" {
            "댓글"
        } else {
            "댓글 수정"
        };
        return format!(
            "- {}: {board}\"{}\" {label} · \"{body}\" · {short_id}",
            entry.history.actor, entry.title
        );
    }

    let kind = match entry.history.entity {
        HistoryEntity::Note => "메모 ",
        HistoryEntity::Todo => "",
        HistoryEntity::Board => "board ",
        HistoryEntity::Section => "section ",
    };
    let action = action_label(&entry.history.action);
    let (diff, has_content) = match entry.history.changes.as_ref() {
        Some(changes) => {
            let rendered = changes
                .iter()
                .filter(|(field, _)| field.as_str() != "content") // 메모 본문 diff 는 장황 — 필드명만
                .map(|(field, pair)| {
                    let old = pair.get(0).map(js_string).unwrap_or_default();
                    let new = pair.get(1).map(js_string).unwrap_or_default();
                    format!("{field}: {old} → {new}")
                })
                .take(3)
                .collect::<Vec<_>>()
                .join(", ");
            (rendered, changes.contains_key("content"))
        }
        None => (String::new(), false),
    };
    let diff_part = if !diff.is_empty() {
        format!(" ({diff})")
    } else if has_content {
        " (내용 편집)".to_string()
    } else {
        String::new()
    };
    format!(
        "- {}: {board}{kind}\"{}\" {action}{diff_part} · {short_id}",
        entry.history.actor, entry.title
    )
}

/// 주입할 컨텍스트 본문. 항목이 없으면 `None` (아무 것도 주입하지 않음).
/// 에이전트가 후속 조치를 스스로 판단하도록 안내 한 줄을 붙인다.
pub fn build_notify_context(entries: &[ChangeFeedEntry]) -> Option<String> {
    if entries.is_empty() {
        return None;
    }
    let mut lines = vec![
        "# rocky: 마지막 확인 이후 호출자의 보드 변경".to_string(),
        String::new(),
    ];
    lines.extend(entries.iter().map(format_line));
    lines.push(String::new());
    lines.push(
        "(자동 주입 — 필요하면 todo_list / note_list 로 상세를 확인하고, 지시로 해석되는 항목은 사용자에게 확인 후 진행)"
            .to_string(),
    );
    Some(lines.join("\n"))
}

/// PR 감시 전이 한 건 — 훅 주입(`build_pr_context`)과 채널(`pr_channel_events`)이 같은 추출을 쓴다.
struct PrTransition<'a> {
    /// `ready` / `conflict`.
    kind: &'a str,
    label: &'a str,
    repo: &'a str,
    number: Option<i64>,
    title: &'a str,
    url: &'a str,
}

/// **사람이 움직일 것만** (ready·conflict). merged/closed 는 히스토리에만 남긴다(문서의 약속;
/// 세션에 넣으면 지시로 오독된다).
fn pr_transition(e: &ChangeFeedEntry) -> Option<PrTransition<'_>> {
    let (kind, label) = match e.history.action.as_str() {
        "pr-ready" => ("ready", "머지 후보"),
        "pr-conflict" => ("conflict", "충돌 — 풀어야 한다"),
        _ => return None,
    };
    let changes = e.history.changes.as_ref();
    // 보드의 PR 작성자 필터에 걸린 전이 — 기록은 있지만 세션을 깨우지 않는다(훅 주입·채널 공통).
    if changes
        .and_then(|c| c.get("quiet"))
        .and_then(|v| v.as_bool())
        == Some(true)
    {
        return None;
    }
    let str_of = |key: &str| {
        changes
            .and_then(|c| c.get(key))
            .and_then(|v| v.as_str())
            .unwrap_or("")
    };
    Some(PrTransition {
        kind,
        label,
        repo: str_of("repo"),
        number: changes
            .and_then(|c| c.get("number"))
            .and_then(|v| v.as_i64()),
        title: str_of("title"),
        url: str_of("url"),
    })
}

/// PR 감시 전이(actor `rocky`, action `pr-*`)를 세션에 알리는 블록 — ready·conflict 만.
/// 데몬이 이미 판정했으므로 에이전트는 감시하지 않아도 된다는 뜻을 마지막 줄에 적는다.
pub fn build_pr_context(entries: &[ChangeFeedEntry]) -> Option<String> {
    let current = latest_pr_entries(entries);
    let lines: Vec<String> = current
        .iter()
        .filter_map(|e| pr_transition(e))
        .map(|t| {
            let number = t.number.map(|n| format!("#{n}")).unwrap_or_default();
            format!(
                "- {} {number} {} — {} ({})",
                t.repo, t.label, t.title, t.url
            )
        })
        .collect();
    if lines.is_empty() {
        return None;
    }
    let mut out = vec![
        "# rocky: PR 상태 변화 (데몬 감시)".to_string(),
        String::new(),
    ];
    out.extend(lines);
    out.push(String::new());
    out.push(
        "(자동 주입 — 데몬이 CI·리뷰 스레드로 기계적으로 고른 것이다. 감시를 따로 돌리지 말고, 머지 후보는 `/rocky:review-fix` 8단계대로 판단해 알린다 — 머지는 사용자 몫)"
            .to_string(),
    );
    Some(out.join("\n"))
}

/// 변경 피드를 한 페이지 읽은 뒤의 다음 cursor — `(cursor, 더 있음)`. 페이지가 꽉 찼으면(`limit` 건)
/// 응답의 `last_id`(전역 MAX) 가 아니라 **받은 마지막 항목의 id** 까지만 전진한다 — 안 그러면 그
/// 사이 행을 영영 건너뛴다(밀린 사이 100건 넘게 쌓인 경우). 비었으면 `last_id` 로 맞춘다.
pub fn page_cursor(feed: &ChangesSince, limit: usize) -> (i64, bool) {
    if limit > 0 && feed.entries.len() >= limit {
        let last = feed
            .entries
            .iter()
            .map(|e| e.history.id)
            .max()
            .unwrap_or(feed.last_id);
        (last, last < feed.last_id)
    } else {
        (feed.last_id, false)
    }
}

/// Claude Code 채널(`notifications/claude/channel`)로 밀어 넣을 전이 한 건 — `content` 가
/// `<channel>` 태그의 본문, `meta` 의 각 항목이 태그 속성이 된다(키는 식별자만 — 하이픈이 있으면
/// Claude Code 가 조용히 버린다).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrChannelEvent {
    pub content: String,
    pub meta: BTreeMap<String, String>,
}

/// 변경 피드에서 채널로 보낼 것만(ready·conflict) — 훅 주입과 같은 규칙, 모양만 채널용.
pub fn pr_channel_events(entries: &[ChangeFeedEntry]) -> Vec<PrChannelEvent> {
    latest_pr_entries(entries)
        .into_iter()
        .filter_map(pr_transition)
        .map(|t| {
            let number = t.number.map(|n| format!("#{n}")).unwrap_or_default();
            let mut meta = BTreeMap::new();
            meta.insert("kind".to_string(), t.kind.to_string());
            meta.insert("repo".to_string(), t.repo.to_string());
            if let Some(n) = t.number {
                meta.insert("number".to_string(), n.to_string());
            }
            meta.insert("url".to_string(), t.url.to_string());
            PrChannelEvent {
                content: format!("{} {number} {} — {}\n{}", t.repo, t.label, t.title, t.url),
                meta,
            }
        })
        .collect()
}

/// PR 마다 **마지막 전이만** — 피드는 커서 이후를 한꺼번에 주므로(세션이 한동안 조용했으면) 같은 PR 의
/// "머지 가능" 뒤에 "머지됨" 이 같이 온다. 옛 "머지 가능" 을 그대로 알리면 이미 머지된 PR 을 머지하라고
/// 한다(2026-09-29 #208). pr-* 가 아닌 항목은 버린다. 순서는 피드 순서(오래된 것부터).
pub fn latest_pr_entries(entries: &[ChangeFeedEntry]) -> Vec<&ChangeFeedEntry> {
    let key = |e: &ChangeFeedEntry| {
        let changes = e.history.changes.as_ref();
        (
            changes
                .and_then(|c| c.get("repo"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            changes
                .and_then(|c| c.get("number"))
                .and_then(|v| v.as_i64())
                .unwrap_or_default(),
        )
    };
    let prs: Vec<&ChangeFeedEntry> = entries
        .iter()
        .filter(|e| e.history.action.starts_with("pr-"))
        .collect();
    prs.iter()
        .enumerate()
        .filter(|(i, e)| {
            let k = key(e);
            !prs[i + 1..].iter().any(|later| key(later) == k)
        })
        .map(|(_, e)| *e)
        .collect()
}

/// 이 세션의 보드에 남은 PR 전이만 — 훅 주입은 세션마다 돌므로, 거르지 않으면 모든 세션이 모든 레포의
/// "머지 가능" 을 받는다(tally 세션에 rocky PR 알림이 간 사고, 2026-09-29). 세션 cwd 가 어느 보드로도
/// 안 풀리면(`None`) PR 전이는 싣지 않는다 — 모르는 세션에 남의 PR 을 알리느니 조용한 편이 낫다.
pub fn pr_entries_for_board(
    entries: &[ChangeFeedEntry],
    board_key: Option<&str>,
) -> Vec<ChangeFeedEntry> {
    let Some(board_key) = board_key else {
        return Vec::new();
    };
    entries
        .iter()
        .filter(|e| e.history.action.starts_with("pr-"))
        .filter(|e| e.board_key.as_deref() == Some(board_key))
        .cloned()
        .collect()
}

/// PR 전이의 `(repo, number)` — 구독·전달 기록과 대조하는 열쇠.
fn pr_key(e: &ChangeFeedEntry) -> (&str, Option<i64>) {
    let changes = e.history.changes.as_ref();
    (
        changes
            .and_then(|c| c.get("repo"))
            .and_then(|v| v.as_str())
            .unwrap_or(""),
        changes
            .and_then(|c| c.get("number"))
            .and_then(|v| v.as_i64()),
    )
}

/// **이 세션이 구독한 PR** 의 전이만 — 보드 기준만으로 거르면 같은 보드의 모든 세션이 남의 PR 머지 후보·충돌을
/// 받았다(2026-10-05). 받은편지함 알림(`rockyd::prwatch::session_notifier`)과 같은 기준이다. 레포는 대소문자를
/// 무시한다(구독 저장소와 같다). pr-* 가 아닌 항목은 버린다.
pub fn subscribed_pr_entries(
    entries: &[ChangeFeedEntry],
    session_id: &str,
    subscriptions: &[PrSubscription],
) -> Vec<ChangeFeedEntry> {
    entries
        .iter()
        .filter(|e| e.history.action.starts_with("pr-"))
        .filter(|e| {
            let (repo, number) = pr_key(e);
            subscriptions.iter().any(|s| {
                s.session_id.as_deref() == Some(session_id)
                    && s.repo.eq_ignore_ascii_case(repo)
                    && Some(s.number) == number
            })
        })
        .cloned()
        .collect()
}

/// 이 세션이 받을 PR 전이 — 세션 보드가 풀리면 그 보드의 것(보드별 `prAuthors` 판정을 따른다) 중, 안 풀리면
/// (보드 경로 밖 워크트리 등) 전 보드 중 **이 세션이 구독한 PR** 만. 구독이 이미 "이 세션의 PR" 이라 보드를 모르는
/// 세션도 받는다 — 받은편지함도 보드를 보지 않고 구독한 세션에 보낸다.
pub fn pr_entries_for_session(
    entries: &[ChangeFeedEntry],
    board_key: Option<&str>,
    session_id: &str,
    subscriptions: &[PrSubscription],
) -> Vec<ChangeFeedEntry> {
    match board_key {
        Some(key) => subscribed_pr_entries(
            &pr_entries_for_board(entries, Some(key)),
            session_id,
            subscriptions,
        ),
        None => subscribed_pr_entries(entries, session_id, subscriptions),
    }
}

/// 받은편지함으로 이미 간 전이를 뺀다 — 구독한 세션은 데몬이 소켓으로 깨운 뒤 다음 턴 훅이 같은 사실을 또
/// 넣어 두 번 받았다. PR 마다 **마지막 전이**를 먼저 고른 뒤 뺀다(먼저 빼면 그 앞의 옛 전이가 살아난다).
/// 전달 기록은 히스토리를 쓴 뒤에 남으므로 같은 세션·kind·url 이고 전이 시각(초) 이후에 `ok` 로 간 것만
/// 보낸 것으로 친다 — 시각을 못 읽으면 보내지 않은 것으로 본다(두 번 받는 쪽이 놓치는 쪽보다 낫다).
pub fn drop_delivered(
    entries: &[ChangeFeedEntry],
    session_id: &str,
    delivered: &[Delivery],
) -> Vec<ChangeFeedEntry> {
    let secs = |at: &str| {
        chrono::DateTime::parse_from_rfc3339(at)
            .ok()
            .map(|t| t.timestamp())
    };
    latest_pr_entries(entries)
        .into_iter()
        .filter(|e| {
            let url = e
                .history
                .changes
                .as_ref()
                .and_then(|c| c.get("url"))
                .and_then(|v| v.as_str());
            let at = secs(&e.history.at);
            !delivered.iter().any(|d| {
                d.ok && d.session_id == session_id
                    && d.kind == e.history.action
                    && d.url.as_deref().is_some_and(|u| Some(u) == url)
                    && matches!((secs(&d.at), at), (Some(d), Some(e)) if d >= e)
            })
        })
        .cloned()
        .collect()
}

/// 세션 보드 조회 결과. 조회가 **실패한 것**(`Failed` — 데몬 응답 없음·본문 깨짐)과 cwd 가 어느
/// 보드로도 **안 풀리는 것**(`Unmatched`)을 가른다 — 전자에서 커서를 넘기면 그 창의 PR 전이가 이
/// 세션에 영영 안 온다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoardLookup {
    Found(String),
    Unmatched,
    Failed,
}

impl BoardLookup {
    pub fn key(&self) -> Option<&str> {
        match self {
            BoardLookup::Found(key) => Some(key),
            BoardLookup::Unmatched | BoardLookup::Failed => None,
        }
    }
}

/// 이번 턴의 주입을 미루고 커서를 그대로 둘지 — PR 전이가 있는데 보드나 PR 구독 조회가 실패했을 때만.
/// 한 창을 통째로 미루므로(사람 변경 포함) 다음 턴에 같은 창을 다시 읽어도 겹쳐 주입되지 않는다.
pub fn hold_cursor(
    entries: &[ChangeFeedEntry],
    board: &BoardLookup,
    subscriptions_failed: bool,
) -> bool {
    (*board == BoardLookup::Failed || subscriptions_failed)
        && entries.iter().any(|e| e.history.action.starts_with("pr-"))
}

/// 여러 주입 블록을 하나의 additionalContext 로 합친다 — 사람의 보드 변경과 핸드오프
/// 요청이 같은 프롬프트에 함께 도착할 수 있다. 실을 내용이 없으면 `None`.
pub fn merge_context(parts: &[Option<String>]) -> Option<String> {
    let kept: Vec<&str> = parts
        .iter()
        .filter_map(|p| p.as_deref())
        .filter(|p| !p.is_empty())
        .collect();
    if kept.is_empty() {
        None
    } else {
        Some(kept.join("\n\n"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CursorEntry {
    last_id: i64,
    at: String,
}

const MAX_CURSOR_SESSIONS: usize = 100;

/// 커서 파일을 읽는다 — 깨졌거나 없으면 빈 목록. **키 순서(삽입 순서)를 보존**해야
/// 하므로 map 이 아니라 vec 으로 다룬다(`write_cursor` 의 동률 처리 전제).
fn read_cursor_file(file: &Path) -> Vec<(String, CursorEntry)> {
    let Ok(raw) = std::fs::read_to_string(file) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Vec::new();
    };
    let Some(object) = value.as_object() else {
        return Vec::new();
    };
    object
        .iter()
        .filter_map(|(key, entry)| {
            let last_id = entry.get("lastId")?.as_i64()?;
            let at = entry.get("at")?.as_str()?.to_string();
            Some((key.clone(), CursorEntry { last_id, at }))
        })
        .collect()
}

/// 세션의 마지막 확인 지점. 기록이 없으면 `None` (첫 프롬프트).
pub fn read_cursor(file: &Path, session_id: &str) -> Option<i64> {
    read_cursor_file(file)
        .into_iter()
        .find(|(key, _)| key == session_id)
        .map(|(_, entry)| entry.last_id)
}

/// 세션 커서를 기록하고 최근 100 세션만 남긴다.
///
/// `at` 은 밀리초라 여러 세션이 같은 값을 갖기 쉬워 동률을 **삽입 순서**로 깬다 —
/// reverse 로 최신 삽입을 앞에 두고 stable sort(동률 유지)로 최신이 살아남게 한다.
/// 자르고 나서 다시 reverse 해 "파일의 키 순서 = 삽입 순서(오래된 것 먼저)" 전제를
/// 되돌려 저장한다. 정렬된 순서를 그대로 쓰면 다음 호출의 reverse 가 전제를 잃고
/// 동률 그룹이 매 호출 뒤집혀 slice 가 임의 구간을 잘라낸다.
pub fn write_cursor(file: &Path, session_id: &str, last_id: i64) {
    let mut all = read_cursor_file(file);
    all.retain(|(key, _)| key != session_id);
    all.push((
        session_id.to_string(),
        CursorEntry {
            last_id,
            at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        },
    ));

    all.reverse();
    all.sort_by(|(_, a), (_, b)| b.at.cmp(&a.at)); // 최신 먼저, 동률은 삽입 역순(=최신 삽입 먼저) 유지
    all.truncate(MAX_CURSOR_SESSIONS);
    all.reverse();

    // 삽입 순서를 유지한 채 JSON 오브젝트로 — preserve_order 라 Map 이 순서를 지킨다.
    let mut object = serde_json::Map::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for (key, entry) in &all {
        if seen.insert(key.as_str()) {
            object.insert(
                key.clone(),
                serde_json::json!({ "lastId": entry.last_id, "at": entry.at }),
            );
        }
    }
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(
        file,
        serde_json::to_string(&serde_json::Value::Object(object)).unwrap_or_default(),
    );
}
