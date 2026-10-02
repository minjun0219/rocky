/**
 * 노트 편집기: CodeMirror 6 + `y-codemirror.next` — 상대 커서·선택 영역까지 그린다. 마크다운(GFM)을
 * 편집하는 자리에서 꾸며 보여 주고(제목 크기·굵게·코드), 서식 명령은 `markdown-commands.ts`.
 *
 * 문서 동기화는 `NoteSync`. 여기서 더하는 것은 **awareness**: y-codemirror
 * 는 y-protocols 의 `Awareness` 로 커서를 주고받는데 그건 전송을 모른다. 여기서 awareness 의
 * 로컬 변경을 프레즌스 라우트의 `state` 에 실어 보내고, 남의 프레즌스 `state` 를 awareness 에
 * 넣는 다리를 놓는다(`bridgeAwareness`). 프레즌스 핑(20초)이 로컬 상태를 다시 실어 보내므로
 * awareness 의 30초 만료를 넘기지 않는다.
 */
import { markdown, markdownLanguage } from '@codemirror/lang-markdown';
import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import { EditorState } from '@codemirror/state';
import { EditorView, keymap, placeholder } from '@codemirror/view';
import { tags } from '@lezer/highlight';
import { yCollab, yUndoManagerKeymap } from 'y-codemirror.next';
import * as Y from 'yjs';
import {
  applyAwarenessUpdate,
  Awareness,
  encodeAwarenessUpdate,
  removeAwarenessStates,
} from 'y-protocols/awareness';
import { applyFormat, insertLink, toggleWrap } from './markdown-commands';
import { fromB64, type NoteSync, toB64 } from './notedoc';

/**
 * 마크다운을 쓰는 자리에서 꾸민다 — 기호(`#`·`**`)는 흐리게 남기고 내용만 모양을 준다. 색은 테마
 * 토큰(`web/styles/tokens.css`)이라 다크 모드를 따라간다.
 */
export const noteHighlight = HighlightStyle.define([
  { tag: tags.heading1, fontSize: '1.3em', fontWeight: '700', color: 'var(--color-text)' },
  { tag: tags.heading2, fontSize: '1.15em', fontWeight: '700', color: 'var(--color-text)' },
  { tag: [tags.heading3, tags.heading4, tags.heading5, tags.heading6], fontWeight: '700' },
  { tag: tags.strong, fontWeight: '700', color: 'var(--color-text)' },
  { tag: tags.emphasis, fontStyle: 'italic' },
  { tag: tags.strikethrough, textDecoration: 'line-through' },
  { tag: tags.monospace, fontFamily: 'var(--font-mono)', fontSize: '0.92em' },
  { tag: [tags.link, tags.url], color: 'var(--color-link)', textDecoration: 'underline' },
  { tag: tags.quote, fontStyle: 'italic' },
  { tag: [tags.processingInstruction, tags.contentSeparator], color: 'var(--color-faint)' },
]);

/**
 * 커서를 화면 안으로 끌어올 때 가려지는 띠 — 화면 위의 고정 머리(`--app-head-h`, main.tsx 가 잰다)와
 * 서식 툴바, 바닥의 고정 버전 줄. 이걸 모르면 긴 글 끝에서 치는 줄이 버전 줄 밑에 숨는다.
 */
const TOOLBAR_H = 40;
const FOOTER_H = 36;
const shellMargins = EditorView.scrollMargins.of(() => {
  const head = Number.parseFloat(
    getComputedStyle(document.documentElement).getPropertyValue('--app-head-h'),
  );
  return { top: (Number.isFinite(head) ? head : 0) + TOOLBAR_H, bottom: FOOTER_H };
});

/** 서식 단축키 — 툴바와 같은 명령. */
const formatKeymap = keymap.of([
  { key: 'Mod-b', run: (view) => applyFormat(view, (s) => toggleWrap(s, '**')) },
  { key: 'Mod-i', run: (view) => applyFormat(view, (s) => toggleWrap(s, '*')) },
  { key: 'Mod-k', run: (view) => applyFormat(view, insertLink) },
]);

