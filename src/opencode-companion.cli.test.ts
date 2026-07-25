import { beforeEach, describe, expect, it } from 'bun:test';
import { chmodSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

/**
 * companion 진입점을 실제 서브프로세스로 돌리는 엔드투엔드 테스트.
 *
 * 핸들러들이 export 되어 있지 않고 `process.exit` 로 끝나므로, 계약(종료 코드 + stdout/stderr)을
 * 검증하려면 실제로 실행하는 편이 정확하다.
 */

const SCRIPT = join(import.meta.dir, 'opencode-companion.ts');

let dir: string;
let jobsDir: string;
let worktree: string;
let fakeBin: string;

function run(args: string[]) {
  const proc = Bun.spawn([process.execPath, SCRIPT, ...args], {
    cwd: dir,
    env: {
      ...process.env,
      ROCKY_OPENCODE_CLI: fakeBin,
      ROCKY_OPENCODE_JOBS_DIR: jobsDir,
      ROCKY_SESSION_ID: 'sess-cli-test',
      ROCKY_CONFIG: join(dir, 'no-such-config.json'),
    },
    stdout: 'pipe',
    stderr: 'pipe',
  });
  return (async () => ({
    stdout: await new Response(proc.stdout).text(),
    stderr: await new Response(proc.stderr).text(),
    exitCode: await proc.exited,
  }))();
}

beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'rocky-cli-'));
  jobsDir = join(dir, 'jobs');
  worktree = join(dir, 'wt');
  mkdirSync(worktree, { recursive: true });
  fakeBin = join(dir, 'fake-opencode');
  writeFileSync(
    fakeBin,
    [
      '#!/bin/sh',
      'if [ "$1" = "--version" ]; then echo "9.9.9"; exit 0; fi',
      'echo \'{"type":"session.start","sessionID":"ses_cli"}\'',
      'echo \'{"type":"text","part":{"id":"p1","text":"완료했다"}}\'',
    ].join('\n'),
    'utf8',
  );
  chmodSync(fakeBin, 0o755);
});

async function seedJob(title: string) {
  const out = await run(['task', '--worktree', worktree, '--model', 'x/y', '--json', title]);
  expect(out.exitCode).toBe(0);
  return JSON.parse(out.stdout) as { id: string; title: string };
}

describe('check', () => {
  it('은 설치된 CLI 를 보고한다', async () => {
    const out = await run(['check']);
    expect(out.exitCode).toBe(0);
    expect(out.stdout).toContain('9.9.9');
  });
});

describe('task', () => {
  it('은 foreground 로 돌고 결과를 낸다', async () => {
    const out = await run(['task', '--worktree', worktree, '--model', 'x/y', '작업해줘']);
    expect(out.exitCode).toBe(0);
    expect(out.stdout).toContain('완료했다');
    expect(out.stdout).toContain('ses_cli');
  });

  it('은 --worktree 없이 거부한다', async () => {
    const out = await run(['task', '작업']);
    expect(out.exitCode).toBe(2);
    expect(out.stderr).toContain('--worktree');
  });

  // 스택 트레이스 대신 한 줄 메시지여야 한다.
  it('은 없는 --prompt-file 을 한 줄로 알린다', async () => {
    const out = await run([
      'task',
      '--worktree',
      worktree,
      '--prompt-file',
      join(dir, 'missing.md'),
    ]);
    expect(out.exitCode).toBe(2);
    expect(out.stderr).toContain('프롬프트를 읽지 못했습니다');
    expect(out.stderr).not.toContain('at ');
  });

  // 명시한 파일이 뒤따르는 positional 에 덮이면 조용히 엉뚱한 프롬프트로 위임하게 된다.
  it('은 --prompt-file 을 positional 보다 우선한다', async () => {
    const file = join(dir, 'prompt.md');
    writeFileSync(file, '파일에서 온 프롬프트', 'utf8');
    const out = await run([
      'task',
      '--worktree',
      worktree,
      '--json',
      '--prompt-file',
      file,
      '무시되어야 하는 positional',
    ]);
    expect(out.exitCode).toBe(0);
    const job = JSON.parse(out.stdout) as { request: { prompt: string } };
    expect(job.request.prompt).toContain('파일에서 온 프롬프트');
    expect(job.request.prompt).not.toContain('무시되어야 하는');
  });

  it('은 파일이 없으면 positional 을 쓴다', async () => {
    const out = await run(['task', '--worktree', worktree, '--json', 'positional 프롬프트']);
    const job = JSON.parse(out.stdout) as { request: { prompt: string } };
    expect(job.request.prompt).toBe('positional 프롬프트');
  });

  it('은 빈 프롬프트를 거부한다', async () => {
    const empty = join(dir, 'empty.md');
    writeFileSync(empty, '   \n', 'utf8');
    const out = await run(['task', '--worktree', worktree, '--prompt-file', empty]);
    expect(out.exitCode).toBe(2);
    expect(out.stderr).toContain('비어');
  });
});

describe('status [job-ref]', () => {
  it('은 job-ref 를 주면 그 잡만 보여준다', async () => {
    const first = await seedJob('첫째 작업');
    const second = await seedJob('둘째 작업');

    const all = await run(['status']);
    expect(all.stdout).toContain('첫째 작업');
    expect(all.stdout).toContain('둘째 작업');

    const one = await run(['status', second.id]);
    expect(one.exitCode).toBe(0);
    expect(one.stdout).toContain('둘째 작업');
    expect(one.stdout).not.toContain('첫째 작업');
  });

  it('은 prefix 로도 좁힌다', async () => {
    const job = await seedJob('프리픽스 작업');
    const out = await run(['status', job.id.slice(0, 10)]);
    expect(out.stdout).toContain('프리픽스 작업');
  });

  it('은 없는 job-ref 를 조용히 넘기지 않는다', async () => {
    await seedJob('아무거나');
    const out = await run(['status', 'oc-does-not-exist']);
    expect(out.exitCode).toBe(1);
    expect(out.stderr).toContain('oc-does-not-exist');
  });

  it('은 모호한 prefix 를 거부한다', async () => {
    await seedJob('a');
    await seedJob('b');
    const out = await run(['status', 'oc-']);
    expect(out.exitCode).toBe(1);
    expect(out.stderr).toMatch(/모호/);
  });
});

describe('result', () => {
  it('은 최신 잡의 출력을 준다', async () => {
    await seedJob('결과 작업');
    const out = await run(['result']);
    expect(out.exitCode).toBe(0);
    expect(out.stdout).toContain('완료했다');
  });
});

describe('cancel', () => {
  it('은 진행 중 잡이 없으면 실패로 알린다', async () => {
    await seedJob('이미 끝난 작업');
    const out = await run(['cancel']);
    expect(out.exitCode).toBe(1);
    expect(out.stderr.length).toBeGreaterThan(0);
  });
});
