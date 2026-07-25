import { beforeEach, describe, expect, it } from 'bun:test';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import type { OpencodeExecutor, OpencodeRunResult } from './opencode-cli';
import { JobStore } from './opencode-jobs';
import { killJobProcess, runJob } from './opencode-runner';

let store: JobStore;

function fake(result: Partial<OpencodeRunResult> | (() => never)): OpencodeExecutor {
  return {
    async run() {
      if (typeof result === 'function') {
        result();
      }
      return { stdout: '', stderr: '', exitCode: 0, ...result };
    },
  };
}

function seed() {
  return store.create({
    title: '테스트 잡',
    workspaceRoot: '/repo',
    request: { prompt: '구현해줘', worktree: '/repo/wt', model: 'anthropic/claude-sonnet-5' },
  });
}

beforeEach(() => {
  store = new JobStore({ dir: mkdtempSync(join(tmpdir(), 'rocky-runner-')) });
});

describe('runJob 성공 경로', () => {
  it('은 completed 로 끝나고 결과 텍스트를 남긴다', async () => {
    const job = seed();
    const done = await runJob(store, job.id, fake({ stdout: '{"type":"text","text":"다 했어"}' }));
    expect(done.status).toBe('completed');
    expect(done.phase).toBe('done');
    expect(done.result).toBe('다 했어');
    expect(done.exitCode).toBe(0);
    expect(done.completedAt).toBeTruthy();
    expect(done.errorMessage).toBeUndefined();
  });

  it('은 opencode 세션 id 를 sessionRef 로 보관한다', async () => {
    const job = seed();
    const done = await runJob(
      store,
      job.id,
      fake({ stdout: '{"type":"session.start","sessionID":"ses_9"}\n{"type":"text","text":"ok"}' }),
    );
    expect(done.sessionRef).toBe('ses_9');
  });

  it('은 실행 전에 running / startedAt / pid 를 기록한다', async () => {
    const job = seed();
    let observed: string | undefined;
    await runJob(store, job.id, {
      async run() {
        observed = store.get(job.id)?.status;
        return { stdout: '', stderr: '', exitCode: 0 };
      },
    });
    expect(observed).toBe('running');
    expect(store.get(job.id)?.startedAt).toBeTruthy();
    expect(store.get(job.id)?.pid).toBe(process.pid);
  });

  // childPid 를 기록하지 않으면 cancel / SessionEnd 가 opencode 프로세스 그룹을 못 찾는다.
  it('은 onSpawn 으로 받은 opencode pid 를 잡에 기록한다', async () => {
    const job = seed();
    await runJob(store, job.id, {
      async run(_args, options) {
        options?.onSpawn?.(31337);
        return { stdout: '{"type":"text","text":"ok"}', stderr: '', exitCode: 0 };
      },
    });
    expect(store.get(job.id)?.childPid).toBe(31337);
    expect(store.readLogTail(job.id, 10).join('\n')).toContain('31337');
  });

  it('은 진행 로그를 남긴다', async () => {
    const job = seed();
    await runJob(store, job.id, fake({ stdout: '{"type":"text","text":"ok"}' }));
    const log = store.readLogTail(job.id, 10).join('\n');
    expect(log).toContain('opencode run 시작');
    expect(log).toContain('opencode 종료');
  });

  it('은 프롬프트 본문을 로그에 노출하지 않는다', async () => {
    const job = seed();
    await runJob(store, job.id, fake({ stdout: '{"type":"text","text":"ok"}' }));
    const log = store.readLogTail(job.id, 10).join('\n');
    expect(log).not.toContain('구현해줘');
    expect(log).toContain('<prompt>');
  });
});

