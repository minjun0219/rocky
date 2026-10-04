/** globalSetup 이 띄운 데몬을 pid 로만 내리고 임시 폴더를 지운다. */
import { rmSync } from 'node:fs';
import { stopDaemon } from './support/daemon';

export default async function globalTeardown(): Promise<void> {
  const pid = Number(process.env.E2E_DAEMON_PID);
  if (pid) {
    await stopDaemon(pid);
  }
  const work = process.env.E2E_WORK;
  if (work) {
    rmSync(work, { recursive: true, force: true });
  }
}
