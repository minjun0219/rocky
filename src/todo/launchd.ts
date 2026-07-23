import { existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { DEFAULT_TODO_DIR } from './config';

/**
 * launchd 상주 등록 — `rocky-todo daemon install` 이 쓰는 macOS 전용 헬퍼.
 *
 * KeepAlive 로 데몬을 로그인 세션 동안 상시 유지한다. 미설치 상태여도 CLI 의
 * 온디맨드 자동 기동은 그대로 동작하므로 install 은 선택 사항이다.
 */

export const LAUNCHD_LABEL = 'com.rocky.todo';

const PLIST_PATH = join(homedir(), 'Library', 'LaunchAgents', `${LAUNCHD_LABEL}.plist`);

function daemonEntryPath(): string {
  return join(import.meta.dir, 'daemon.ts');
}

function plistContent(dir: string): string {
  // 로그 경로는 데몬의 데이터 디렉터리(runtime.dir)를 따른다 — db/pid 와 같은 곳.
  const logPath = join(dir, 'daemon.log');
  return `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>${LAUNCHD_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>${process.execPath}</string>
    <string>run</string>
    <string>${daemonEntryPath()}</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>${logPath}</string>
  <key>StandardErrorPath</key><string>${logPath}</string>
</dict>
</plist>
`;
}

function launchctl(args: string[]): { ok: boolean; out: string } {
  // spawn 자체 실패(launchctl 없음/비-macOS 의 ENOENT 등)도 예외 대신 결과로 돌려준다.
  try {
    const proc = Bun.spawnSync({ cmd: ['launchctl', ...args], stdout: 'pipe', stderr: 'pipe' });
    return {
      ok: proc.exitCode === 0,
      out: `${proc.stdout.toString()}${proc.stderr.toString()}`.trim(),
    };
  } catch (error) {
    return { ok: false, out: error instanceof Error ? error.message : String(error) };
  }
}

function gid(): string {
  return `gui/${process.getuid?.() ?? 501}`;
}

/** launchd 는 macOS 전용 — 다른 OS 에서는 안내 문구를 돌려주고 명령을 건너뛴다. */
const NOT_MACOS_MESSAGE =
  'launchd 상주 등록은 macOS 전용이다 — 다른 OS 에서는 CLI 온디맨드 자동 기동만 사용한다';

/**
 * launchd 상주 등록. `dir` 은 데몬의 데이터 디렉터리(runtime.dir) — 로그 경로와
 * mkdir 대상을 이 값으로 맞춰 db/pid 와 로그가 같은 곳(ROCKY_TODO_DIR / rocky.json
 * todo.dir 반영)에 놓이게 한다. 미지정 시 기본 디렉터리.
 */
export function installLaunchd(dir: string = DEFAULT_TODO_DIR): string {
  if (process.platform !== 'darwin') {
    return NOT_MACOS_MESSAGE;
  }
  mkdirSync(join(homedir(), 'Library', 'LaunchAgents'), { recursive: true });
  mkdirSync(dir, { recursive: true });
  writeFileSync(PLIST_PATH, plistContent(dir));
  // 재설치를 멱등하게 — 이미 떠 있으면 내리고 다시 올린다
  launchctl(['bootout', gid(), PLIST_PATH]);
  const result = launchctl(['bootstrap', gid(), PLIST_PATH]);
  if (!result.ok) {
    return `launchd 등록 실패: ${result.out}\nplist: ${PLIST_PATH}`;
  }
  return `✓ launchd 등록 완료 (${LAUNCHD_LABEL}) — 로그인 시 자동 기동 + KeepAlive\n  plist: ${PLIST_PATH}`;
}

export function uninstallLaunchd(): string {
  if (process.platform !== 'darwin') {
    return NOT_MACOS_MESSAGE;
  }
  const result = launchctl(['bootout', gid(), PLIST_PATH]);
  if (existsSync(PLIST_PATH)) {
    rmSync(PLIST_PATH, { force: true });
  }
  return result.ok
    ? `✓ launchd 해제 완료 (${LAUNCHD_LABEL})`
    : `launchd 해제: 등록되어 있지 않았다 (plist 는 정리됨)`;
}

export function launchdStatus(): string {
  if (process.platform !== 'darwin') {
    return NOT_MACOS_MESSAGE;
  }
  if (!existsSync(PLIST_PATH)) {
    return 'launchd: 미등록 (온디맨드 자동 기동만 사용중)';
  }
  const result = launchctl(['print', `${gid()}/${LAUNCHD_LABEL}`]);
  if (!result.ok) {
    return `launchd: plist 는 있으나 로드되지 않음 (${PLIST_PATH})`;
  }
  const state = result.out.match(/state = (\w+)/)?.[1] ?? 'unknown';
  return `launchd: 등록됨, state=${state}`;
}
