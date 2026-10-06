import { afterEach, describe, expect, test } from 'bun:test';
import { act, cleanup, render, screen } from '@testing-library/react';
import { useMediaQuery, WIDE_QUERY } from './media';

afterEach(cleanup);
const realMatchMedia = window.matchMedia;
afterEach(() => {
  window.matchMedia = realMatchMedia;
});

/** 창 폭을 흉내 내는 matchMedia — `set` 으로 바꾸면 change 를 쏜다. */
function fakeMedia(initial: boolean) {
  let matches = initial;
  const listeners = new Set<() => void>();
  window.matchMedia = ((query: string) => ({
    media: query,
    get matches() {
      return matches;
    },
    addEventListener: (_: string, fn: () => void) => listeners.add(fn),
    removeEventListener: (_: string, fn: () => void) => listeners.delete(fn),
  })) as unknown as typeof window.matchMedia;
  return {
    set(next: boolean) {
      matches = next;
      for (const fn of listeners) {
        fn();
      }
    },
  };
}

function Probe() {
  return <span>{useMediaQuery(WIDE_QUERY) ? 'wide' : 'narrow'}</span>;
}

describe('useMediaQuery', () => {
  test('지금 맞는지 돌려주고, 창이 바뀌면 다시 그린다', () => {
    const media = fakeMedia(false);
    render(<Probe />);
    expect(screen.getByText('narrow')).toBeTruthy();
    act(() => media.set(true));
    expect(screen.getByText('wide')).toBeTruthy();
  });
});
