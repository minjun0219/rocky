import { chmodSync, existsSync, readFileSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';

/**
 * statusline 코어 — 번들 템플릿을 설치 안정 경로로 동기화한다 (순수/DI).
 *
 * Claude Code 의 `statusLine` 설정은 user `settings.json` 에만 살 수 있어 (플러그인
 * `settings.json` 은 `agent`/`subagentStatusLine` 만 지원), rocky 는 이렇게 우회한다:
 *   - 스크립트 원본은 번들 템플릿 `statusline/<name>.sh` (레포에서 버전 관리, 복수 제공),
 *   - 설치는 `/rocky:statusline` 커맨드가 고른 템플릿을 안정 경로
 *     (`~/.config/rocky/statusline.sh`)로 복사하고 user settings 의 `statusLine.command`
 *     를 그 경로로 1회 지정,
 *   - 이후 플러그인 업데이트는 `SessionStart` 훅(`src/hooks/sync-statusline.ts`)이
 *     설치본 헤더의 템플릿 마커(`# rocky-statusline-template: <name>`)를 읽어
 *     같은 템플릿에서 자동 전파한다.
 * 플러그인 캐시 경로는 버전마다 바뀌므로 settings 가 캐시를 직접 가리키면 안 된다 —
 * 안정 경로 간접화가 이 설계의 핵심.
 */

/** 마커 없는(구형) 설치본을 동기화할 때 쓰는 기본 템플릿. */
export const DEFAULT_TEMPLATE = 'duo';

/**
 * 설치본 헤더의 템플릿 마커 — 템플릿 이름은 파일명 제약과 동일하게 [a-zA-Z0-9_-]+.
 * CRLF 설치본(사용자가 Windows 에디터로 수정한 경우)도 매칭되게 줄 끝 \r 을 허용한다 —
 * 아니면 마커 파싱이 실패해 사용자가 고른 템플릿이 기본값으로 덮인다.
 */
const TEMPLATE_MARKER = /^# rocky-statusline-template: ([a-zA-Z0-9_-]+)\r?$/m;

/** sync 대상 경로 쌍. 테스트/훅이 임의 경로를 주입할 수 있게 파라미터화. */
export interface StatuslineSyncOptions {
  /** 번들 템플릿 디렉터리 (`<pluginRoot>/statusline`). */
  bundledDir: string;
  /** 설치된 안정 경로 (`~/.config/rocky/statusline.sh`). */
  targetPath: string;
}

export type StatuslineSyncResult = 'not-installed' | 'up-to-date' | 'updated';

/** 설치 안정 경로 — user settings 의 `statusLine.command` 가 가리키는 곳. */
export function defaultStatuslinePath(): string {
  return join(homedir(), '.config', 'rocky', 'statusline.sh');
}

/** 플러그인 루트 기준 번들 템플릿 디렉터리. */
export function bundledStatuslineDir(pluginRoot: string): string {
  return join(pluginRoot, 'statusline');
}

/** 스크립트 내용에서 템플릿 마커를 읽는다. 없거나 이름이 어긋나면 null. */
export function parseTemplateName(content: string): string | null {
  return TEMPLATE_MARKER.exec(content)?.[1] ?? null;
}

/**
 * 설치본이 가리키는 템플릿을 번들에서 찾아 동기화한다. 안정 경로에 파일이 없으면 아직
 * `/rocky:statusline` 로 설치하지 않은 것이므로 아무것도 만들지 않는다 (opt-in 유지).
 * 내용이 다를 때만 덮어쓰며, 실행 권한(0o755)은 내용과 무관하게 항상 보장한다.
 */
export function syncStatusline(options: StatuslineSyncOptions): StatuslineSyncResult {
  const { bundledDir, targetPath } = options;
  if (!existsSync(targetPath)) {
    return 'not-installed';
  }
  const target = readFileSync(targetPath, 'utf8');
  const templateName = parseTemplateName(target) ?? DEFAULT_TEMPLATE;
  const sourcePath = join(bundledDir, `${templateName}.sh`);
  if (!existsSync(sourcePath)) {
    throw new Error(`bundled statusline template "${templateName}" not found: ${sourcePath}`);
  }
  const source = readFileSync(sourcePath, 'utf8');
  const result = source === target ? 'up-to-date' : 'updated';
  if (result === 'updated') {
    writeFileSync(targetPath, source);
  }
  // 내용이 같아도 실행 비트가 유실됐을 수 있으므로 (사용자 chmod / 파일 복원) 항상 복구한다.
  chmodSync(targetPath, 0o755);
  return result;
}
