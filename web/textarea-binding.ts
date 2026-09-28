/**
 * `<textarea>` ↔ `Y.Text` 바인딩 — 라이브러리 없이 직접 짠 얇은 층(편집기 후보 (a)).
 *
 * 로컬: `input` 마다 직전 값과의 최소 diff 를 문서에 넣는다(IME 조합 중의 중간 상태도 그대로
 * diff 라 한글 조합이 깨지지 않는다). 원격: observe 로 새 본문을 그리고 커서를 delta 만큼
 * 옮긴다. 조합 중에는 호출자가 원격 적용을 멈춘다(`pauseRemote`/`resumeRemote`).
 */
import type * as Y from 'yjs';
import { type DeltaOp, diffText, shiftCursor } from './notedoc';

export interface BindOptions {
  /** 로컬 편집의 트랜잭션 원점 — observe 가 자기 편집을 다시 그리지 않게 가른다. */
  origin: unknown;
  pauseRemote?: () => void;
  resumeRemote?: () => void;
  /** 본문이 바뀔 때(로컬·원격 모두) — 줄 수 같은 파생 상태용. */
  onChange?: (value: string) => void;
}

/** 바인딩을 걸고 해제 함수를 돌려준다. 걸리는 순간 textarea 는 문서 본문으로 덮인다. */
export function bindTextarea(
  el: HTMLTextAreaElement,
  text: Y.Text,
  options: BindOptions,
): () => void {
  let prev = text.toString();
  el.value = prev;

  const onInput = () => {
    const next = el.value;
    const edit = diffText(prev, next);
    if (edit.remove === 0 && edit.insert === '') {
      return;
    }
    text.doc?.transact(() => {
      if (edit.remove > 0) {
        text.delete(edit.index, edit.remove);
      }
      if (edit.insert !== '') {
        text.insert(edit.index, edit.insert);
      }
    }, options.origin);
    prev = next;
    options.onChange?.(next);
  };

  const observer = (event: Y.YTextEvent, txn: Y.Transaction) => {
    if (txn.origin === options.origin) {
      return;
    }
    const value = text.toString();
    const delta = event.delta as DeltaOp[];
    const start = shiftCursor(delta, el.selectionStart);
    const end = shiftCursor(delta, el.selectionEnd);
    el.value = value;
    prev = value;
    try {
      el.setSelectionRange(start, end);
    } catch {
      // 포커스가 없는 textarea 는 브라우저에 따라 거부한다 — 커서는 어차피 안 보인다.
    }
    options.onChange?.(value);
  };

  const onCompositionStart = () => options.pauseRemote?.();
  const onCompositionEnd = () => options.resumeRemote?.();

  el.addEventListener('input', onInput);
  el.addEventListener('compositionstart', onCompositionStart);
  el.addEventListener('compositionend', onCompositionEnd);
  text.observe(observer);

  return () => {
    el.removeEventListener('input', onInput);
    el.removeEventListener('compositionstart', onCompositionStart);
    el.removeEventListener('compositionend', onCompositionEnd);
    text.unobserve(observer);
  };
}