/**
 * 마크다운을 쓰는 자리의 공통 설정 — 노트 본문과 할 일 설명이 같은 손맛이어야 한다(서식 단축키·GFM·
 * 꾸밈·줄바꿈·고정 머리 밑으로 숨지 않기). 문서를 어디에 두는지(Y.Text / 문자열)는 부르는 쪽이 정한다.
 */
function markdownEditing() {
  return [
    formatKeymap,
    // GFM — 체크박스·취소선·표. 목록에서 Enter 는 다음 머리를 이어 준다(markdownKeymap).
    markdown({ base: markdownLanguage }),
    syntaxHighlighting(noteHighlight),
    shellMargins,
    placeholder('마크다운으로 적는다 — ⌘B 굵게 · ⌘K 링크 · "- [ ] " 체크박스'),
    EditorView.lineWrapping,
  ];
}

/**
 * 문자열 하나를 편집하는 마크다운 편집기 — 할 일 설명처럼 실시간 문서(CRDT)가 아닌 자리. 저장은 부르는
 * 쪽이 한다: `onChange` 로 지금 글을 받고, ⌘Enter 는 `onSave`, Esc 는 `onCancel`.
 *
 * 되돌리기(⌘Z)는 동기화하지 않는 로컬 `Y.Text` 의 UndoManager 로 한다 — 노트와 같은 키·같은 손맛이고,
 * CodeMirror 의 history 를 쓰려면 `@codemirror/commands` 를 새로 들여야 해서.
 */
export function mountMarkdownEditor(
  parent: HTMLElement,
  options: {
    doc: string;
    onChange: (text: string) => void;
    onSave?: () => void;
    onCancel?: () => void;
  },
): EditorView {
  const text = new Y.Doc().getText('body');
  text.insert(0, options.doc);
  return new EditorView({
    parent,
    state: EditorState.create({
      doc: options.doc,
      extensions: [
        keymap.of([
          {
            key: 'Mod-Enter',
            run: () => {
              options.onSave?.();
              return true;
            },
          },
          {
            key: 'Escape',
            run: () => {
              options.onCancel?.();
              return true;
            },
          },
          ...yUndoManagerKeymap,
        ]),
        ...markdownEditing(),
        yCollab(text, null),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) {
            options.onChange(update.state.doc.toString());
          }
        }),
      ],
    }),
  });
}

/** 사람·에이전트별 커서 색 — 이름의 해시로 고정(같은 이름은 늘 같은 색). */
const CURSOR_COLORS = ['#167a56', '#c2410c', '#1d5fb0', '#7a2e12', '#6d28d9', '#0f766e'];

export function colorFor(actor: string): string {
  let h = 0;
  for (let i = 0; i < actor.length; i++) {
    h = (h * 31 + actor.charCodeAt(i)) >>> 0;
  }
  return CURSOR_COLORS[h % CURSOR_COLORS.length] ?? CURSOR_COLORS[0]!;
}

/** 프레즌스 `state` 에 실리는 모양 — awareness update(v1) 의 base64. */
export interface AwarenessState {
  awareness: string;
}

function isAwarenessState(state: unknown): state is AwarenessState {
  return (
    typeof state === 'object' &&
    state !== null &&
    typeof (state as { awareness?: unknown }).awareness === 'string'
  );
}

/** 커서 이동을 묶어 보내는 간격 — 문서 편집의 배치(150ms)와 같은 결로. */
export const AWARENESS_FLUSH_MS = 150;

/**
 * awareness ↔ 프레즌스 다리. 돌려주는 함수로 걷는다.
 *
 * - 로컬 상태가 바뀌면(커서 이동·이름 설정) 내 clientID 의 update 를 `sync.ping` 에 실어 보낸다.
 *   타이핑·커서 이동은 글자마다 awareness 를 바꾸므로 **150ms 로 묶어 마지막 상태만** 보낸다
 *   (그대로 두면 키 입력마다 POST + 방송이 나가 터널 지연에서 밀린다). 떠나는 것(removed)만 즉시.
 * - `sync.onPresenceState` 로 온 남의 update 를 awareness 에 넣는다.
 * - 20초 핑도 같은 페이로드로 — 호출자는 `pingState()` 를 핑에 쓴다.
 */
