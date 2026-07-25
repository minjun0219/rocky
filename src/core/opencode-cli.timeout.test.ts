import { describe, expect, it } from 'bun:test';
import { chmodSync, existsSync, mkdtempSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createOpencodeExecutor } from './opencode-cli';

/**
 * 타임아웃 시 opencode 가 띄운 **손자 프로세스**까지 죽는지 검증한다.
 *
 * 단일 pid 만 죽이면 opencode 의 도구 프로세스(`bash` 등)가 살아남아, 잡이 `failed` 로
 * 기록된 뒤에도 worktree 를 계속 수정한다. worktree 격리가 유일한 봉쇄 수단이므로 이건
 * 조용히 지나가면 안 되는 회귀다.
 *
 * 생존 판정은 pgrep 대신 **heartbeat 파일**로 한다: 손자가 100ms 마다 파일에 쓰고, 죽은
 * 뒤에는 mtime 이 더 이상 변하지 않는다. 프로세스 조회 도구에 의존하지 않아 이식성이 좋다.
 */

function writeFakeOpencode(dir: string, heartbeat: string): string {
  const path = join(dir, 'fake-opencode');
  writeFileSync(
    path,
    [
      '#!/bin/sh',
      'if [ "$1" = "--version" ]; then echo "9.9.9"; exit 0; fi',
      // opencode 가 도구 프로세스를 띄운 상황 — 이 자식이 손자다.
      `sh -c 'while true; do echo tick >> "${heartbeat}"; sleep 0.1; done' &`,
      'sleep 60',
    ].join('\n'),
    'utf8',
  );
  chmodSync(path, 0o755);
  return path;
}

describe('실행 타임아웃', () => {
  it('은 타임아웃 시 손자 프로세스까지 정리한다', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'rocky-timeout-'));
    const heartbeat = join(dir, 'heartbeat.txt');
    const bin = writeFakeOpencode(dir, heartbeat);

    const executor = createOpencodeExecutor(bin);
    await expect(executor.run(['run'], { cwd: dir, timeoutMs: 700 })).rejects.toThrow(/700ms/);

    // 손자가 실제로 살아있었는지 먼저 확인 — 아니면 이 테스트는 아무것도 증명하지 못한다.
    expect(existsSync(heartbeat)).toBe(true);

    await Bun.sleep(150);
    const settled = statSync(heartbeat).mtimeMs;
    await Bun.sleep(600); // heartbeat 주기(100ms)보다 충분히 길게
    expect(statSync(heartbeat).mtimeMs).toBe(settled);
  }, 20_000);

  it('은 타임아웃 전에 끝나면 정상 결과를 준다', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'rocky-timeout-ok-'));
    const bin = join(dir, 'quick');
    writeFileSync(bin, '#!/bin/sh\necho \'{"type":"text","text":"ok"}\'\n', 'utf8');
    chmodSync(bin, 0o755);

    const result = await createOpencodeExecutor(bin).run(['run'], { cwd: dir, timeoutMs: 5_000 });
    expect(result.exitCode).toBe(0);
    expect(result.stdout).toContain('ok');
  }, 20_000);

  it('은 신호로 죽은 프로세스를 성공(0)으로 오해하지 않는다', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'rocky-signal-'));
    const bin = join(dir, 'suicide');
    writeFileSync(bin, '#!/bin/sh\nkill -9 $$\n', 'utf8');
    chmodSync(bin, 0o755);

    const result = await createOpencodeExecutor(bin).run(['run'], { cwd: dir, timeoutMs: 5_000 });
    expect(result.exitCode).not.toBe(0);
  }, 20_000);

  it('은 바이너리가 없으면 예외를 던진다', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'rocky-enoent-'));
    await expect(
      createOpencodeExecutor(join(dir, 'nope')).run(['run'], { cwd: dir, timeoutMs: 5_000 }),
    ).rejects.toThrow();
  }, 20_000);
});
