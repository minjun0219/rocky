import { describe, expect, it } from 'bun:test';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { buildEnvExports } from './session-jobs';

/**
 * `CLAUDE_ENV_FILE` 에 쓰는 export 구문이 실제 셸에서 주입으로 새지 않는지 검증한다.
 *
 * 단정만으로는 부족하다 — 생성한 구문을 진짜 `sh` 로 source 해서, 주입 시도가 담긴 세션 id 가
 * 추가 명령/추가 변수로 실행되지 않는지 확인한다.
 */

/** 생성된 export 구문을 sh 로 source 한 뒤, 지정한 변수들의 값을 돌려준다. */
async function sourceAndInspect(
  sessionId: string,
  vars: string[],
): Promise<{ values: Record<string, string>; sideEffect: string }> {
  const dir = mkdtempSync(join(tmpdir(), 'rocky-env-'));
  const envFile = join(dir, 'env.sh');
  const marker = join(dir, 'pwned.txt');
  writeFileSync(envFile, buildEnvExports(sessionId), 'utf8');

  const probe = vars.map((v) => `printf '%s\\037' "\${${v}-}"`).join('; ');
  const proc = Bun.spawn(['sh', '-c', `. "${envFile}" 2>/dev/null; ${probe}`], {
    env: { PATH: process.env.PATH ?? '', MARKER: marker },
    stdout: 'pipe',
    stderr: 'ignore',
  });
  const out = await new Response(proc.stdout).text();
  await proc.exited;

  const parts = out.split('');
  const values: Record<string, string> = {};
  vars.forEach((v, i) => {
    values[v] = parts[i] ?? '';
  });
  return { values, sideEffect: (await Bun.file(marker).exists()) ? 'created' : 'none' };
}

describe('CLAUDE_ENV_FILE 주입 방어', () => {
  it('은 세미콜론 주입을 값 안에 가둔다', async () => {
    const evil = `x'; export EVIL=1; :'`;
    const { values } = await sourceAndInspect(evil, ['ROCKY_SESSION_ID', 'EVIL']);
    expect(values.EVIL).toBe('');
    expect(values.ROCKY_SESSION_ID).toBe(evil);
  });

  // 개행은 가드가 먼저 걸러 아무것도 쓰지 않는다 (buildEnvExports). 설령 통과했더라도
  // 작은따옴표 안에서는 리터럴이라 구문이 끊기지 않는다 — 두 겹 모두 EVIL 이 설정되지 않는다.
  it('은 개행이 섞인 값으로 추가 export 를 실행하지 않는다', async () => {
    const { values } = await sourceAndInspect('x\nexport EVIL=1\n', ['ROCKY_SESSION_ID', 'EVIL']);
    expect(values.EVIL).toBe('');
    expect(values.ROCKY_SESSION_ID).toBe('');
  });

  // 줄바꿈 없는 주입 시도는 가드를 통과하므로, 여기서 막는 건 순수하게 quoting 이다.
  it('은 개행 없는 주입 시도를 quoting 만으로 가둔다', async () => {
    const evil = `x' ; export EVIL=1 ; :'`;
    const { values } = await sourceAndInspect(evil, ['ROCKY_SESSION_ID', 'EVIL']);
    expect(values.EVIL).toBe('');
    expect(values.ROCKY_SESSION_ID).toBe(evil);
  });

  it('은 명령 치환 시도를 실행하지 않는다', async () => {
    const { values } = await sourceAndInspect('$(touch "$MARKER")', ['ROCKY_SESSION_ID']);
    expect(values.ROCKY_SESSION_ID).toBe('$(touch "$MARKER")');
  });

  it('은 백틱 치환 시도를 실행하지 않는다', async () => {
    const { values, sideEffect } = await sourceAndInspect('`touch "$MARKER"`', [
      'ROCKY_SESSION_ID',
    ]);
    expect(sideEffect).toBe('none');
    expect(values.ROCKY_SESSION_ID).toBe('`touch "$MARKER"`');
  });
});
