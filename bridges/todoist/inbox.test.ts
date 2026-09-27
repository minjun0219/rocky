import { describe, expect, test } from 'bun:test';
import { join } from 'node:path';

/**
 * Todoist 어댑터 — API 를 부르지 않고 `--from` 픽스처로 규약 변환만 본다. 토큰 경로(op read)는
 * 자격이 필요해 테스트에 넣지 않는다 — 대신 "토큰이 없으면 exit 1 + stderr 한 줄" 만 고정한다.
 */
const script = join(import.meta.dir, 'inbox.py');
const fixture = join(import.meta.dir, 'fixture.json');

function run(args: string[], env: Record<string, string> = {}) {
  const proc = Bun.spawnSync({
    cmd: ['python3', script, ...args],
    stdout: 'pipe',
    stderr: 'pipe',
    env: { PATH: process.env.PATH ?? '', HOME: '/nonexistent', ...env },
  });
  return { code: proc.exitCode, out: proc.stdout.toString(), err: proc.stderr.toString() };
}

describe('bridges/todoist/inbox.py', () => {
  test('픽스처를 규약 item 으로 바꾼다 — 완료·빈 제목 제외, due 는 날짜만, 링크 자동', () => {
    const r = run(['--from', fixture]);
    expect(r.err).toBe('');
    expect(r.code).toBe(0);
    const { items } = JSON.parse(r.out) as { items: Record<string, string>[] };
    expect(items.map((i) => i.id)).toEqual(['6XGgmFVcrG5RRjVr', '6fFPHV272WWh3gpW', '6fFPHNoDue']);
    expect(items[0]).toEqual({
      id: '6XGgmFVcrG5RRjVr',
      title: 'Buy milk',
      url: 'https://app.todoist.com/app/task/6XGgmFVcrG5RRjVr',
      note: 'Pick up organic milk',
      due: '2025-02-12',
      createdAt: '2025-01-15T10:30:00Z',
    });
    // datetime due → YYYY-MM-DD, 제목 trim, 빈 description 은 생략
    expect(items[1]).toEqual({
      id: '6fFPHV272WWh3gpW',
      title: 'Datetime due, no description',
      url: 'https://app.todoist.com/app/task/6fFPHV272WWh3gpW',
      due: '2025-02-13',
      createdAt: '2025-01-16T08:00:00Z',
    });
    expect(items[2]).toEqual({
      id: '6fFPHNoDue',
      title: 'No due, no added_at',
      url: 'https://app.todoist.com/app/task/6fFPHNoDue',
    });
  });

  test('출력이 수집함 규약을 지킨다 — id·title 필수, due 는 YYYY-MM-DD, createdAt 은 RFC 3339', () => {
    // 데몬(rocky_core::inbox::parse_inbox_output)이 거부하는 모양이면 소스 전체가 실패한다 —
    // 서비스 이름을 crates/ 테스트에 넣지 않기로 했으니(AGENTS.md Scope) 규약 검사는 여기서 한다.
    const { items } = JSON.parse(run(['--from', fixture]).out) as {
      items: Record<string, unknown>[];
    };
    expect(items.length).toBeGreaterThan(0);
    for (const item of items) {
      expect(typeof item.id).toBe('string');
      expect((item.id as string).length).toBeGreaterThan(0);
      expect(typeof item.title).toBe('string');
      expect((item.title as string).trim()).toBe(item.title);
      if (item.due !== undefined) {
        expect(item.due).toMatch(/^\d{4}-\d{2}-\d{2}$/);
      }
      if (item.createdAt !== undefined) {
        expect(item.createdAt).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}/);
      }
      if (item.url !== undefined) {
        expect(item.url).toMatch(/^https:\/\//);
      }
      expect(
        Object.keys(item).every((k) =>
          ['id', 'title', 'url', 'note', 'due', 'createdAt'].includes(k),
        ),
      ).toBe(true);
    }
  });

  test('--limit 이 항목 수를 자른다', () => {
    const r = run(['--from', fixture, '--limit', '1']);
    expect(JSON.parse(r.out).items).toHaveLength(1);
  });

  test('토큰이 없으면 exit 1 + stderr 한 줄, stdout 비움', () => {
    const r = run([]);
    expect(r.code).toBe(1);
    expect(r.out).toBe('');
    expect(r.err).toMatch(/^todoist: 토큰이 없다/);
    expect(r.err.trim().split('\n')).toHaveLength(1);
  });

  test('--op 인데 서비스 계정 토큰 파일이 없으면 값을 찍지 않고 실패한다', () => {
    const r = run(['--op', 'op://Agent Vault/deadbeef/credential']);
    expect(r.code).toBe(1);
    expect(r.err).toMatch(/op 서비스 계정 토큰 파일이 없다/);
  });

  test('--from 파일이 없으면 실패', () => {
    const r = run(['--from', '/no/such/file.json']);
    expect(r.code).toBe(1);
    expect(r.err).toMatch(/--from 읽기 실패/);
  });
});
