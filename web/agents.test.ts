import { describe, expect, test } from 'bun:test';
import { agentPhase, agentSections, isSafeShortId, repoLabel } from './agents';
import type { SessionRow } from './types';

// `test-support` 는 스토어(=DOM 전역)를 끌고 와 순수 실행에서 못 쓴다 — 쓰는 필드만 채운다.
const row = (over: Partial<SessionRow> = {}): SessionRow => ({
  pid: 1,
  cwd: '/w/rocky',
  kind: 'interactive',
  sessionId: 'sess-1',
  name: 'rocky-1',
  status: 'idle',
  startedAt: Date.parse('2026-10-06T00:00:00Z'),
  matched: false,
  ...over,
});

const dormant = (over: Partial<SessionRow> = {}): SessionRow =>
  row({
    pid: undefined,
    kind: 'background',
    id: '0da6a98a',
    sessionId: '0da6a98a-full',
    name: 'acorn-25',
    // 잠든 background 행의 cwd 는 워크트리가 아니라 레포 루트다(Claude Code 2.1.289).
    cwd: '/w/acorn',
    state: 'blocked',
    ...over,
  });

describe('agentPhase', () => {
  test('blocked 는 내 차례, busy·working 은 실행 중, done 은 끝남, 나머지는 쉬는 중', () => {
    expect(agentPhase(dormant())).toBe('blocked');
    expect(agentPhase(row({ status: 'busy' }))).toBe('run');
    expect(agentPhase(dormant({ state: 'working' }))).toBe('run');
    expect(agentPhase(dormant({ state: 'done' }))).toBe('done');
    expect(agentPhase(row())).toBe('idle');
  });
});

describe('어디서 도나', () => {
  test('레포 폴더 이름, 워크트리는 레포로 접는다', () => {
    expect(repoLabel('/w/acorn/.claude/worktrees/todo-25')).toBe('acorn');
    expect(repoLabel('/w/cc-usage')).toBe('cc-usage');
  });
});

describe('agentSections', () => {
  test('내 차례 → 실행 중 → 쉬는 중, 빈 묶음은 빠진다', () => {
    const sections = agentSections([
      row(),
      dormant(),
      row({ sessionId: 'sess-2', status: 'busy' }),
    ]);
    expect(sections.map((s) => [s.group, s.rows.length])).toEqual([
      ['mine', 1],
      ['run', 1],
      ['rest', 1],
    ]);
    expect(sections[0]?.rows[0]?.place).toBe('acorn');
    expect(sections[1]?.rows[0]?.place).toBe('rocky');
  });

  test('내 차례는 기다리기 시작한 때(요약을 고친 때)가 최근인 것이 위', () => {
    const older = dormant({ sessionId: 'a', job: { updatedAt: '2026-08-10T00:00:00Z' } });
    const newer = dormant({ sessionId: 'b', job: { updatedAt: '2026-10-05T00:00:00Z' } });
    const [mine] = agentSections([older, newer]);
    expect(mine?.rows.map((r) => r.session.sessionId)).toEqual(['b', 'a']);
  });
});

describe('isSafeShortId', () => {
  test('영숫자 64자 이하만 — 셸 명령으로 복사하기 전에 거른다', () => {
    expect(isSafeShortId('0da6a98a')).toBe(true);
    expect(isSafeShortId('a'.repeat(64))).toBe(true);
    for (const bad of ['', 'a'.repeat(65), '../x', 'a b', 'aa;rm', '-rf', 'a\nb']) {
      expect(isSafeShortId(bad)).toBe(false);
    }
  });
});
