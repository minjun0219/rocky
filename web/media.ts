import { useSyncExternalStore } from 'react';

/**
 * 넓은 창 — "실행 중" 을 머리 대신 오른쪽 열에 둔다(`web/DESIGN.md` Layout). 머리에 두면 실행 중이 늘 붙어 있어
 * 1920×1080 에서 화면 높이의 1/3 을 먹고, 목록은 가로로 늘어나 오른쪽이 빈다.
 */
export const WIDE_QUERY = '(min-width: 1280px)';

/** media query 가 지금 맞는가 — 창 크기가 바뀌면 다시 그린다. `matchMedia` 가 없으면 false. */
export function useMediaQuery(query: string): boolean {
  return useSyncExternalStore(
    (onChange) => {
      if (typeof window.matchMedia !== 'function') {
        return () => {};
      }
      const list = window.matchMedia(query);
      list.addEventListener('change', onChange);
      return () => list.removeEventListener('change', onChange);
    },
    () => typeof window.matchMedia === 'function' && window.matchMedia(query).matches,
    () => false,
  );
}
