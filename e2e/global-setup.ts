/**
 * 모든 프로젝트가 같이 쓰는 격리 데몬을 **한 번** 띄우고 공용 픽스처를 심는다.
 *
 * 빌드(`bun run build:ui` + `cargo build -p rockyd`)도 여기서 한다 — `E2E_NO_BUILD=1` 이면 건너뛴다(CI 는
 * 앞 단계에서 빌드한다). 데몬 주소·pid·임시 폴더는 환경 변수로 넘긴다: 워커와 globalTeardown 이 러너의
 * 환경을 물려받는다.
 */
import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, rmSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { join } from 'node:path';
import { root, seed, startDaemon, stopDaemon } from './support/daemon';

function run(cmd: string, args: string[]): void {
  const result = spawnSync(cmd, args, { cwd: root, stdio: 'inherit' });
  if (result.status !== 0) {
    throw new Error(
      `${cmd} ${args.join(' ')} 실패 (exit ${result.status ?? result.error?.message})`,
    );
  }
}

function cargo(): string {
  const which = spawnSync('which', ['cargo'], { encoding: 'utf8' });
  const found = which.status === 0 ? which.stdout.trim() : '';
  if (found) {
    return found;
  }
  const fallback = join(homedir(), '.cargo', 'bin', 'cargo');
  return existsSync(fallback) ? fallback : 'cargo';
}

export default async function globalSetup(): Promise<void> {
  if (process.env.E2E_NO_BUILD !== '1') {
    run('bun', ['run', 'build:ui']);
    run(cargo(), ['build', '-p', 'rockyd']);
  }
  const work = mkdtempSync(join(tmpdir(), 'rocky-e2e-'));
  process.env.E2E_WORK = work;
  try {
    const daemon = await startDaemon(work);
    process.env.E2E_DAEMON_PID = String(daemon.pid);
    process.env.E2E_BASE_URL = daemon.base;
    const seeded = await seed(daemon.base);
    process.env.E2E_PERMALINK = String(seeded.permalinkNumber);
  } catch (e) {
    // globalSetup 이 던지면 globalTeardown 이 돌지 않는다 — 여기서 치운다.
    const pid = Number(process.env.E2E_DAEMON_PID);
    if (pid) {
      await stopDaemon(pid);
    }
    rmSync(work, { recursive: true, force: true });
    throw e;
  }
}
