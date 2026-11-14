import type { AppView } from './types';

/**
 * URL ↔ 탭 — 웹 UI 주소의 단일 소유자. 라우터 라이브러리를 쓰지 않고(새 런타임 dep 은 별도 논의) 순수 변환만 둔다 —
 * History API 호출은 `store.ts` 가 한다.
 *
 * 주소는 탭 하나만 싣는다: `/`(피드) · `/?view=worklog`. 보드 · 할 일 · 노트 주소(`/rocky/12`, `/rocky/notes/3`)는
 * 2026-11-13 보드 화면을 걷어내며 없앴다 — 그런 옛 링크는 피드로 떨어지고 주소는 `/` 로 고친다(죽은 링크에 에러 화면을
 * 띄우지 않는다).
 */

export const VIEWS: readonly AppView[] = ['feed', 'worklog', 'github', 'agents', 'rc'];

/** `?view=worklog` → `'worklog'`. 모르는 값 · 피드 · 깨진 쿼리는 `'feed'`(첫 화면). 경로는 보지 않는다. */
export function parseView(search: string): AppView {
  let raw: string | null;
  try {
    raw = new URLSearchParams(search).get('view');
  } catch {
    return 'feed';
  }
  return raw !== null && VIEWS.includes(raw as AppView) ? (raw as AppView) : 'feed';
}

/** 탭 → 주소. 피드는 맨 주소다. */
export function pathFor(view: AppView): string {
  return view === 'feed' ? '/' : `/?view=${view}`;
}