describe('runJob 실패 경로', () => {
  it('은 non-zero 종료를 failed 로 기록한다', async () => {
    const job = seed();
    const done = await runJob(store, job.id, fake({ exitCode: 3, stderr: '터졌다' }));
    expect(done.status).toBe('failed');
    expect(done.exitCode).toBe(3);
    expect(done.errorMessage).toContain('터졌다');
  });

  it('은 exit 0 이어도 에러 이벤트가 있으면 failed 로 본다', async () => {
    const job = seed();
    const done = await runJob(
      store,
      job.id,
      fake({ stdout: '{"type":"error","error":{"message":"model not found"}}', exitCode: 0 }),
    );
    expect(done.status).toBe('failed');
    expect(done.errorMessage).toBe('model not found');
  });

  it('은 executor 예외를 던지지 않고 잡에 기록한다', async () => {
    const job = seed();
    const done = await runJob(
      store,
      job.id,
      fake(() => {
        throw new Error('시간 초과');
      }),
    );
    expect(done.status).toBe('failed');
    expect(done.errorMessage).toContain('시간 초과');
    expect(done.completedAt).toBeTruthy();
  });

  it('은 없는 잡에 대해서만 던진다', async () => {
    await expect(runJob(store, 'oc-nope', fake({}))).rejects.toThrow(/oc-nope/);
  });
});

describe('killJobProcess', () => {
  it('은 프로세스 그룹에 신호를 보낸다', () => {
    const seen: number[] = [];
    const outcome = killJobProcess(4242, 'SIGTERM', (target) => {
      seen.push(target);
    });
    expect(outcome.killed).toBe(true);
    expect(seen).toEqual([-4242]);
  });

  // opencode 는 워커와 **별도 프로세스 그룹**으로 뜬다 (타임아웃 때 손자까지 끊으려고).
  // 워커 그룹만 죽이면 opencode 가 고아로 남아 worktree 를 계속 수정한다.
  it('은 워커 그룹과 opencode 그룹을 모두 끊는다', () => {
    const seen: number[] = [];
    const outcome = killJobProcess({ pid: 100, childPid: 200 }, 'SIGTERM', (target) => {
      seen.push(target);
    });
    expect(outcome.killed).toBe(true);
    expect(seen).toEqual([-100, -200]);
    expect(outcome.detail).toContain('worker');
    expect(outcome.detail).toContain('opencode');
  });

  it('은 childPid 만 있어도 그 그룹을 끊는다', () => {
    const seen: number[] = [];
    killJobProcess({ childPid: 200 }, 'SIGTERM', (target) => {
      seen.push(target);
    });
    expect(seen).toEqual([-200]);
  });

  it('은 한쪽이 이미 죽어도 다른 쪽을 계속 끊는다', () => {
    const seen: number[] = [];
    const outcome = killJobProcess({ pid: 100, childPid: 200 }, 'SIGTERM', (target) => {
      seen.push(target);
      if (target === -100 || target === 100) {
        throw new Error('ESRCH');
      }
    });
    expect(seen).toEqual([-100, 100, -200]);
    expect(outcome.killed).toBe(true);
  });

  it('은 그룹이 없으면 단일 프로세스로 재시도한다', () => {
    const seen: number[] = [];
    const outcome = killJobProcess(4242, 'SIGTERM', (target) => {
      seen.push(target);
      if (target < 0) {
        throw new Error('ESRCH');
      }
    });
    expect(outcome.killed).toBe(true);
    expect(seen).toEqual([-4242, 4242]);
    expect(outcome.detail).toContain('그룹 없음');
  });

  it('은 둘 다 실패하면 killed false 를 준다', () => {
    const outcome = killJobProcess(4242, 'SIGTERM', () => {
      throw new Error('ESRCH');
    });
    expect(outcome.killed).toBe(false);
    expect(outcome.detail).toContain('ESRCH');
  });

  it('은 pid 가 없으면 아무것도 하지 않는다', () => {
    expect(killJobProcess(undefined).killed).toBe(false);
    expect(killJobProcess(0).killed).toBe(false);
    expect(killJobProcess({}).killed).toBe(false);
  });
});
