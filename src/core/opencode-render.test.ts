import { describe, expect, it } from 'bun:test';
import type { JobRecord } from './opencode-jobs';
import {
  buildStatusSnapshot,
  elapsedMs,
  formatDuration,
  renderJobLine,
  renderResult,
  renderStatus,
  toSummary,
} from './opencode-render';

const NOW = new Date('2026-07-25T12:00:00.000Z');

function job(overrides: Partial<JobRecord> = {}): JobRecord {
  return {
    id: 'oc-aaa-111111',
    kind: 'task',
    title: '문서 갱신',
    status: 'completed',
    phase: 'done',
    workspaceRoot: '/repo',
    request: { prompt: 'p', worktree: '/repo/wt', branch: 'opencode/docs' },
    logFile: '/tmp/x.log',
    seq: 1,
    createdAt: '2026-07-25T11:58:00.000Z',
    startedAt: '2026-07-25T11:58:00.000Z',
    updatedAt: '2026-07-25T11:59:00.000Z',
    completedAt: '2026-07-25T11:59:00.000Z',
    ...overrides,
  };
}

describe('formatDuration', () => {
  it('은 분 미만을 초로 쓴다', () => {
    expect(formatDuration(12_000)).toBe('12s');
  });

  it('은 시간 미만을 분초로 쓴다', () => {
    expect(formatDuration(184_000)).toBe('3m 4s');
  });

  it('은 한 시간 이상을 시분으로 쓴다', () => {
    expect(formatDuration(3_720_000)).toBe('1h 2m');
  });

  it('은 이상한 값을 - 로 흡수한다', () => {
    expect(formatDuration(Number.NaN)).toBe('-');
    expect(formatDuration(-5)).toBe('-');
  });
});

describe('elapsedMs', () => {
  it('은 끝난 잡의 시작~완료 구간을 잰다', () => {
    expect(elapsedMs(job(), NOW)).toBe(60_000);
  });

  it('은 진행 중인 잡을 now 까지 잰다', () => {
    expect(elapsedMs(job({ status: 'running', completedAt: undefined }), NOW)).toBe(120_000);
  });

  it('은 startedAt 이 없으면 createdAt 부터 잰다', () => {
    const queued = job({ status: 'queued', startedAt: undefined, completedAt: undefined });
    expect(elapsedMs(queued, NOW)).toBe(120_000);
  });

  it('은 시각이 깨져 있으면 0 을 준다', () => {
    expect(elapsedMs(job({ startedAt: '없는날짜', completedAt: undefined }), NOW)).toBe(0);
  });
});

describe('renderJobLine', () => {
  it('은 상태 기호와 id / 제목을 담는다', () => {
    const line = renderJobLine(toSummary(job(), NOW));
    expect(line).toContain('✓');
    expect(line).toContain('oc-aaa-111111');
    expect(line).toContain('문서 갱신');
  });

  it('은 진행 중 잡에 ▶ 를 쓴다', () => {
    const line = renderJobLine(toSummary(job({ status: 'running', completedAt: undefined }), NOW));
    expect(line).toContain('▶');
  });
});

describe('buildStatusSnapshot / renderStatus', () => {
  it('은 잡이 없으면 그 사실을 알린다', () => {
    expect(renderStatus(buildStatusSnapshot([]))).toContain('없습니다');
  });

  it('은 running 과 종료된 잡을 나눈다', () => {
    const running = toSummary(
      job({ id: 'oc-run', status: 'running', completedAt: undefined }),
      NOW,
    );
    const done = toSummary(job({ id: 'oc-done' }), NOW);
    const snapshot = buildStatusSnapshot([running, done]);
    expect(snapshot.running.map((j) => j.id)).toEqual(['oc-run']);
    expect(snapshot.latestFinished?.id).toBe('oc-done');
    expect(snapshot.total).toBe(2);
  });

  it('은 queued 도 진행 중으로 센다', () => {
    const queued = toSummary(job({ status: 'queued', completedAt: undefined }), NOW);
    expect(buildStatusSnapshot([queued]).running).toHaveLength(1);
  });

  it('은 진행 중 잡의 로그 tail 을 보여준다', () => {
    const running = toSummary(job({ status: 'running', completedAt: undefined }), NOW, [
      '[ts] opencode run 시작',
    ]);
    expect(renderStatus(buildStatusSnapshot([running]))).toContain('opencode run 시작');
  });

  it('은 실패 잡의 사유 첫 줄을 보여준다', () => {
    const failed = toSummary(job({ status: 'failed', errorMessage: '게이트 실패\n상세' }), NOW);
    const out = renderStatus(buildStatusSnapshot([failed]));
    expect(out).toContain('게이트 실패');
    expect(out).not.toContain('상세');
  });
});

describe('renderResult', () => {
  it('은 진행 중이면 결과 대신 상태를 알린다', () => {
    const out = renderResult(
      job({ status: 'running', completedAt: undefined, result: '절반' }),
      NOW,
    );
    expect(out).toContain('아직 running');
    expect(out).not.toContain('절반');
  });

  it('은 끝난 잡의 본문을 보여준다', () => {
    expect(renderResult(job({ result: '완성된 출력' }), NOW)).toContain('완성된 출력');
  });

  it('은 출력이 비면 그 사실을 표시한다', () => {
    expect(renderResult(job({ result: '   ' }), NOW)).toContain('(출력 없음)');
  });

  it('은 opencode 세션 id 로 이어가는 법을 안내한다', () => {
    const out = renderResult(job({ sessionRef: 'ses_7' }), NOW);
    expect(out).toContain('--session ses_7');
  });

  it('은 실패 사유를 함께 보여준다', () => {
    expect(renderResult(job({ status: 'failed', errorMessage: '터짐' }), NOW)).toContain('터짐');
  });
});
