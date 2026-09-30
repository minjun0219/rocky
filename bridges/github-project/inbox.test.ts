import { describe, expect, test } from 'bun:test';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { applyFilter, searchQuery } from './inbox';

/**
 * GitHub 프로젝트 수집함 — `gh` 를 부르지 않고 `--from` 픽스처로 필터와 규약 변환만 본다.
 * 픽스처는 `QUERY` 의 응답(검색 페이지 배열) 모양을 따른다.
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
  test('검색 결과에서 이 보드의 항목만 골라 --field 를 AND 로 — 둘째 페이지도, 새 것이 위', () => {
    const r = run(['--from', fixture, '--project', 'acme/7', '--field', 'Component/s=Web']);
    expect(r.err).toBe('');
    expect(r.code).toBe(0);
    // 2 다른 컴포넌트 · 3 다른 보드에만 · 4 같은 번호 다른 주인 — 5 는 둘째 페이지(보드 앞 100개 밖)
    expect(ids(r.out)).toEqual(['5', '1']);
    const first = (JSON.parse(r.out) as { items: Record<string, string>[] }).items[0];
    expect(first).toEqual({
      id: 'https://github.com/acme/web/issues/5',
      title: '둘째 페이지의 웹 버그',
      url: 'https://github.com/acme/web/issues/5',
      note: 'acme/web#5',
      createdAt: '2026-09-29T01:00:00Z',
    });
  });

  test('필드 조건이 없으면 이 보드에 있는 이슈 전부', () => {
    const r = run(['--from', fixture, '--project', 'acme/7']);
    expect(ids(r.out)).toEqual(['5', '2', '1']);
  });

  test('필드 이름·값은 대소문자를 가리지 않는다', () => {
    const r = run(['--from', fixture, '--project', 'acme/7', '--field', 'component/S=WEB']);
    expect(ids(r.out)).toEqual(['5', '1']);
  });

  test('잘린 결과는 성공으로 보고하지 않는다 — 검색 페이지 누락·보드 항목 넘침·필드 값 넘침', () => {
    const dir = mkdtempSync(join(tmpdir(), 'gh-project-'));
    const pages = JSON.parse(readFileSync(fixture, 'utf8')) as unknown[];
    const onlyFirst = join(dir, 'first.json');
    writeFileSync(onlyFirst, JSON.stringify([pages[0]]));
    // 2번 이슈는 필드 값이 넘쳤다 — "Status" 가 잘린 뒤쪽에 있었을 수 있다.
    type Page = {
      data: {
        search: {
          nodes: {
            projectItems: { nodes: { fieldValues: { pageInfo: { hasNextPage: boolean } } }[] };
          }[];
        };
      };
    };
    const cutPages = JSON.parse(readFileSync(fixture, 'utf8')) as Page[];
    cutPages[0].data.search.nodes[1].projectItems.nodes[0].fieldValues.pageInfo.hasNextPage = true;
    const fieldsCut = join(dir, 'fields.json');
    writeFileSync(fieldsCut, JSON.stringify(cutPages));
    for (const [file, reason] of [
      [onlyFirst, '검색 결과가 잘렸다'],
      [join(import.meta.dir, 'fixture-cut.json'), '보드 5개 넘게'],
    ]) {
      const r = run(['--from', file, '--project', 'acme/7']);
      expect(r.code).toBe(1);
      expect(r.err).toContain(reason);
      expect(r.err.trim().split('\n')).toHaveLength(1);
    }
    const r = run(['--from', fieldsCut, '--project', 'acme/7', '--field', 'Status=Todo']);
    expect(r.code).toBe(1);
    expect(r.err).toContain('"Status" 을 판정하지 못했다');
  });

  test('--filter 는 보드 필터 문자열을 그대로 — 필드·담당자·타입으로 풀린다', () => {
    const ok = run(['--from', fixture, '--project', 'acme/7', '--filter', 'component/s:Web']);
    expect(ids(ok.out)).toEqual(['5', '1']);
    const filters = { fields: [] as { name: string; value: string }[] } as Parameters<
      typeof applyFilter
    >[0];
    applyFilter(filters, 'assignee:@me type:"Feature request" due-date:2026-10-01');
    expect(filters).toEqual({
      assignee: '@me',
      type: 'Feature request',
      fields: [{ name: 'due-date', value: '2026-10-01' }],
    });
  });

  test('--filter 가 못 하는 문법은 조용히 버리지 않고 실패한다', () => {
    for (const filter of [
      '-type:Bug',
      'type:Bug,Task',
      'crash',
      'is:closed',
      'type:A type:B',
      'a:"b',
    ]) {
      const r = run(['--from', fixture, '--project', 'acme/7', '--filter', filter]);
      expect(r.code).toBe(1);
      expect(r.err.startsWith('github-project: ')).toBe(true);
    }
  });

  test('--describe 는 입력 칸 목록만 — 다른 인자 없이', () => {
    const r = run(['--describe']);
    expect(r.code).toBe(0);
    const d = JSON.parse(r.out) as { params: { flag: string; required: boolean }[] };
    expect(d.params.map((p) => p.flag)).toEqual(['--project', '--filter']);
    expect(d.params.find((p) => p.flag === '--project')?.required).toBe(true);
  });

  test('검색어 — 조직은 이슈 타입, 개인 계정은 같은 이름의 라벨, 공백 값은 따옴표', () => {
    const filters = { assignee: '@me', type: 'Feature request', fields: [] };
    expect(searchQuery('acme', false, filters)).toBe(
      'user:acme is:issue is:open assignee:@me type:"Feature request"',
    );
    expect(searchQuery('me', true, { type: 'Bug', fields: [] })).toBe(
      'user:me is:issue is:open label:Bug',
    );
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
