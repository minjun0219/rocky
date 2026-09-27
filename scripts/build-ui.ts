/**
 * 웹 UI(`web/`)를 `dist/` 로 번들한다 — 데몬(`rockyd`)이 실행 파일 옆 `dist/index.html` 을 찾아
 * `/` 에 서빙한다(`crates/rockyd/src/main.rs::resolve_ui_dist`). 릴리스 워크플로가 tarball 에 넣는다.
 * bun 은 **UI 빌드 시에만** 필요하다 — 런타임은 전부 Rust.
 */
import { rmSync } from 'node:fs';
import { join } from 'node:path';
import tailwind from 'bun-plugin-tailwind';

// 경로는 레포 루트 기준 절대 경로 — cwd 에 기대면 다른 위치에서 부를 때 엉뚱한 dist/ 를 지운다.
const root = join(import.meta.dir, '..');
const outdir = join(root, 'dist');

// 청크 이름이 내용 해시라 이전 빌드의 청크가 남는다 — 매번 비우고 시작한다.
rmSync(outdir, { recursive: true, force: true });

const result = await Bun.build({
  entrypoints: [join(root, 'web/index.html')],
  outdir,
  plugins: [tailwind],
  minify: true,
  // 자산 참조를 루트 절대 경로로 — 기본값은 `./chunk-*.js` 라 퍼머링크(`/rocky/12`)
  // 새로고침에서 브라우저가 `/rocky/chunk-*.js` 를 찾는다. 데몬의 SPA fallback 은
  // 그 요청에 index.html 을 돌려주므로 청크가 HTML 로 와서 앱이 부팅하지 못한다.
  publicPath: '/',
});
if (!result.success) {
  for (const log of result.logs) {
    console.error(log);
  }
  process.exit(1);
}
console.log(`dist/ 에 ${result.outputs.length}개 산출물`);
