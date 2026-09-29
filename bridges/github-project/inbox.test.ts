import { describe, expect, test } from 'bun:test';
import { join } from 'node:path';

/**
 * GitHub 프로젝트 수집함 — `gh` 를 부르지 않고 `--from` 픽스처로 필터와 규약 변환만 본다.
 * 픽스처는 `QUERY` 의 응답 모양을 따른다.
 */
const script = join(import.meta.dir, 'inbox.ts');
const fixture = join(import.meta.dir, 'fixture.json');

function run(args: string[]) {
  const proc = Bun.spawnSync({
    cmd: ['bun', script, ...args],
    stdout: 'pipe',
    stderr: 'pipe',
    env: { PATH: process.env.PATH ?? '', HOME: '/nonexistent' },
  });
  return { code: proc.exitCode, out: proc.stdout.toString(), err: proc.stderr.toString() };
}

const ids = (out: string) =>
  (JSON.parse(out) as { items: { id: string }[] }).items.map((i) => i.id.split('/').pop());

describe('bridges/github-project/inbox.ts', () => {
  test('보드 필터(assignee:@me type:Bug component/s:Web)를 AND 로 — 열린 이슈만, 새 것이 위', () => {
    const r = run([
      '--from',
      fixture,
      '--assignee',
      '@me',
      '--type',
      'Bug',
      '--field',
      'Component/s=Web',
    ]);
    expect(r.err).toBe('');
    expect(r.code).toBe(0);
    // 3 닫힘 · 4 남의 것 · 5 다른 컴포넌트 · 6 초안 · 7 PR · 8 타입이 Task · 9 조직 레포 타입 미지정 제외
    // (라벨 대체는 개인 계정 레포인 2번에만)
    expect(ids(r.out)).toEqual(['2', '1']);
    const first = (JSON.parse(r.out) as { items: Record<string, string>[] }).items[0];
    expect(first).toEqual({
      id: 'https://github.com/someone/side/issues/2',
      title: '라벨로 버그인 이슈',
      url: 'https://github.com/someone/side/issues/2',
      note: 'someone/side#2',
      createdAt: '2026-09-29T01:00:00Z',
    });
  });

  test('조건이 없으면 열린 이슈 전부', () => {
    const r = run(['--from', fixture]);
    expect(r.code).toBe(0);
    expect(ids(r.out)).toEqual(['2', '1', '4', '5', '8', '9']);
  });

  test('필드 이름·값은 대소문자를 가리지 않는다', () => {
    const r = run(['--from', fixture, '--field', 'component/S=WEB', '--assignee', 'Me']);
    expect(ids(r.out)).toEqual(['2', '1', '8', '9']);
  });

  test('gh 가 없으면 소스 코드 조각이 아니라 이유 한 줄로 실패한다', () => {
    const proc = Bun.spawnSync({
      cmd: [process.execPath, script, '--project', 'o/1'],
      stdout: 'pipe',
      stderr: 'pipe',
      env: { PATH: '/nonexistent', HOME: '/nonexistent' },
    });
    const err = proc.stderr.toString();
    expect(proc.exitCode).toBe(1);
    expect(err.startsWith('github-project: gh 를 실행하지 못했다')).toBe(true);
    expect(err.trim().split('\n')).toHaveLength(1);
  });

  test('잘못된 인자는 exit 1 + stderr 한 줄', () => {
    for (const args of [
      ['--field', 'no-equals'],
      ['--project', 'nope'],
      ['--limit', '0'],
      ['--what', 'x'],
    ]) {
      const r = run(args);
      expect(r.code).toBe(1);
      expect(r.err.startsWith('github-project: ')).toBe(true);
      expect(r.err.trim().split('\n')).toHaveLength(1);
    }
  });
});
