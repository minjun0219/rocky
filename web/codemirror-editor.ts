/**
 * 편집기 후보 (b): CodeMirror 6 + `y-codemirror.next` — 상대 커서·선택 영역까지 그린다.
 *
 * 문서 동기화는 후보 (a)와 같은 `NoteSync` 를 그대로 쓴다. 다른 점은 **awareness**: y-codemirror
 * 는 y-protocols 의 `Awareness` 로 커서를 주고받는데 그건 전송을 모른다. 여기서 awareness 의
 * 로컬 변경을 프레즌스 라우트의 `state` 에 실어 보내고, 남의 프레즌스 `state` 를 awareness 에
 * 넣는 다리를 놓는다(`bridgeAwareness`). 프레즌스 핑(20초)이 로컬 상태를 다시 실어 보내므로
 * awareness 의 30초 만료를 넘기지 않는다.
 */
import { markdown } from '@codemirror/lang-markdown';
import { EditorState } from '@codemirror/state';
import { EditorView, keymap } from '@codemirror/view';
import { yCollab, yUndoManagerKeymap } from 'y-codemirror.next';
import {
  applyAwarenessUpdate,
  Awareness,
  encodeAwarenessUpdate,
  removeAwarenessStates,
} from 'y-protocols/awareness';
import { fromB64, type NoteSync, toB64 } from './notedoc';

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
        markdown(),
        EditorView.lineWrapping,
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

/** 편집기 선택 — 둘을 번갈아 써 보고 하나를 지우기 위한 임시 스위치. localStorage 에 남는다. */
export type NoteEditorKind = 'textarea' | 'codemirror';
export const EDITOR_PREF_KEY = 'rocky.noteEditor';

export function readEditorPref(storage: Pick<Storage, 'getItem'>): NoteEditorKind {
  try {
    return storage.getItem(EDITOR_PREF_KEY) === 'codemirror' ? 'codemirror' : 'textarea';
  } catch {
    return 'textarea';
  }
}

export function writeEditorPref(storage: Pick<Storage, 'setItem'>, kind: NoteEditorKind): void {
  try {
    storage.setItem(EDITOR_PREF_KEY, kind);
  } catch {
    // 저장 못 해도 이번 화면에서는 동작한다.
  }
}
