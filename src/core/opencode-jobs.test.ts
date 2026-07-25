import { beforeEach, describe, expect, it } from 'bun:test';
import { existsSync, mkdtempSync, unlinkSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  DEFAULT_JOBS_ROOT,
  JobStore,
  createJobStoreFromEnv,
  filterBySession,
  matchJobReference,
  newJobId,
  resolveDefaultJobsDir,
} from './opencode-jobs';

let dir: string;
let store: JobStore;

function seed(title: string, extra: Record<string, unknown> = {}) {
  return store.create({
    title,
    workspaceRoot: '/repo',
    request: { prompt: title, worktree: '/repo/wt' },
    ...extra,
  });
}

beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'rocky-jobs-'));
  store = new JobStore({ dir });
});

describe('newJobId', () => {
  it('은 prefix 와 두 세그먼트를 가진다', () => {
    const id = newJobId('oc');
    expect(id.startsWith('oc-')).toBe(true);
    expect(id.split('-')).toHaveLength(3);
  });

  it('은 연속 호출에서 충돌하지 않는다', () => {
    const ids = new Set(Array.from({ length: 200 }, () => newJobId('oc')));
    expect(ids.size).toBe(200);
  });
});

describe('JobStore.create / get', () => {
  it('은 queued 상태의 레코드를 만들고 다시 읽어온다', () => {
    const job = seed('첫 잡');
    expect(job.status).toBe('queued');
    expect(job.kind).toBe('task');
    expect(job.createdAt).toBeTruthy();

    const loaded = store.get(job.id);
    expect(loaded?.title).toBe('첫 잡');
    expect(loaded?.request.prompt).toBe('첫 잡');
  });

  it('은 payload 파일과 로그 파일 경로를 잡 디렉터리 아래에 둔다', () => {
    const job = seed('경로');
    expect(existsSync(join(dir, 'jobs', `${job.id}.json`))).toBe(true);
    expect(job.logFile).toBe(join(dir, 'jobs', `${job.id}.log`));
  });

  it('은 없는 id 에 대해 null 을 준다', () => {
    expect(store.get('oc-nope-000000')).toBeNull();
  });
});

describe('JobStore.list', () => {
  it('은 최신순으로 정렬한다', () => {
    const a = seed('a');
    const b = seed('b');
    const ids = store.list().map((j) => j.id);
    expect(ids[0]).toBe(b.id);
    expect(ids[1]).toBe(a.id);
  });

  it('은 payload 파일이 사라져도 인덱스 정보로 버틴다', () => {
    const job = seed('a');
    unlinkSync(join(dir, 'jobs', `${job.id}.json`));
    const listed = store.list();
    expect(listed).toHaveLength(1);
    expect(listed[0]?.id).toBe(job.id);
  });
});

describe('JobStore.update', () => {
  it('은 필드를 패치하고 updatedAt 을 갱신한다', async () => {
    const job = seed('a');
    await Bun.sleep(2);
    const updated = store.update(job.id, { status: 'running', phase: 'dispatch', pid: 4242 });
    expect(updated.status).toBe('running');
    expect(updated.phase).toBe('dispatch');
    expect(updated.pid).toBe(4242);
    expect(updated.updatedAt > job.updatedAt).toBe(true);
    expect(store.get(job.id)?.status).toBe('running');
  });

  it('은 인덱스에도 상태를 반영한다', () => {
    const job = seed('a');
    store.update(job.id, { status: 'completed' });
    expect(store.list()[0]?.status).toBe('completed');
  });

  it('은 없는 잡에 대해 에러를 던진다', () => {
    expect(() => store.update('oc-missing-1', { status: 'failed' })).toThrow(/oc-missing-1/);
  });
});

describe('JobStore 로그', () => {
  it('은 append 한 줄을 tail 로 되읽는다', () => {
    const job = seed('a');
    store.appendLog(job.id, 'first');
    store.appendLog(job.id, 'second');
    const tail = store.readLogTail(job.id, 1);
    expect(tail).toHaveLength(1);
    expect(tail[0]).toContain('second');
  });

  it('은 로그가 없으면 빈 배열을 준다', () => {
    const job = seed('a');
    expect(store.readLogTail(job.id, 5)).toEqual([]);
  });
});

