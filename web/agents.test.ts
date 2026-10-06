import { describe, expect, test } from 'bun:test';
import { agentPhase, agentSections, boardOfSession, isSafeShortId, repoLabel } from './agents';
import type { Board, SessionRow, TodoView } from './types';

// `test-support` 는 스토어(=DOM 전역)를 끌고 와 순수 실행에서 못 쓴다 — 쓰는 필드만 채운다.
const boardFixture = (over: Partial<Board> = {}): Board => ({
  id: 'board1',
  key: 'rocky',
  title: 'rocky',
  createdAt: '2026-07-27T00:00:00.000Z',
  ...over,
});

const todoFixture = (over: Partial<TodoView> = {}): TodoView => ({
  id: 'todo1',
  number: 1,
  boardId: 'board1',
  title: '할 일',
  description: '',
  status: 'todo',
  priority: 'p4',
  labels: [],
  links: [],
  position: 0,
  createdAt: '2026-07-27T00:00:00.000Z',
  updatedAt: '2026-07-27T00:00:00.000Z',
  ref: 'rocky-1',
  commentCount: 0,
  ...over,
});

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
  test('보드는 경로 세그먼트(옛 key 포함)나 path 아래로 찾는다', () => {
    const boards = [
      boardFixture({ key: 'rocky', previousKeys: ['agent-toolkit'] }),
      boardFixture({ id: 'b2', key: 'tally', path: '/w/money' }),
    ];
    expect(boardOfSession('/w/rocky/.claude/worktrees/x', boards)?.key).toBe('rocky');
    expect(boardOfSession('/w/agent-toolkit', boards)?.key).toBe('rocky');
    expect(boardOfSession('/w/money/sub', boards)?.key).toBe('tally');
    expect(boardOfSession('/w/moneybag', boards)).toBeUndefined();
    expect(boardOfSession('/w/rocky-wt', boards)).toBeUndefined();
  });

  test('path 가 key 보다 먼저, 후보가 여럿이면 더 구체적인 쪽', () => {
    const outer = boardFixture({ id: 'o', key: 'ws', path: '/w' });
    const inner = boardFixture({ id: 'i', key: 'acorn', path: '/w/acorn' });
    const clash = boardFixture({ id: 'c', key: 'w' }); // 상위 폴더 이름과 우연히 같은 key
    expect(boardOfSession('/w/acorn/src', [outer, clash, inner])?.key).toBe('acorn');
    expect(boardOfSession('/w/other', [clash, outer])?.key).toBe('ws');
    const short = boardFixture({ id: 's', key: 'a' });
    const long = boardFixture({ id: 'l', key: 'acorn-server' });
    expect(boardOfSession('/x/a/acorn-server', [short, long])?.key).toBe('acorn-server');
  });

  test('보드가 없으면 레포 폴더 이름, 워크트리는 레포로 접는다', () => {
    expect(repoLabel('/w/acorn/.claude/worktrees/todo-25')).toBe('acorn');
    expect(repoLabel('/w/cc-usage')).toBe('cc-usage');
  });
});

describe('agentSections', () => {
  const boards = [boardFixture({ key: 'rocky' })];

  test('내 차례 → 실행 중 → 쉬는 중, 빈 묶음은 빠진다', () => {
    const sections = agentSections({
      sessions: [row(), dormant(), row({ sessionId: 'sess-2', status: 'busy' })],
      boards,
      selected: 'all',
      doing: [],
    });
    expect(sections.map((s) => [s.group, s.rows.length])).toEqual([
      ['mine', 1],
      ['run', 1],
      ['rest', 1],
    ]);
    expect(sections[0]?.rows[0]?.place).toBe('acorn');
    expect(sections[1]?.rows[0]?.place).toBe('rocky');
  });

  test('보드를 고르면 그 보드의 세션만', () => {
    const sections = agentSections({
      sessions: [row(), dormant()],
      boards,
      selected: 'rocky',
      doing: [],
    });
    expect(sections.flatMap((s) => s.rows.map((r) => r.session.name))).toEqual(['rocky-1']);
  });

  test('든 할 일은 짧은 id 로도 붙는다', () => {
    const todo = todoFixture({ status: 'doing', doingSessionId: '0da6a98a' });
    const [mine] = agentSections({ sessions: [dormant()], boards, selected: 'all', doing: [todo] });
    expect(mine?.rows[0]?.todo?.id).toBe(todo.id);
  });

  test('내 차례는 기다리기 시작한 때(요약을 고친 때)가 최근인 것이 위', () => {
    const older = dormant({ sessionId: 'a', job: { updatedAt: '2026-08-10T00:00:00Z' } });
    const newer = dormant({ sessionId: 'b', job: { updatedAt: '2026-10-05T00:00:00Z' } });
    const [mine] = agentSections({
      sessions: [older, newer],
      boards,
      selected: 'all',
      doing: [],
    });
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
