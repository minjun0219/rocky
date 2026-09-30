/**
 * 노트 편집기의 마크다운 서식 명령 — 툴바 버튼과 단축키(⌘B·⌘I·⌘K)가 같은 함수를 부른다.
 *
 * 전부 `EditorState` → 트랜잭션 스펙의 순수 함수라 편집기 없이 테스트한다. 편집은 CRDT 문서로
 * 가므로(yCollab) 여기서는 "무엇을 어떻게 바꿀지" 만 정한다.
 */
import { EditorSelection, type EditorState, type TransactionSpec } from '@codemirror/state';
import type { EditorView } from '@codemirror/view';

/**
 * 선택을 `mark` 로 감싸거나(이미 감싸여 있으면) 벗긴다. 빈 선택이면 `mark mark` 사이에 커서를 둔다.
 * 굵게(`**`)·기울임(`*`)·코드(`` ` ``) 가 쓴다.
 */
export function toggleWrap(state: EditorState, mark: string): TransactionSpec {
  const len = mark.length;
  const ch = mark[0] ?? '';
  // `*` 로 `**굵게**` 안을 누르면 굵게의 별 하나를 떼는 게 아니라 기울임을 더해야 한다 — 같은 글자가
  // 몇 개 붙어 있는지 세어, 한 글자 기호는 홀수 개일 때만 "이미 감쌌다" 로 본다.
  const wrapped = (run: number) => (len === 1 ? run % 2 === 1 : run >= len);
  const runBack = (pos: number) => {
    let n = 0;
    while (pos - n > 0 && state.sliceDoc(pos - n - 1, pos - n) === ch) {
      n++;
    }
    return n;
  };
  const runFwd = (pos: number) => {
    let n = 0;
    while (pos + n < state.doc.length && state.sliceDoc(pos + n, pos + n + 1) === ch) {
      n++;
    }
    return n;
  };
  const leading = (text: string) => text.length - text.replace(new RegExp(`^\\${ch}+`), '').length;
  const trailing = (text: string) => text.length - text.replace(new RegExp(`\\${ch}+$`), '').length;
  return state.changeByRange((range) => {
    if (wrapped(runBack(range.from)) && wrapped(runFwd(range.to))) {
      return {
        changes: [
          { from: range.from - len, to: range.from },
          { from: range.to, to: range.to + len },
        ],
        range: EditorSelection.range(range.from - len, range.to - len),
      };
    }
    const text = state.sliceDoc(range.from, range.to);
    if (text.length >= len * 2 && wrapped(leading(text)) && wrapped(trailing(text))) {
      return {
        changes: { from: range.from, to: range.to, insert: text.slice(len, -len) },
        range: EditorSelection.range(range.from, range.to - len * 2),
      };
    }
    return {
      changes: [
        { from: range.from, insert: mark },
        { from: range.to, insert: mark },
      ],
      range: EditorSelection.range(range.from + len, range.to + len),
    };
  });
}

/** 줄 머리 서식 — 제목·목록·체크박스·인용. 서로 바꿔 끼우고, 같은 걸 다시 누르면 벗긴다. */
const LINE_PREFIX = /^(#{1,6} |[-*] \[[ xX]\] |[-*] |> |\d+\. )/;

/** 선택이 걸친 줄마다 머리(`# `·`- `·`- [ ] `·`> `)를 붙이거나 벗긴다. */
export function toggleLinePrefix(state: EditorState, prefix: string): TransactionSpec {
  const lines = new Set<number>();
  for (const range of state.selection.ranges) {
    const first = state.doc.lineAt(range.from).number;
    const last = state.doc.lineAt(range.to).number;
    for (let n = first; n <= last; n++) {
      lines.add(n);
    }
  }
  const targets = [...lines].map((n) => state.doc.line(n));
  // 모든 줄이 이미 이 머리면 벗기고, 아니면 전부 이 머리로 맞춘다(섞인 선택을 한 번에 정리).
  const allHave = targets.every((line) => line.text.match(LINE_PREFIX)?.[0] === prefix);
  const changes = targets.map((line) => {
    const existing = line.text.match(LINE_PREFIX)?.[0] ?? '';
    return {
      from: line.from,
      to: line.from + existing.length,
      insert: allHave ? '' : prefix,
    };
  });
  return { changes };
}

/** 링크 — 선택한 글자를 `[글자](주소)` 로. 커서는 주소 자리에 둔다. */
export function insertLink(state: EditorState): TransactionSpec {
  return state.changeByRange((range) => {
    const text = state.sliceDoc(range.from, range.to);
    const insert = `[${text}]()`;
    // 글자가 없으면 [] 안에, 있으면 () 안에 커서.
    const cursor = text ? range.from + text.length + 3 : range.from + 1;
    return {
      changes: { from: range.from, to: range.to, insert },
      range: EditorSelection.cursor(cursor),
    };
  });
}

/** 툴바 버튼 하나 — 이름(툴팁·aria), 단축키 표기, 실행. */
export interface FormatAction {
  id: string;
  label: string;
  shortcut?: string;
  run: (state: EditorState) => TransactionSpec;
}

export const FORMAT_ACTIONS: readonly FormatAction[] = [
  { id: 'h2', label: '제목', run: (s) => toggleLinePrefix(s, '## ') },
  { id: 'bold', label: '굵게', shortcut: '⌘B', run: (s) => toggleWrap(s, '**') },
  { id: 'italic', label: '기울임', shortcut: '⌘I', run: (s) => toggleWrap(s, '*') },
  { id: 'code', label: '코드', run: (s) => toggleWrap(s, '`') },
  { id: 'link', label: '링크', shortcut: '⌘K', run: insertLink },
  { id: 'list', label: '목록', run: (s) => toggleLinePrefix(s, '- ') },
  { id: 'task', label: '체크박스', run: (s) => toggleLinePrefix(s, '- [ ] ') },
  { id: 'quote', label: '인용', run: (s) => toggleLinePrefix(s, '> ') },
];

/** 편집기에 한 번 적용한다 — 툴바·단축키 공용. 포커스는 편집기로 돌려준다. */
export function applyFormat(view: EditorView, run: FormatAction['run']): boolean {
  view.dispatch(view.state.update(run(view.state), { scrollIntoView: true, userEvent: 'input' }));
  view.focus();
  return true;
}
