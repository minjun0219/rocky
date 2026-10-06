// lab 의 순수 판정 — `bun run test`(CI)가 돈다. 엔진이 필요한 테스트는 register.test.tsx(`bun run test:lab`).
import { describe, expect, test } from 'bun:test';

import { limitsLine, parseLabConfig, rockyToast, summaryParts } from './lib';

describe('parseLabConfig', () => {
  // crates/rocky-core/tests/it/config_test.rs::lab_block_turns_on_only_when_present 와 같은 케이스.
  test('블록이 있어야 켜지고 enabled:false 면 꺼진다', () => {
    expect(parseLabConfig(undefined)).toBeUndefined();
    expect(parseLabConfig('{ not json')).toBeUndefined();
    expect(parseLabConfig('{"rc":{}}')).toBeUndefined();
    expect(parseLabConfig('{"lab":true}')).toBeUndefined();
    expect(parseLabConfig('{"lab":{"enabled":false}}')).toBeUndefined();
    expect(parseLabConfig('{"lab":{}}')).toEqual({ toast: true, band: true, limits: true });
    expect(parseLabConfig('{"lab":{"band":false}}')).toEqual({
      toast: true,
      band: false,
      limits: true,
    });
  });
});

describe('rockyToast', () => {
  // 머리 모양은 rocky_core::peer_inbox(pr_session_message 등)·handoff::build_handoff_poke 가 정한다 — 그쪽 테스트도 고정한다.
  test('데몬 메시지의 첫 줄만, 레포 접두는 떼고', () => {
    expect(
      rockyToast('rocky: minjun0219/rocky #391 머지 후보 — feat(cli): doctor\nhttps://…\n\n본문'),
    ).toBe('rocky · #391 머지 후보 — feat(cli): doctor');
    expect(rockyToast('# rocky: 보드에서 작업 요청이 도착했다 — rocky-41 "제목"\n\n…')).toBe(
      'rocky · 보드에서 작업 요청이 도착했다 — rocky-41 "제목"',
    );
    expect(rockyToast('rocky: 구독한 수집함 `todoist` 에 새 항목 2건\n- …')).toBe(
      'rocky · 구독한 수집함 `todoist` 에 새 항목 2건',
    );
    expect(rockyToast('다른 세션이 보낸 메시지 rocky: 아님')).toBeUndefined();
  });
});

test('한도 줄', () => {
  const limits = [
    { kind: 'five_hour', percentUsed: 42 },
    { kind: 'seven_day', percentUsed: 18 },
  ];
  expect(limitsLine(limits, 31)).toBe('rocky lab 5h 42% · 7d 18% · ctx 31%');
  expect(limitsLine([], undefined)).toBeUndefined();
});

test('요약 칸은 0 과 빠진 칸을 뺀다', () => {
  expect(summaryParts({ doing: 2, handoffsOpen: 0, overdue: 1, collect: 3 })).toEqual([
    '진행중 2',
    '기한 지남 1',
    '수집함 3',
  ]);
  expect(summaryParts({ doing: 0, handoffsOpen: 0, overdue: 0 })).toEqual([]);
});
