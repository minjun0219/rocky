import { type JobRecord, isTerminal } from './opencode-jobs';

/**
 * 잡 상태 / 결과의 사람용 렌더링. 전부 순수 함수라 시각(now)을 주입받는다.
 *
 * 렌더링을 companion 진입점에서 떼어낸 이유는 두 가지다 — (a) 시간 계산과 문자열 조립은
 * 회귀가 잘 나는데 진입점에 있으면 테스트가 안 되고, (b) `--json` 경로와 사람용 경로가
 * 같은 스냅샷 구조를 공유해야 둘이 어긋나지 않는다.
 */

/** `status` 가 만드는 스냅샷 — `--json` 출력이 이 모양 그대로 나간다. */
export interface StatusSnapshot {
  running: JobSummary[];
  latestFinished?: JobSummary;
  recent: JobSummary[];
  total: number;
}

/** 잡 한 건의 표시용 축약. */
export interface JobSummary {
  id: string;
  title: string;
  status: JobRecord['status'];
  phase: string;
  elapsedMs: number;
  worktree: string;
  branch?: string;
  logTail: string[];
  errorMessage?: string;
}

/** 경과 시간을 사람이 읽는 형태로 (`12s` / `3m 4s` / `1h 2m`). */
export function formatDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) {
    return '-';
  }
  const seconds = Math.floor(ms / 1000);
  if (seconds < 60) {
    return `${seconds}s`;
  }
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) {
    return `${minutes}m ${seconds % 60}s`;
  }
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}

/**
 * 잡의 경과 시간. 진행 중이면 `now - startedAt`, 끝났으면 `completedAt - startedAt`.
 * `startedAt` 이 없으면 (아직 queued) 생성 시각부터 잰다.
 */
export function elapsedMs(job: JobRecord, now: Date): number {
  const from = Date.parse(job.startedAt ?? job.createdAt);
  const to = job.completedAt ? Date.parse(job.completedAt) : now.getTime();
  if (!Number.isFinite(from) || !Number.isFinite(to)) {
    return 0;
  }
  return Math.max(0, to - from);
}

/** 잡을 표시용 축약으로 바꾼다. `logTail` 은 호출자가 주입한다 (저장소 접근을 분리). */
export function toSummary(job: JobRecord, now: Date, logTail: string[] = []): JobSummary {
  return {
    id: job.id,
    title: job.title,
    status: job.status,
    phase: job.phase,
    elapsedMs: elapsedMs(job, now),
    worktree: job.request.worktree,
    branch: job.request.branch,
    logTail,
    errorMessage: job.errorMessage,
  };
}

/** 잡 목록을 running / 최근 종료 / 전체 로 나눈다. */
export function buildStatusSnapshot(summaries: JobSummary[]): StatusSnapshot {
  const running = summaries.filter((job) => job.status === 'running' || job.status === 'queued');
  const finished = summaries.filter((job) => !running.includes(job));
  return {
    running,
    latestFinished: finished[0],
    recent: summaries.slice(0, 10),
    total: summaries.length,
  };
}

const STATUS_MARK: Record<JobRecord['status'], string> = {
  queued: '·',
  running: '▶',
  completed: '✓',
  failed: '✗',
  cancelled: '⊘',
};

/** 한 줄 요약 — `▶ oc-abc123  3m 4s  구현 중  제목`. */
export function renderJobLine(job: JobSummary): string {
  const parts = [
    STATUS_MARK[job.status] ?? '?',
    job.id,
    formatDuration(job.elapsedMs).padStart(7),
    job.phase.padEnd(8),
    job.title,
  ];
  return parts.join('  ');
}

/** `status` 사람용 출력. 잡이 없으면 그 사실을 한 줄로 알린다. */
export function renderStatus(snapshot: StatusSnapshot): string {
  if (snapshot.total === 0) {
    return '이 세션에 기록된 opencode 잡이 없습니다.';
  }
  const lines: string[] = [];
  if (snapshot.running.length > 0) {
    lines.push(`진행 중 (${snapshot.running.length})`);
    for (const job of snapshot.running) {
      lines.push(`  ${renderJobLine(job)}`);
      for (const tail of job.logTail) {
        lines.push(`      ${tail}`);
      }
    }
  }
  const others = snapshot.recent.filter((job) => !snapshot.running.includes(job));
  if (others.length > 0) {
    lines.push(`최근 (${others.length})`);
    for (const job of others) {
      lines.push(`  ${renderJobLine(job)}`);
      if (job.errorMessage) {
        lines.push(`      사유: ${job.errorMessage.split('\n')[0]}`);
      }
    }
  }
  return lines.join('\n');
}

/**
 * `result` 사람용 출력. 아직 진행 중이면 결과 대신 그 사실을 알린다 —
 * 절반만 쓰인 출력을 최종 결과처럼 보여주면 안 된다.
 */
export function renderResult(job: JobRecord, now: Date): string {
  if (!isTerminal(job.status)) {
    return `잡 ${job.id} 는 아직 ${job.status} 상태입니다 (${formatDuration(
      elapsedMs(job, now),
    )} 경과, phase: ${job.phase}). 끝난 뒤 다시 조회하세요.`;
  }
  const header = `${STATUS_MARK[job.status]} ${job.id} — ${job.title} (${job.status}, ${formatDuration(
    elapsedMs(job, now),
  )})`;
  const body = job.result?.trim();
  const lines = [header, `worktree: ${job.request.worktree}`];
  if (job.sessionRef) {
    lines.push(`opencode session: ${job.sessionRef} (이어가려면 --session ${job.sessionRef})`);
  }
  if (job.errorMessage) {
    lines.push(`사유: ${job.errorMessage}`);
  }
  lines.push('');
  lines.push(body && body.length > 0 ? body : '(출력 없음)');
  return lines.join('\n');
}
