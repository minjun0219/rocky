/**
 * TUI(`tui/`)를 JS 한 파일(`tui/dist/tui.js`)로 묶는다 — 릴리스 tarball 이 바이너리 옆에 넣고, `rocky tui` 가
 * `bun --no-install tui.js` 로 띄운다. `node_modules` 없이 돈다(yoga-layout 의 WASM 도 안에 들어간다).
 *
 * Ink 는 `DEV=true` 일 때만 `react-devtools-core` 를 부르는데, 번들에는 그 import 가 **정적으로** 남는다. 그대로
 * 두면 `node_modules` 가 있는 폴더에서는 시작하자마자 죽고, 없는 폴더에서는 Bun 이 그 패키지를 네트워크로 몰래
 * 받아 버린다(2026-10-11 스파이크에서 실측) — 빈 모듈로 바꿔 끼운다.
 */
import { rmSync } from 'node:fs';
import { join } from 'node:path';
import type { BunPlugin } from 'bun';

const root = join(import.meta.dir, '..');
const outdir = join(root, 'tui', 'dist');

const stubDevtools: BunPlugin = {
  name: 'stub-react-devtools-core',
  setup(build) {
    build.onResolve({ filter: /^react-devtools-core$/ }, () => ({
      path: 'react-devtools-core',
      namespace: 'stub',
    }));
    build.onLoad({ filter: /.*/, namespace: 'stub' }, () => ({
      contents: 'export default { initialize() {}, connectToDevTools() {} };',
      loader: 'js',
    }));
  },
};

rmSync(outdir, { recursive: true, force: true });

const result = await Bun.build({
  entrypoints: [join(root, 'tui', 'main.tsx')],
  outdir,
  naming: 'tui.js',
  target: 'bun',
  minify: true,
  plugins: [stubDevtools],
});
if (!result.success) {
  for (const log of result.logs) {
    console.error(log);
  }
  process.exit(1);
}
for (const output of result.outputs) {
  console.log(`${output.path} ${(output.size / 1024).toFixed(0)}KB`);
}
