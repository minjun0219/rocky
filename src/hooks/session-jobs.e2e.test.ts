import { beforeEach, describe, expect, it } from 'bun:test';
import { mkdtempSync, unlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { JobStore } from '../core/opencode-jobs';

/**
 * `SessionEnd` 훅을 실제 서브프로세스로 돌리는 테스트.
 *
 * 훅은 stdin 으로 JSON 을 받아 `process.exit(0)` 으로 끝나므로, 계약(고아 정리 + fail-open)을
 * 확인하려면 실행해 보는 편이 정확하다.
 */

const HOOK = join(import.meta.dir, 'session-jobs.ts');

// 존재하지 않을 가능성이 높은 pid — 실제 프로세스를 죽이지 않기 위해서다.
// kill 이 실패해도 잡은 cancelled 로 기록되어야 한다.
const DEAD_PID = 2_147_400_000;

let dir: string;
let store: JobStore;

function runHook(event: string, input: Record<string, unknown>) {
  const proc = Bun.spawn([process.execPath, HOOK, event], {
    env: { ...process.env, ROCKY_OPENCODE_JOBS_DIR: dir },
    stdin: new TextEncoder().encode(JSON.stringify(input)),
    stdout: 'pipe',
    stderr: 'pipe',
  });
  return (async () => ({
    stdout: await new Response(proc.stdout).text(),
    exitCode: await proc.exited,
  }))();
}

function running(title: string, sessionId: string) {
  const job = store.create({
    title,
    workspaceRoot: '/repo',
    request: { prompt: title, worktree: '/repo/wt' },
    sessionId,
  });
  return store.update(job.id, { status: 'running', pid: DEAD_PID });
}

beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'rocky-hook-'));
  store = new JobStore({ dir });
});

describe('SessionEnd 정리', () => {
  it('은 이 세션의 진행 중 잡을 cancelled 로 기록한다', async () => {
    const job = running('내 잡', 'sess-a');
    const out = await runHook('SessionEnd', { session_id: 'sess-a', cwd: '/repo' });
    expect(out.exitCode).toBe(0);
    expect(store.get(job.id)?.status).toBe('cancelled');
  });

  it('은 다른 세션의 잡은 건드리지 않는다', async () => {
    const mine = running('내 잡', 'sess-a');
    const theirs = running('남의 잡', 'sess-b');
    await runHook('SessionEnd', { session_id: 'sess-a', cwd: '/repo' });
    expect(store.get(mine.id)?.status).toBe('cancelled');
    expect(store.get(theirs.id)?.status).toBe('running');
  });

  it('은 이미 끝난 잡을 다시 건드리지 않는다', async () => {
    const job = running('끝난 잡', 'sess-a');
    store.update(job.id, { status: 'completed' });
    await runHook('SessionEnd', { session_id: 'sess-a', cwd: '/repo' });
    expect(store.get(job.id)?.status).toBe('completed');
  });

  // 한 잡이 깨졌다고 루프가 멈추면, 고아를 막으려고 만든 훅이 정작 고아를 남긴다.
  it('은 payload 가 없는 잡 때문에 나머지 정리를 멈추지 않는다', async () => {
    // `list()` 는 최신순이므로 **깨진 잡을 나중에** 만들어야 루프에서 먼저 만난다 —
    // 순서가 반대면 멀쩡한 잡이 이미 정리된 뒤라 회귀를 잡지 못한다.
    const healthy = running('멀쩡한 잡', 'sess-a');
    const broken = running('깨진 잡', 'sess-a');
    unlinkSync(join(dir, 'jobs', `${broken.id}.json`));

    const out = await runHook('SessionEnd', { session_id: 'sess-a', cwd: '/repo' });
    expect(out.exitCode).toBe(0);
    expect(store.get(healthy.id)?.status).toBe('cancelled');
  });

  it('은 세션 id 가 없으면 아무 잡도 건드리지 않는다', async () => {
    const job = running('내 잡', 'sess-a');
    await runHook('SessionEnd', { cwd: '/repo' });
    expect(store.get(job.id)?.status).toBe('running');
  });

  it('은 깨진 stdin 에도 fail-open 한다', async () => {
    const proc = Bun.spawn([process.execPath, HOOK, 'SessionEnd'], {
      env: { ...process.env, ROCKY_OPENCODE_JOBS_DIR: dir },
      stdin: new TextEncoder().encode('not json'),
      stdout: 'ignore',
      stderr: 'ignore',
    });
    expect(await proc.exited).toBe(0);
  });
});
