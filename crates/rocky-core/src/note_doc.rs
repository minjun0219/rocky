//! 노트 본문의 CRDT 문서 — Yjs 호환(`yrs`). 사람(웹 `yjs`)과 에이전트(MCP/CLI 의 set/append)
//! 가 같은 메모를 동시에 고쳐도 글자 단위로 합쳐지게 하는 층이다. 설계는
//! `docs/design/specs/2026-09-28-note-crdt-design.md`.
//!
//! 순수 모듈 — DB·HTTP 를 모른다. 스토어가 `note_docs.state` 를 넣고 꺼내며, 라우트는 바이트를
//! 그대로 나른다. 본문은 루트 `Text("content")` 하나다.
//!
//! **오프셋은 UTF-8 바이트**(`yrs` 기본 `OffsetKind::Bytes`) — 이 모듈이 내는 편집만 그 단위를
//! 쓰고, 클라이언트와 오가는 것은 인덱스가 없는 update 바이너리라 웹(UTF-16)과 어긋날 일이 없다.

use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{Doc, GetString, ReadTxn, StateVector, Text, TextRef, Transact, Update};

/// 본문이 들어 있는 루트 텍스트의 이름. 웹 클라이언트의 `doc.getText('content')` 와 같아야 한다.
pub const TEXT_KEY: &str = "content";

/// `NoteDoc::apply` 의 결과.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Applied {
    /// CRDT state 가 앞으로 갔나 — 저장해야 한다.
    pub state_changed: bool,
    /// 표시 본문이 바뀌었나 — content·히스토리를 갱신한다.
    pub text_changed: bool,
}

/// 노트 하나의 문서. 스토어가 열 때마다 저장된 state 로 복원한다(데몬은 문서를 메모리에
/// 들고 있지 않다 — 노트가 수백 개여도 열린 것만 잠깐 산다).
pub struct NoteDoc {
    doc: Doc,
    text: TextRef,
}

impl NoteDoc {
    /// 저장된 state 로 복원한다. state 가 없거나 깨졌으면 `fallback` 본문으로 새 문서를 만든다
    /// — `notes.content` 가 읽는 쪽의 진실이라 잃는 것은 없다. 복원한 문서의 본문이 `fallback`
    /// 과 다르면(구버전 데몬이 content 만 고친 경우) 문서를 content 에 맞춘다.
    pub fn open(state: Option<&[u8]>, fallback: &str) -> NoteDoc {
        let doc = Doc::new();
        let text = doc.get_or_insert_text(TEXT_KEY);
        let me = NoteDoc { doc, text };
        let restored = match state {
            Some(bytes) => me.apply(bytes).is_ok(),
            None => false,
        };
        if !restored || me.text() != fallback {
            me.set_text(fallback);
        }
        me
    }

    /// 저장·전송된 state 그대로 연다 — 맞출 본문이 없는 쪽(테스트의 클라이언트 흉내)용.
    /// 서버는 `open` 을 쓴다(content 가 진실).
    pub fn from_state(state: &[u8]) -> Result<NoteDoc, String> {
        let doc = Doc::new();
        let text = doc.get_or_insert_text(TEXT_KEY);
        let me = NoteDoc { doc, text };
        me.apply(state)?;
        Ok(me)
    }

    /// 지금 본문.
    pub fn text(&self) -> String {
        self.text.get_string(&self.doc.transact())
    }

    /// 전체 상태(update v1) — 저장용이자 처음 여는 클라이언트에게 주는 것.
    pub fn state(&self) -> Vec<u8> {
        self.doc
            .transact()
            .encode_state_as_update_v1(&StateVector::default())
    }

    /// 상태 벡터(v1) — 클라이언트가 "여기까지 안다" 고 보내는 값의 서버 쪽.
    pub fn state_vector(&self) -> Vec<u8> {
        self.doc.transact().state_vector().encode_v1()
    }

    /// `sv` 이후의 차분(update v1). sv 가 깨졌으면 에러.
    pub fn diff_since(&self, sv: &[u8]) -> Result<Vec<u8>, String> {
        let sv = StateVector::decode_v1(sv).map_err(|e| format!("bad state vector: {e}"))?;
        Ok(self.doc.transact().encode_diff_v1(&sv))
    }

