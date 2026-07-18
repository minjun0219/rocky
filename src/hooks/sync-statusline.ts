import { join } from 'node:path';
import { bundledStatuslineDir, defaultStatuslinePath, syncStatusline } from '../core/statusline';

/**
 * SessionStart hook: 번들 statusline 템플릿을 설치 안정 경로로 동기화한다.
 *   번들(`statusline/<template>.sh`, 설치본 헤더 마커로 선택) → `~/.config/rocky/statusline.sh`.
 * 설치 경로에 파일이 없으면(= `/rocky:statusline` 미설치) 아무것도 하지 않는다.
 * 어떤 실패도 세션 시작을 막지 않도록 항상 exit 0.
 */

function run(): void {
  // 이 파일은 <pluginRoot>/src/hooks/ 에 산다 — 두 단계 위가 플러그인 루트.
  const pluginRoot = join(import.meta.dir, '..', '..');
  const result = syncStatusline({
    bundledDir: bundledStatuslineDir(pluginRoot),
    targetPath: defaultStatuslinePath(),
  });
  if (result === 'updated') {
    process.stderr.write('[rocky statusline] synced bundled script to install path\n');
  }
}

if (import.meta.main) {
  try {
    run();
  } catch (error) {
    // 절대 세션 시작을 막지 않는다 — 오류는 stderr 로만 남긴다.
    process.stderr.write(`[rocky statusline] sync skipped: ${String(error)}\n`);
  } finally {
    process.exit(0);
  }
}
