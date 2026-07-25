import { describe, expect, it } from 'bun:test';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { JobStore } from './opencode-jobs';

/**
 * 인덱스 갱신의 프로세스 간 경합 회귀 테스트.
 *
 * 단일 프로세스 안에서는 동기 코드가 경합하지 않으므로, 실제로 프로세스를 여럿 띄워야만
 * lost update 를 재현할 수 있다. 락이 없으면 각 프로세스가 같은 `state.json` 스냅샷을 읽고
 * 덮어써서 대부분의 잡이 인덱스에서 사라진다.
 */

const WORKER = `
import { JobStore } from "%SRC%";
const store = new JobStore({ dir: process.argv[2] });
store.create({
  title: process.argv[3],
  workspaceRoot: "/repo",
  request: { prompt: "p", worktree: "/repo/wt" },
});
`;

async function spawnCreators(dir: string, count: number): Promise<void> {
  const src = join(import.meta.dir, 'opencode-jobs.ts');
  const scriptPath = join(dir, 'creator.ts');
  writeFileSync(scriptPath, WORKER.replace('%SRC%', src), 'utf8');
  const procs = Array.from({ length: count }, (_, i) =>
    Bun.spawn([process.execPath, scriptPath, dir, `job-${i}`], {
      stdout: 'ignore',
      stderr: 'pipe',
    }),
  );
  await Promise.all(procs.map((proc) => proc.exited));
}

describe('동시 create (프로세스 간)', () => {
  it('은 동시에 만든 잡을 하나도 잃지 않는다', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'rocky-jobs-race-'));
    const count = 8;
    await spawnCreators(dir, count);

    const jobs = new JobStore({ dir }).list();
    expect(jobs).toHaveLength(count);
    // 인덱스에만 있고 payload 가 없는 유령 잡이 없어야 한다.
    expect(jobs.filter((job) => job.request.prompt === 'p')).toHaveLength(count);
  }, 30_000);

  it('은 seq 를 중복 없이 발급한다', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'rocky-jobs-seq-'));
    await spawnCreators(dir, 6);
    const seqs = new JobStore({ dir }).list().map((job) => job.seq);
    expect(new Set(seqs).size).toBe(seqs.length);
  }, 30_000);
});
