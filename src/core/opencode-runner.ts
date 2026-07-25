import {
  type OpencodeExecutor,
  buildRunArgs,
  killProcessGroup,
  renderRunOutput,
  summarizeRunEvents,
} from './opencode-cli';
import type { JobRecord, JobStore } from './opencode-jobs';

/**
 * 잡 하나를 실제로 실행한다 — foreground 실행과 detached 워커가 **같은 코드**를 쓴다.
 * 두 실행 모드는 "누가 이 함수를 부르는가" 만 다르고 잡 기록 / 로그 / 상태 전이는 동일하다.
 *
 * 예외를 밖으로 던지지 않는 것이 계약이다: 워커는 stdout 이 버려진 detached 프로세스라
 * 던져봐야 아무도 못 읽는다. 실패는 반드시 잡 레코드(`status: "failed"`, `errorMessage`)에
 * 남겨야 `status` / `result` 가 사후에 사유를 보여줄 수 있다.
 */
export async function runJob(
  store: JobStore,
  jobId: string,
  executor: OpencodeExecutor,
  options: { pid?: number; timeoutMs?: number } = {},
): Promise<JobRecord> {
  const job = store.get(jobId);
  if (!job) {
    throw new Error(`opencode 잡 "${jobId}" 를 찾을 수 없습니다.`);
  }

  const started = store.update(jobId, {
    status: 'running',
    phase: 'dispatch',
    pid: options.pid ?? process.pid,
    startedAt: new Date().toISOString(),
  });
  const args = buildRunArgs(started.request);
  store.appendLog(jobId, `opencode run 시작 (worktree: ${started.request.worktree})`);
  store.appendLog(jobId, `args: ${args.slice(0, args.length - 1).join(' ')} <prompt>`);

  try {
    const result = await executor.run(args, {
      cwd: started.request.worktree,
      timeoutMs: options.timeoutMs,
      // opencode 는 자체 프로세스 그룹으로 뜨므로 pid 를 즉시 기록해 둬야 `cancel` /
      // `SessionEnd` 가 워커 그룹과 함께 이 그룹도 끊을 수 있다.
      onSpawn: (childPid) => {
        store.update(jobId, { childPid, phase: 'running' });
        store.appendLog(jobId, `opencode 프로세스 시작 (pid ${childPid})`);
      },
    });
    const summary = summarizeRunEvents(result.stdout);
    const rendered = renderRunOutput(result, summary);
    for (const error of summary.errors) {
      store.appendLog(jobId, `error: ${error}`);
    }

    // 종료 코드가 0 이어도 스트림에 에러 이벤트가 있으면 실패로 본다 — opencode 가 모델 오류를
    // 이벤트로만 알리고 0 으로 끝나는 경우가 있어, 조용한 실패를 성공으로 보고하면 안 된다.
    const failed = result.exitCode !== 0 || summary.errors.length > 0;
    store.appendLog(jobId, `opencode 종료 (exit ${result.exitCode})`);
    return store.update(jobId, {
      status: failed ? 'failed' : 'completed',
      phase: 'done',
      exitCode: result.exitCode,
      sessionRef: summary.sessionId,
      result: rendered,
      errorMessage: failed
        ? summary.errors.join('\n') || result.stderr.trim() || `exit ${result.exitCode}`
        : undefined,
      completedAt: new Date().toISOString(),
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    store.appendLog(jobId, `실패: ${message}`);
    return store.update(jobId, {
      status: 'failed',
      phase: 'done',
      errorMessage: message,
      completedAt: new Date().toISOString(),
    });
  }
}

/**
 * 잡에 딸린 프로세스를 모두 죽인다.
 *
 * **두 그룹을 끊어야 한다.** 워커는 `detached` 로 떠서 자기 그룹의 리더이고, opencode 는
 * 타임아웃 때 손자까지 정리하려고 **또 다른** 그룹으로 떨어져 나가 있다. 그래서 워커 그룹만
 * 끊으면 opencode 가 고아로 남아 worktree 를 계속 건드린다. 두 pid 모두에 그룹 kill 을 보낸다.
 */
export function killJobProcess(
  job: { pid?: number; childPid?: number } | number | undefined,
  signal: NodeJS.Signals = 'SIGTERM',
  kill: (target: number, sig: NodeJS.Signals) => void = process.kill,
): { killed: boolean; detail: string } {
  const targets = typeof job === 'number' ? { pid: job } : (job ?? {});
  const parts: string[] = [];
  let killed = false;
  for (const [label, pid] of [
    ['worker', targets.pid],
    ['opencode', targets.childPid],
  ] as const) {
    if (!pid || pid <= 0) {
      continue;
    }
    const outcome = killProcessGroup(pid, signal, kill);
    killed ||= outcome.killed;
    parts.push(`${label}: ${outcome.detail}`);
  }
  if (parts.length === 0) {
    return { killed: false, detail: '기록된 pid 가 없습니다.' };
  }
  return { killed, detail: parts.join(' / ') };
}
