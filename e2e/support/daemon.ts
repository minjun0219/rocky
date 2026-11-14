/**
 * E2E 용 격리 데몬 — 임시 폴더의 전용 설정·DB 로 `target/debug/rockyd` 를 띄우고 가짜 픽스처를 심는다.
 *
 * Playwright 러너는 Node 에서 돈다 — 그래서 `Bun.*` 대신 `node:` 모듈만 쓴다(Bun 에서도 그대로 돈다).
 * 실제 보드·작업로그·사용 로그·GitHub 계정에는 닿지 않는다: HOME 까지 임시 폴더로 돌리고 환경 변수는
 * 필요한 것만 넘긴다. 세션 목록도 이 기기의 것이 아니라 가짜 `claude`(`claude.ts`)가 낸다.
 */
import { spawn } from 'node:child_process';
import { closeSync, openSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { delimiter, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { installFakeClaude } from './claude';

export const root = fileURLToPath(new URL('../..', import.meta.url));

function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      server.close(() => {
        if (address && typeof address === 'object') {
          resolve(address.port);
        } else {
          reject(new Error('빈 포트를 얻지 못했다'));
        }
      });
    });
  });
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

export type Daemon = { base: string; pid: number; log: string };

export async function startDaemon(work: string): Promise<Daemon> {
  const port = await freePort();
  const config = join(work, 'rocky.json');
  writeFileSync(
    config,
    JSON.stringify(
      {
        todo: { port, dir: join(work, 'todo'), expose: 'off' },
        pr: { enabled: false },
        worklog: { dir: join(work, 'worklog') },
        usage: { enabled: false },
      },
      null,
      2,
    ),
  );
  const claude = installFakeClaude(work);
  const log = join(work, 'rockyd.log');
  const fd = openSync(log, 'w');
  // 환경은 물려받지 않는다: GH_TOKEN·ROCKY_* 같은 값이 새면 실제 계정·설정에 닿는다. HOME 도 임시 폴더라
  // `~/.config` 기본값이나 gh 설정을 읽지 않는다.
  const proc = spawn(join(root, 'target', 'debug', 'rockyd'), [], {
    cwd: root,
    env: {
      PATH: `${claude.bin}${delimiter}${process.env.PATH ?? '/usr/bin:/bin'}`,
      HOME: work,
      CLAUDE_CONFIG_DIR: claude.configDir,
      ROCKY_CONFIG: config,
      ROCKY_TODO_UI_DIST: join(root, 'dist'),
      ROCKY_USAGE: '0',
    },
    stdio: ['ignore', fd, fd],
  });
  closeSync(fd); // 자식이 복제해 갖고 있다
  if (!proc.pid) {
    throw new Error(`rockyd 를 띄우지 못했다 — 로그: ${log}`);
  }
  // 러너가 이 자식을 기다리지 않게 한다 — 끝내는 건 globalTeardown 이 pid 로.
  proc.unref();
  const daemon: Daemon = { base: `http://127.0.0.1:${port}`, pid: proc.pid, log };
  const deadline = Date.now() + 20_000;
  while (Date.now() < deadline) {
    if (proc.exitCode !== null) {
      throw new Error(`rockyd 가 바로 끝났다 (exit ${proc.exitCode}) — 로그: ${log}`);
    }
    try {
      const res = await fetch(`${daemon.base}/api/health`);
      const body = (await res.json()) as { name?: string };
      if (res.ok && body.name === 'rocky') {
        return daemon;
      }
    } catch {
      // 아직 안 떴다
    }
    await sleep(200);
  }
  await stopDaemon(daemon.pid);
  throw new Error(`rockyd health 가 20초 안에 오지 않았다 (${daemon.base}) — 로그: ${log}`);
}

const alive = (pid: number) => {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
};

/** 우리가 띄운 pid 만 내린다 — 패턴으로 찾아 죽이지 않는다(다른 rockyd 가 같이 돌 수 있다). */
export async function stopDaemon(pid: number): Promise<void> {
  if (!alive(pid)) {
    return;
  }
  process.kill(pid, 'SIGTERM');
  const deadline = Date.now() + 5000;
  while (Date.now() < deadline) {
    if (!alive(pid)) {
      return;
    }
    await sleep(100);
  }
  process.kill(pid, 'SIGKILL');
}

/** 픽스처 REST 호출 — 실패하면 상태 코드와 본문을 담아 던진다. */
export async function call<T>(
  base: string,
  method: string,
  path: string,
  body: unknown,
  actor = 'human',
): Promise<T> {
  const res = await fetch(`${base}${path}`, {
    method,
    headers: { 'content-type': 'application/json', 'x-rocky-actor': actor },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    throw new Error(`픽스처 ${method} ${path} → ${res.status} ${await res.text()}`);
  }
  return (await res.json()) as T;
}
