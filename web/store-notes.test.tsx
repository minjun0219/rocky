import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { useUiStore } from './store';
import type { NoteView } from './types';

/**
 * 노트 상세의 주소·뒤로가기 규칙 — 컴포넌트 테스트는 스토어를 목으로 바꿔 쓰니 여기서 진짜 스토어로 본다.
 * (`.tsx` 인 이유: happy-dom preload 가 붙어야 `window.history` 가 있다.)
 */

const NOTE: NoteView = {
  id: 'n3',
  number: 3,
  ref: 'rocky-3',
  title: '회의록',
  content: '',
  position: 0,
  createdAt: '2026-09-30T00:00:00.000Z',
  updatedAt: '2026-09-30T00:00:00.000Z',
};

const pristine = useUiStore.getState();
const realFetch = globalThis.fetch;

beforeEach(() => {
  // refetch 는 서버가 없으니 실패한다 — 여기서 보는 것은 주소와 상태뿐이다.
  // 기본 문서는 about:blank 라 pushState 가 경로를 싣지 못한다 — 앱과 같은 출처로 옮긴다.
  (window as unknown as { happyDOM: { setURL: (url: string) => void } }).happyDOM.setURL(
    'http://localhost/rocky',
  );
  window.history.replaceState(null, '', '/rocky');
  // 사용 로그(POST /api/usage)는 받아만 준다.
  globalThis.fetch = (async () => new Response('{}')) as unknown as typeof fetch;
  // 목록 다시 읽기는 서버가 없으니 빈 동작으로 — 여기서 보는 것은 주소와 상태뿐이다.
  useUiStore.setState(
    { ...pristine, selected: 'rocky', notes: [NOTE], view: 'notes', refetch: async () => {} },
    true,
  );
});

afterEach(() => {
  globalThis.fetch = realFetch;
});

const here = () => window.location.pathname + window.location.search;

describe('노트 상세 주소', () => {
  test('열면 /{board}/notes/{n} 항목을 쌓는다', () => {
    useUiStore.getState().openNote('n3');
    expect(useUiStore.getState().openNoteId).toBe('n3');
    expect(here()).toBe('/rocky/notes/3');
    expect((window.history.state as { rockyNote?: boolean }).rockyNote).toBe(true);
  });

  test('상세 위의 히스토리 드로어를 닫아도 상세 주소와 뒤로가기 항목은 그대로다', () => {
    useUiStore.getState().openNote('n3');
    useUiStore.setState({ detail: { kind: 'note', note: NOTE, history: [], comments: [] } });
    useUiStore.getState().closeDetail();
    expect(useUiStore.getState().detail).toBeNull();
    expect(here()).toBe('/rocky/notes/3');
    expect((window.history.state as { rockyNote?: boolean }).rockyNote).toBe(true);
  });

  test('보드를 바꾸면 상세를 닫는다', () => {
    useUiStore.getState().openNote('n3');
    useUiStore.getState().setSelected('other');
    expect(useUiStore.getState().openNoteId).toBeNull();
  });

  test('상세에서 "노트" 탭을 다시 누르면 목록으로 돌아간다', () => {
    useUiStore.getState().openNote('n3');
    useUiStore.getState().setView('notes');
    expect(useUiStore.getState().openNoteId).toBeNull();
  });

  test('퍼머링크로 들어와 닫으면 뒤로 가지 않고 보드 주소로 갈아끼운다', () => {
    window.history.replaceState(null, '', '/rocky/notes/3');
    useUiStore.setState({ openNoteId: 'n3' });
    useUiStore.getState().closeNote();
    expect(here()).toBe('/rocky');
  });

  test('옛 보드 key 로 온 노트 링크도 연다', async () => {
    useUiStore.setState({
      boards: [
        { id: 'b1', key: 'rocky', title: 'rocky', createdAt: '', previousKeys: ['old'] },
      ] as never,
    });
    await useUiStore.getState().applyRoute({ board: 'old', note: 'old-3' });
    expect(useUiStore.getState().openNoteId).toBe('n3');
    expect(here()).toBe('/rocky/notes/3');
  });
});
