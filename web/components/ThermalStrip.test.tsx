import { afterEach, describe, expect, test } from 'bun:test';
import { act, cleanup } from '@testing-library/react';
import { renderWithStore } from '../test-support';
import { ThermalStrip, thermalCaption } from './ThermalStrip';

afterEach(cleanup);

const NEWEST = { id: 2, actor: 'claude-code', at: '2026-08-16T12:00:00.000Z' }; // 최신 (API 는 최신순)
const OLDEST = { id: 1, actor: 'minjun', at: '2026-08-16T11:00:00.000Z' };
const EVENTS = [NEWEST, OLDEST];

function mockHistory(rows: unknown) {
  const original = globalThis.fetch;
  globalThis.fetch = (async () => new Response(JSON.stringify(rows))) as unknown as typeof fetch;
  return () => {
    globalThis.fetch = original;
  };
}

describe('ThermalStrip', () => {
  test('왼쪽=과거·오른쪽=최신으로 뒤집고, 색은 두 대기를 따른다', async () => {
    const restore = mockHistory(EVENTS);
    try {
      await act(async () => {
        renderWithStore(<ThermalStrip />, {});
      });
      // 캡션 span 은 눈금이 아니다 — 눈금은 인라인 background 를 가진 것만.
      const ticks = [...document.querySelectorAll('[role="img"] span[style]')];
      expect(ticks.length).toBe(2);
      // API 최신순 → 화면은 뒤집혀 [사람(과거), 에이전트(최신)] 순.
      expect((ticks[0] as HTMLElement).style.background).toContain('--cool');
      expect((ticks[1] as HTMLElement).style.background).toContain('--warm');
      // 최신이 가장 진하다.
      expect((ticks[1] as HTMLElement).style.opacity).toBe('1');
      // 마지막 활동이 글자로도 보인다 — 눈금만으로는 무슨 뜻인지 안 읽혔다.
      expect(document.querySelector('.thermal-caption')?.textContent).toMatch(/^에이전트 · /);
    } finally {
      restore();
    }
  });

  test('히스토리가 비면 아무것도 그리지 않는다', async () => {
    const restore = mockHistory([]);
    try {
      await act(async () => {
        renderWithStore(<ThermalStrip />, {});
      });
      expect(document.querySelector('[role="img"]')).toBeNull();
    } finally {
      restore();
    }
  });

  test('캡션은 마지막 활동의 주체와 경과를 말한다', () => {
    const now = Date.parse('2026-08-16T12:05:00.000Z');
    // 화면 순서(왼쪽=과거)로 넘긴다 — 마지막 원소가 최신.
    expect(thermalCaption([OLDEST, NEWEST], now)).toBe('에이전트 · 5분 전');
    expect(thermalCaption([NEWEST, OLDEST], now)).toBe('사람 · 1시간 전');
    expect(thermalCaption([{ id: 3, actor: 'minjun', at: new Date(now).toISOString() }], now)).toBe(
      '사람 · 방금',
    );
    expect(thermalCaption([], now)).toBe('');
  });
});