export function bridgeAwareness(
  awareness: Awareness,
  sync: Pick<NoteSync, 'ping'> & { onPresenceState: NoteSync['onPresenceState'] },
): { pingState: () => AwarenessState; dispose: () => void } {
  const pingState = (): AwarenessState => ({
    awareness: toB64(encodeAwarenessUpdate(awareness, [awareness.clientID])),
  });
  let timer: ReturnType<typeof setTimeout> | null = null;
  const flush = () => {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
    void sync.ping(pingState());
  };
  const onUpdate = (
    { added, updated, removed }: { added: number[]; updated: number[]; removed: number[] },
    origin: unknown,
  ) => {
    if (origin === 'remote') {
      return;
    }
    const me = awareness.clientID;
    // 떠나는 것(removed)도 실어 보낸다 — 안 그러면 남들은 내 커서를 30초 만료까지 들고 있고,
    // 그 사이 다시 열면 새 client id 라 같은 이름의 커서가 둘 보인다. 이건 미루지 않는다.
    if (removed.includes(me)) {
      flush();
      return;
    }
    if ((added.includes(me) || updated.includes(me)) && !timer) {
      timer = setTimeout(flush, AWARENESS_FLUSH_MS);
    }
  };
  awareness.on('update', onUpdate);
  sync.onPresenceState = (_client: string, state: unknown) => {
    if (isAwarenessState(state)) {
      try {
        applyAwarenessUpdate(awareness, fromB64(state.awareness), 'remote');
      } catch {
        // 깨진 한 건은 버린다.
      }
    }
  };
  return {
    pingState,
    dispose: () => {
      // 걷기 전에 "나 갔다" 를 먼저 보낸다 — onUpdate 가 아직 붙어 있어야 핑이 나간다.
      // 묶여 있던 커서 갱신은 버린다(떠나는 상태가 그걸 덮는다).
      if (timer) {
        clearTimeout(timer);
        timer = null;
      }
      removeAwarenessStates(awareness, [awareness.clientID], 'local');
      awareness.off('update', onUpdate);
      sync.onPresenceState = null;
    },
  };
}

export interface MountedEditor {
  view: EditorView;
  awareness: Awareness;
  /** 프레즌스 핑에 실을 페이로드. */
  pingState: () => AwarenessState;
  destroy: () => void;
}

/**
 * 열린 `NoteSync` 위에 CodeMirror 를 올린다. 문서는 `sync.text`, 커서는 awareness 로.
 * `onFocus(false)` 가 오면 호출자가 세션 정리를 예약한다(textarea 의 blur 와 같은 규칙).
 */
export function mountNoteEditor(
  parent: HTMLElement,
  sync: NoteSync,
  options: { actor: string; onFocus?: (focused: boolean) => void },
): MountedEditor {
  const awareness = new Awareness(sync.doc);
  awareness.setLocalStateField('user', {
    name: options.actor,
    color: colorFor(options.actor),
    colorLight: `${colorFor(options.actor)}33`,
  });
  const bridge = bridgeAwareness(awareness, sync);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc: sync.text.toString(),
      extensions: [
        keymap.of([...yUndoManagerKeymap]),
        ...markdownEditing(),
        yCollab(sync.text, awareness),
        EditorView.updateListener.of((update) => {
          if (update.focusChanged) {
            options.onFocus?.(update.view.hasFocus);
          }
        }),
      ],
    }),
  });
  return {
    view,
    awareness,
    pingState: bridge.pingState,
    destroy: () => {
      bridge.dispose();
      awareness.destroy();
      view.destroy();
    },
  };
}
