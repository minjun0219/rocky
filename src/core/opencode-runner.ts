import {
  type OpencodeExecutor,
  buildRunArgs,
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
 * 잡의 워커 프로세스를 죽인다.
 *
 * 워커는 `detached: true` 로 띄웠으므로 자기 자신이 프로세스 그룹 리더다 — `kill(-pid)` 로
 * **그룹 전체**를 끊어야 워커가 띄운 opencode 자식까지 같이 죽는다. `kill(pid)` 만 보내면
 * 워커만 죽고 opencode 는 고아로 계속 돈다. detached spawn 과 이 호출은 한 세트다.
 */
export function killJobProcess(
  pid: number | undefined,
  signal: NodeJS.Signals = 'SIGTERM',
  kill: (target: number, sig: NodeJS.Signals) => void = process.kill,
): { killed: boolean; detail: string } {
  if (!pid || pid <= 0) {
    return { killed: false, detail: '기록된 pid 가 없습니다.' };
  }
  try {
    kill(-pid, signal);
    return { killed: true, detail: `프로세스 그룹 ${pid} 에 ${signal} 전송` };
  } catch (groupError) {
    // 그룹이 없으면(워커가 detached 로 안 떴거나 이미 리핑됨) 단일 프로세스로 재시도.
    try {
      kill(pid, signal);
      return { killed: true, detail: `프로세스 ${pid} 에 ${signal} 전송 (그룹 없음)` };
    } catch {
      const message = groupError instanceof Error ? groupError.message : String(groupError);
      return { killed: false, detail: `이미 종료됐거나 죽일 수 없습니다: ${message}` };
    }
  }
}