    /// 클라이언트(또는 저장된 state)의 update 를 적용한다.
    ///
    /// **state 가 바뀐 것과 본문이 바뀐 것은 다르다** — 같은 글자를 지웠다 다시 넣은 편집이나
    /// 의존 update 가 먼저 도착한 경우는 본문은 그대로인데 state 는 앞으로 간다. 호출자는
    /// `state_changed` 로 저장을, `text_changed` 로 content·히스토리를 판단해야 한다. 전자를
    /// 후자로 판단하면 그 update 가 버려져 뒤이어 오는(그것에 기대는) update 가 영영 안 붙는다.
    pub fn apply(&self, update: &[u8]) -> Result<Applied, String> {
        let update = Update::decode_v1(update).map_err(|e| format!("bad update: {e}"))?;
        let text_before = self.text();
        let sv_before = self.doc.transact().state_vector();
        {
            let mut txn = self.doc.transact_mut();
            txn.apply_update(update)
                .map_err(|e| format!("update rejected: {e}"))?;
        }
        Ok(Applied {
            state_changed: self.doc.transact().state_vector() != sv_before,
            text_changed: self.text() != text_before,
        })
    }

    /// 본문을 `next` 로 만든다 — 통째로 갈지 않고 **공통 접두·접미를 뺀 한 구간**만 지우고
    /// 넣는다. 그래야 같은 순간 다른 자리를 고치던 사람의 글자가 살아남는다(전체 교체는
    /// 상대의 편집이 붙을 자리를 전부 지워 버린다). `set` 의미는 그대로다: 결과는 `next`.
    pub fn set_text(&self, next: &str) {
        let current = self.text();
        if current == next {
            return;
        }
        let (prefix, cur_tail, next_tail) = split_common(&current, next);
        let mut txn = self.doc.transact_mut();
        if !cur_tail.is_empty() {
            self.text
                .remove_range(&mut txn, prefix as u32, cur_tail.len() as u32);
        }
        if !next_tail.is_empty() {
            self.text.insert(&mut txn, prefix as u32, next_tail);
        }
    }

    /// 뒤에 이어붙인다 — 비어 있지 않으면 줄바꿈으로 잇는다(`update_note` 의 append 와 같은 규칙).
    pub fn append(&self, chunk: &str) {
        let current = self.text();
        let piece = if current.is_empty() {
            chunk.to_string()
        } else {
            format!("\n{chunk}")
        };
        let mut txn = self.doc.transact_mut();
        self.text.insert(&mut txn, current.len() as u32, &piece);
    }
}

/// `a`/`b` 의 공통 접두(바이트 길이)와 그 뒤의 서로 다른 꼬리(공통 접미를 뺀). 경계는 문자
/// 경계다 — 바이트 단위로 자르면 한글 가운데가 잘려 `insert` 가 패닉한다.
fn split_common<'a>(a: &'a str, b: &'a str) -> (usize, &'a str, &'a str) {
    let mut prefix = 0;
    for (ca, cb) in a.chars().zip(b.chars()) {
        if ca != cb {
            break;
        }
        prefix += ca.len_utf8();
    }
    let a_rest = &a[prefix..];
    let b_rest = &b[prefix..];
    let mut suffix = 0;
    for (ca, cb) in a_rest.chars().rev().zip(b_rest.chars().rev()) {
        if ca != cb {
            break;
        }
        suffix += ca.len_utf8();
    }
    (
        prefix,
        &a_rest[..a_rest.len() - suffix],
        &b_rest[..b_rest.len() - suffix],
    )
}

#[cfg(test)]
mod tests {
    use super::split_common;

    #[test]
    fn common_parts_are_cut_on_char_boundaries() {
        assert_eq!(split_common("가나다", "가라다"), (3, "나", "라"));
        assert_eq!(split_common("abc", "abc"), (3, "", ""));
        assert_eq!(split_common("", "x"), (0, "", "x"));
        assert_eq!(split_common("ab", "a"), (1, "b", ""));
    }
}
