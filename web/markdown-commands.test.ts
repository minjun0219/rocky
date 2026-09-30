import { describe, expect, test } from 'bun:test';
import { EditorSelection, EditorState } from '@codemirror/state';
import { insertLink, toggleLinePrefix, toggleWrap } from './markdown-commands';

/** `|` 가 커서, `[`…`]` 가 선택인 문자열로 상태를 만든다. */
function at(doc: string, from: number, to = from) {
  return EditorState.create({ doc, selection: EditorSelection.single(from, to) });
}

function apply(state: EditorState, spec: ReturnType<typeof toggleWrap>) {
  const next = state.update(spec).state;
  const main = next.selection.main;
  return { doc: next.doc.toString(), from: main.from, to: main.to };
}

describe('toggleWrap', () => {
  test('선택을 감싸고 선택은 안쪽 글자에 남긴다', () => {
    const s = at('할 일 정리', 0, 3);
    expect(apply(s, toggleWrap(s, '**'))).toEqual({ doc: '**할 일** 정리', from: 2, to: 5 });
  });

  test('이미 감싼 선택은 벗긴다 — 같은 버튼이 켜고 끈다', () => {
    const s = at('**할 일** 정리', 2, 5);
    expect(apply(s, toggleWrap(s, '**'))).toEqual({ doc: '할 일 정리', from: 0, to: 3 });
  });

  test('기호째 고른 선택도 벗긴다', () => {
    const s = at('`code`', 0, 6);
    expect(apply(s, toggleWrap(s, '`')).doc).toBe('code');
  });

  test('굵게 안에서 기울임은 별을 더한다 — 굵게를 깨지 않는다', () => {
    const s = at('**꽉**', 2, 3);
    const once = apply(s, toggleWrap(s, '*'));
    expect(once.doc).toBe('***꽉***');
    const again = EditorState.create({
      doc: once.doc,
      selection: EditorSelection.single(once.from, once.to),
    });
    expect(apply(again, toggleWrap(again, '*')).doc).toBe('**꽉**');
  });

  test('빈 선택이면 기호 한 쌍 사이에 커서', () => {
    const s = at('ab', 1);
    expect(apply(s, toggleWrap(s, '*'))).toEqual({ doc: 'a**b', from: 2, to: 2 });
  });
});

describe('toggleLinePrefix', () => {
  test('걸친 줄마다 붙이고, 다시 누르면 벗긴다', () => {
    const s = at('하나\n둘', 0, 4);
    const once = s.update(toggleLinePrefix(s, '- ')).state;
    expect(once.doc.toString()).toBe('- 하나\n- 둘');
    const twice = once.update(toggleLinePrefix(once, '- ')).state;
    expect(twice.doc.toString()).toBe('하나\n둘');
  });

  test('다른 머리는 바꿔 끼운다 — 목록이 체크박스로, 제목 단계가 바뀐다', () => {
    const list = at('- 우유', 0);
    expect(apply(list, toggleLinePrefix(list, '- [ ] ')).doc).toBe('- [ ] 우유');
    const heading = at('# 제목', 0);
    expect(apply(heading, toggleLinePrefix(heading, '## ')).doc).toBe('## 제목');
  });
});

describe('insertLink', () => {
  test('선택한 글자를 링크 글자로 두고 커서를 주소 자리에', () => {
    const s = at('문서 보기', 0, 2);
    expect(apply(s, insertLink(s))).toEqual({ doc: '[문서]() 보기', from: 5, to: 5 });
  });

  test('선택이 없으면 글자 자리에 커서', () => {
    const s = at('', 0);
    expect(apply(s, insertLink(s))).toEqual({ doc: '[]()', from: 1, to: 1 });
  });
});