describe('JobStore.prune', () => {
  it('은 maxJobs 를 넘는 오래된 잡과 그 파일을 지운다', () => {
    const small = new JobStore({ dir, maxJobs: 2 });
    const a = small.create({
      title: 'a',
      workspaceRoot: '/r',
      request: { prompt: 'a', worktree: '/r' },
    });
    small.appendLog(a.id, 'x');
    small.create({ title: 'b', workspaceRoot: '/r', request: { prompt: 'b', worktree: '/r' } });
    small.create({ title: 'c', workspaceRoot: '/r', request: { prompt: 'c', worktree: '/r' } });

    expect(small.list()).toHaveLength(2);
    expect(small.get(a.id)).toBeNull();
    expect(existsSync(join(dir, 'jobs', `${a.id}.json`))).toBe(false);
    expect(existsSync(join(dir, 'jobs', `${a.id}.log`))).toBe(false);
  });
});

describe('matchJobReference', () => {
  it('은 정확한 id 를 찾는다', () => {
    const a = seed('a');
    seed('b');
    expect(matchJobReference(store.list(), a.id).id).toBe(a.id);
  });

  it('은 유일한 prefix 를 채택한다', () => {
    const a = seed('a');
    expect(matchJobReference(store.list(), a.id.slice(0, 10)).id).toBe(a.id);
  });

  it('은 모호한 prefix 에 대해 에러를 던진다', () => {
    seed('a');
    seed('b');
    expect(() => matchJobReference(store.list(), 'oc-')).toThrow(/모호|ambiguous/i);
  });

  it('은 ref 가 없으면 최신 잡을 준다', () => {
    seed('a');
    const b = seed('b');
    expect(matchJobReference(store.list(), undefined).id).toBe(b.id);
  });

  it('은 후보가 없으면 에러를 던진다', () => {
    expect(() => matchJobReference([], undefined)).toThrow();
    seed('a');
    expect(() => matchJobReference(store.list(), 'zzz')).toThrow(/zzz/);
  });
});

describe('filterBySession', () => {
  it('은 같은 Claude 세션의 잡만 남긴다', () => {
    const mine = seed('mine', { sessionId: 's1' });
    seed('theirs', { sessionId: 's2' });
    const filtered = filterBySession(store.list(), 's1');
    expect(filtered.map((j) => j.id)).toEqual([mine.id]);
  });

  it('은 세션 id 가 없으면 전부 남긴다', () => {
    seed('a', { sessionId: 's1' });
    seed('b', { sessionId: 's2' });
    expect(filterBySession(store.list(), undefined)).toHaveLength(2);
  });

  it('은 필터 결과가 비면 세션 없는 잡으로 폴백하지 않는다', () => {
    seed('a', { sessionId: 's2' });
    expect(filterBySession(store.list(), 's1')).toHaveLength(0);
  });
});

describe('경로 해석', () => {
  it('은 기본 루트를 ~/.config/rocky/jobs 아래 프로젝트별로 잡는다', () => {
    expect(DEFAULT_JOBS_ROOT).toBe(join(homedir(), '.config', 'rocky', 'jobs'));
    const resolved = resolveDefaultJobsDir('/tmp/some-project');
    expect(resolved.startsWith(DEFAULT_JOBS_ROOT)).toBe(true);
    expect(resolved).not.toBe(DEFAULT_JOBS_ROOT);
  });

  it('은 env 로 통째 덮어쓸 수 있다', () => {
    const s = createJobStoreFromEnv(undefined, { ROCKY_OPENCODE_JOBS_DIR: dir });
    expect(s.dir).toBe(dir);
  });

  it('은 config.dir 의 ~ 를 확장한다', () => {
    const s = createJobStoreFromEnv({ dir: '~/rocky-jobs-test' }, {});
    expect(s.dir).toBe(join(homedir(), 'rocky-jobs-test'));
  });
});
