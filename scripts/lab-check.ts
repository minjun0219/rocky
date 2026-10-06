// rocky lab(plugin/hooks/lab) 검증 — `claude plugin validate` + `claude plugin test`. claude CLI 가 있어야 해서 CI 밖이다.
//
// `claude plugin test <dir>` 은 그 폴더의 *.test.ts·*.test.tsx 를 전부 엔진 키트로 돌려 bun 테스트(plugin/scripts,
// plugin/hooks/lab/lib.test.ts)까지 집는다(`bun:test` 를 못 불러 실패). 그래서 lab 이 쓰는 파일만 임시 폴더에 옮기고 bun
// 테스트(*.test.ts)는 빼서 거기서 돌린다 — 엔진 테스트는 *.test.tsx 로 둔다.
import { cpSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const plugin = join(import.meta.dir, '..', 'plugin');
const dir = mkdtempSync(join(tmpdir(), 'rocky-lab-'));

function run(args: string[]): number {
  try {
    return (
      Bun.spawnSync(['claude', ...args], { stdout: 'inherit', stderr: 'inherit' }).exitCode ?? 1
    );
  } catch (err) {
    console.error(
      `claude ${args[0]} ${args[1]}: ${String(err)} — claude CLI 가 PATH 에 있어야 한다`,
    );
    return 1;
  }
}

let code = 0;
try {
  for (const part of ['.claude-plugin/plugin.json', 'hooks', 'types']) {
    cpSync(join(plugin, part), join(dir, part), {
      recursive: true,
      filter: (src) => !src.endsWith('.test.ts'),
    });
  }
  code = run(['plugin', 'validate', dir]) || run(['plugin', 'test', dir]);
} finally {
  rmSync(dir, { recursive: true, force: true });
}
process.exit(code);
