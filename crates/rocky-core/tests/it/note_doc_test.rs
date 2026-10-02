//! 노트 CRDT 문서 — 복원·최소 편집·동시 편집 병합·차분 동기화.

use rocky_core::note_doc::NoteDoc;

#[test]
fn opens_from_content_when_there_is_no_state() {
    let doc = NoteDoc::open(None, "첫 줄\n둘째 줄");
    assert_eq!(doc.text(), "첫 줄\n둘째 줄");
    // 저장 → 다시 열기.
    let again = NoteDoc::open(Some(&doc.state()), "첫 줄\n둘째 줄");
    assert_eq!(again.text(), "첫 줄\n둘째 줄");
}

/// 구버전 데몬이 content 만 고친 채 남긴 state — content 가 진실이라 문서를 거기 맞춘다.
#[test]
fn a_stale_state_is_reconciled_to_content() {
    let doc = NoteDoc::open(None, "old");
    let state = doc.state();
    let reopened = NoteDoc::open(Some(&state), "new content");
    assert_eq!(reopened.text(), "new content");
}

#[test]
fn garbage_state_falls_back_to_content() {
    let doc = NoteDoc::open(Some(b"\xff\xfe not an update"), "safe");
    assert_eq!(doc.text(), "safe");
}

/// 에이전트의 set 이 사람의 동시 편집을 지우지 않는다 — 다른 자리를 고친 두 편집이 둘 다 남는다.
#[test]
fn set_text_is_a_minimal_edit_so_concurrent_edits_survive() {
    let server = NoteDoc::open(None, "## 계획\n- a\n- b\n\n## 메모\n");
    let base = server.state();
    // 사람: 첫 절에 항목 추가.
    let human = NoteDoc::open(Some(&base), "## 계획\n- a\n- b\n\n## 메모\n");
    human.set_text("## 계획\n- a\n- b\n- c\n\n## 메모\n");
    let human_update = human.diff_since(&server.state_vector()).unwrap();
    // 에이전트: 같은 순간 둘째 절에 set 으로 문장 추가(전체 본문을 보냄).
    server.set_text("## 계획\n- a\n- b\n\n## 메모\n에이전트가 적음\n");
    assert!(server.apply(&human_update).unwrap().text_changed);
    assert_eq!(
        server.text(),
        "## 계획\n- a\n- b\n- c\n\n## 메모\n에이전트가 적음\n"
    );
}

#[test]
fn append_joins_with_a_newline_like_update_note() {
    let doc = NoteDoc::open(None, "");
    doc.append("첫");
    doc.append("둘");
    assert_eq!(doc.text(), "첫\n둘");
}

#[test]
fn diff_since_gives_only_what_the_client_lacks() {
    let server = NoteDoc::open(None, "hello");
    let client = NoteDoc::open(Some(&server.state()), "hello");
    server.append("world");
    let diff = server.diff_since(&client.state_vector()).unwrap();
    assert!(diff.len() < server.state().len());
    assert!(client.apply(&diff).unwrap().text_changed);
    assert_eq!(client.text(), "hello\nworld");
    // 이미 아는 것을 다시 적용하면 바뀐 게 없다.
    assert_eq!(
        client.apply(&diff).unwrap(),
        rocky_core::note_doc::Applied {
            state_changed: false,
            text_changed: false
        }
    );
}

#[test]
fn bad_input_is_an_error_not_a_panic() {
    let doc = NoteDoc::open(None, "x");
    assert!(doc.apply(b"nope").is_err());
    assert!(doc.diff_since(b"\xff").is_err());
}

#[test]
fn korean_text_edits_land_on_char_boundaries() {
    let doc = NoteDoc::open(None, "가나다라");
    doc.set_text("가마다라");
    assert_eq!(doc.text(), "가마다라");
    doc.set_text("가마다");
    assert_eq!(doc.text(), "가마다");
    doc.append("한글 끝");
    assert_eq!(doc.text(), "가마다\n한글 끝");
}

/// 같은 글자를 지웠다 다시 넣으면 본문은 그대로인데 state 는 앞으로 간다 — 둘을 가른다.
#[test]
fn a_delete_and_reinsert_of_the_same_text_changes_state_but_not_text() {
    let server = NoteDoc::open(None, "x");
    let client = NoteDoc::from_state(&server.state()).unwrap();
    client.set_text("");
    client.set_text("x");
    let diff = client.diff_since(&server.state_vector()).unwrap();
    let applied = server.apply(&diff).unwrap();
    assert!(applied.state_changed && !applied.text_changed);
}

/// 같은 클라이언트의 두 배치가 역순으로 오면 뒤 배치는 선행 조각이 없어 pending 이 된다 —
/// 그것도 "바뀐 상태" 라 저장돼야 하고, 저장된 state 로 다시 열어도 붙들려 있다가 선행
/// 배치가 오면 붙는다.
#[test]
fn an_out_of_order_update_counts_as_a_state_change_and_survives_a_reopen() {
    let server = NoteDoc::open(None, "x");
    let client = NoteDoc::from_state(&server.state()).unwrap();
    let sv0 = client.state_vector();
    client.append("first");
    let sv1 = client.state_vector();
    let batch1 = client.diff_since(&sv0).unwrap();
    client.append("second");
    let batch2 = client.diff_since(&sv1).unwrap();

    let applied = server.apply(&batch2).unwrap();
    assert!(
        applied.state_changed && !applied.text_changed,
        "{applied:?}"
    );
    assert_eq!(server.text(), "x");
    // 저장 → 다시 열기 → 선행 배치 도착.
    let reopened = NoteDoc::open(Some(&server.state()), "x");
    let applied = reopened.apply(&batch1).unwrap();
    assert!(applied.state_changed && applied.text_changed);
    assert_eq!(reopened.text(), "x\nfirst\nsecond");
}

/// 삭제만 있는 update 는 Yjs 의 state vector(삽입 clock)를 안 올리고 DeleteSet 만 바꾼다 —
/// vector 비교면 "안 바뀜" 이 되어 지운 글자가 다음 동기화에 되살아난다. 전체 상태 비교라야 잡힌다.
#[test]
fn a_delete_only_update_is_a_state_change() {
    let server = NoteDoc::open(None, "hello");
    let client = NoteDoc::from_state(&server.state()).unwrap();
    client.set_text("hllo");
    let diff = client.diff_since(&server.state_vector()).unwrap();
    let applied = server.apply(&diff).unwrap();
    assert!(applied.state_changed && applied.text_changed, "{applied:?}");
    assert_eq!(server.text(), "hllo");
    // 저장했다 다시 열어도 지워진 채다.
    assert_eq!(NoteDoc::open(Some(&server.state()), "hllo").text(), "hllo");
}
