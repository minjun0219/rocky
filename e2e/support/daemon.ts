/**
 * E2E 용 격리 데몬 — 임시 폴더의 전용 설정·DB 로 `target/debug/rockyd` 를 띄우고 가짜 픽스처를 심는다.
 *
 * Playwright 러너는 Node 에서 돈다 — 그래서 `Bun.*` 대신 `node:` 모듈만 쓴다(Bun 에서도 그대로 돈다).
 * 실제 보드·작업로그·사용 로그·GitHub 계정에는 닿지 않는다: HOME 까지 임시 폴더로 돌리고 환경 변수는
 * 필요한 것만 넘긴다.
 */
import { spawn } from 'node:child_process';
import { closeSync, openSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

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
  const log = join(work, 'rockyd.log');
  const fd = openSync(log, 'w');
  // 환경은 물려받지 않는다: GH_TOKEN·ROCKY_* 같은 값이 새면 실제 계정·설정에 닿는다. HOME 도 임시 폴더라
  // `~/.config` 기본값이나 gh 설정을 읽지 않는다.
  const proc = spawn(join(root, 'target', 'debug', 'rockyd'), [], {
    cwd: root,
    env: {
      PATH: process.env.PATH ?? '/usr/bin:/bin',
      HOME: work,
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

export type Todo = { id: string; ref: string };

/** ref(`demo-3`)의 번호 조각 — 가장 오른쪽 `-` 에서 가른다. */
export function refNumber(ref: string): number {
  const n = Number(ref.slice(ref.lastIndexOf('-') + 1));
  if (!Number.isInteger(n)) {
    throw new Error(`todo ref 에서 번호를 못 읽었다: ${ref}`);
  }
  return n;
}

/**
 * 공용 픽스처 — 모든 테스트가 읽기만 한다. 쓰는 테스트는 자기 이름이 든 항목을 따로 만든다.
 * 돌려주는 값은 퍼머링크 테스트가 여는 할 일 번호.
 */
export async function seed(base: string): Promise<{ permalinkNumber: number }> {
  await call(base, 'POST', '/api/boards', { key: 'demo', title: 'Demo' });
  await call(base, 'POST', '/api/boards', { key: 'demo-two', title: 'Demo Two' });
  await call(base, 'POST', '/api/sections', { board: 'demo', title: '이번 주' });

  const first = await call<Todo>(base, 'POST', '/api/todos', {
    board: 'demo',
    title: '로그인 화면 문구 다듬기',
    description: '버튼 문구와 **오류 메시지**를 맞춘다.\n\n- 빈 입력\n- 잘못된 비밀번호',
    priority: 'p1',
    labels: ['ui', 'copy'],
    links: [{ url: 'https://example.com/spec/login', title: '로그인 스펙' }],
    section: '이번 주',
  });
  for (const title of ['하위: 오류 메시지 목록', '하위: 문구 검토 요청']) {
    await call(base, 'POST', '/api/todos', { board: 'demo', title, parentId: first.id });
  }
  const others: Todo[] = [];
  for (const todo of [
    { board: 'demo', title: '설정 화면 정리', priority: 'p2', labels: ['ui'] },
    { board: 'demo', title: '의존성 버전 점검', priority: 'p3' },
    { board: 'demo-two', title: '두 번째 보드의 할 일', priority: 'p2' },
    { board: 'demo-two', title: '문서 링크 모으기', labels: ['docs'] },
  ]) {
    others.push(await call<Todo>(base, 'POST', '/api/todos', todo));
  }
  // 읽지 않은 댓글 — 새 브라우저는 전부 안 읽음이라 4건 이상이면 "N건 더 보기" 요약 줄이 생긴다.
  for (const [i, todo] of [first, ...others].entries()) {
    await call(
      base,
      'POST',
      `/api/todos/${todo.id}/comments`,
      { body: `진행 메모 ${i + 1}` },
      'claude',
    );
  }
  const done = await call<Todo>(base, 'POST', '/api/todos', {
    board: 'demo',
    title: '끝난 일 하나',
    priority: 'p3',
  });
  await call(base, 'POST', `/api/todos/${done.id}/status`, { action: 'done' });

  await call(base, 'POST', '/api/notes', {
    board: 'demo',
    title: '회의 메모',
    content: '## 결정\n\n- 다음 주 배포',
  });

  return { permalinkNumber: refNumber(first.ref) };
}
