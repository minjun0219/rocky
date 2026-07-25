import { spawn } from 'node:child_process';
import type { JobRequest } from './opencode-jobs';

/**
 * opencode CLI 위임 어댑터.
 *
 * rocky 는 opencode 의 인증 / provider 설정을 직접 다루지 않는다 — 사용자가 이미 로그인해 둔
 * `opencode` CLI 를 그대로 호출할 뿐이다 (`ntn` 위임과 같은 정책). 실제 spawn 은
 * `OpencodeExecutor` 뒤에 숨겨 테스트에서 fake 로 대체한다.
 *
 * `codex app-server` 같은 장수 RPC 서버를 쓰지 않는 이유: `opencode run` 은 one-shot 이라
 * 브로커 / 엔드포인트 / PID 관리 계층이 통째로 불필요하다. 대신 잡 상태는 `opencode-jobs.ts`
 * 의 파일 저장소가 들고 있는다.
 */

/** CLI 한 번 실행 결과. */
export interface OpencodeRunResult {
  stdout: string;
  stderr: string;
  exitCode: number;
}

/** 실행 옵션. */
export interface OpencodeRunOptions {
  cwd?: string;
  timeoutMs?: number;
  /**
   * opencode 프로세스가 뜨는 즉시 그 pid 를 알린다.
   *
   * 호출자(runJob)가 이 pid 를 잡에 기록해야 `cancel` / `SessionEnd` 가 opencode 를 정확히
   * 끊을 수 있다 — opencode 는 자체 프로세스 그룹으로 떨어져 나가므로 워커 그룹을 끊는 것만으로는
   * 닿지 않는다.
   */
  onSpawn?: (pid: number) => void;
}

/** spawn 을 추상화한 실행기. 테스트는 이 인터페이스를 fake 로 구현한다. */
export interface OpencodeExecutor {
  run(args: string[], options?: OpencodeRunOptions): Promise<OpencodeRunResult>;
}

/**
 * 프로세스 **그룹** 전체에 신호를 보낸다 (`kill(-pid)`).
 *
 * 단일 pid 만 죽이면 그 프로세스가 띄운 자식들이 살아남는다 — opencode 는 `bash` 같은 도구
 * 프로세스를 띄우므로, 그것들이 남으면 실패를 보고한 뒤에도 worktree 를 계속 수정할 수 있다.
 * worktree 격리가 유일한 봉쇄 수단인 설계에서 이건 치명적이라 항상 그룹 단위로 끊는다.
 *
 * 그룹이 없으면(detached 로 안 떴거나 이미 리핑됨) 단일 프로세스로 재시도한다.
 */
export function killProcessGroup(
  pid: number,
  signal: NodeJS.Signals = 'SIGTERM',
  kill: (target: number, sig: NodeJS.Signals) => void = process.kill,
): { killed: boolean; detail: string } {
  try {
    kill(-pid, signal);
    return { killed: true, detail: `프로세스 그룹 ${pid} 에 ${signal} 전송` };
  } catch (groupError) {
    try {
      kill(pid, signal);
      return { killed: true, detail: `프로세스 ${pid} 에 ${signal} 전송 (그룹 없음)` };
    } catch {
      const message = groupError instanceof Error ? groupError.message : String(groupError);
      return { killed: false, detail: `이미 종료됐거나 죽일 수 없습니다: ${message}` };
    }
  }
}

export const OPENCODE_DEFAULT_BIN = 'opencode';

/** 사용할 CLI 바이너리. `ROCKY_OPENCODE_CLI` 로 오버라이드 (기본 `opencode`). */
export function opencodeBin(env: NodeJS.ProcessEnv = process.env): string {
  const override = env.ROCKY_OPENCODE_CLI?.trim();
  return override && override.length > 0 ? override : OPENCODE_DEFAULT_BIN;
}

/** CLI 탐지 결과. */
export interface OpencodeDetection {
  available: boolean;
  version?: string;
  detail: string;
}

/**
 * `opencode --version` 으로 설치 여부를 확인한다.
 * 실패는 전부 "미설치" 로 흡수한다 — 탐지 단계에서 던지면 위임 커맨드가 안내 대신 스택을 뱉는다.
 */
export async function detectOpencode(executor: OpencodeExecutor): Promise<OpencodeDetection> {
  try {
    const result = await executor.run(['--version'], { timeoutMs: 10_000 });
    if (result.exitCode !== 0) {
      return { available: false, detail: `opencode --version exited ${result.exitCode}` };
    }
    const version = result.stdout.trim().split('\n').pop()?.trim();
    return { available: true, version, detail: `opencode ${version ?? '(unknown version)'}` };
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    return { available: false, detail: `opencode not found on PATH: ${message}` };
  }
}

/**
 * `opencode run` argv 를 만든다.
 *
 * 프롬프트는 **맨 마지막 positional** 로 붙인다. shell 없이 `spawn(bin, argv)` 로 넘기므로
 * 인용 문제는 발생하지 않는다 (셸 문자열로 조립하면 멀티라인 프롬프트에서 곧바로 깨진다).
 *
 * `--model` 을 항상 넘기라고 권하는 이유: opencode 는 config 에 top-level `model` 이 없으면
 * **"마지막에 쓴 모델" 로 조용히 폴백**한다. 위임 결과의 재현성을 위해 호출자가 명시해야 한다.
 * 마찬가지로 `--agent` 미지정 시 write 권한이 있는 `build` 로 폴백한다.
 */
export function buildRunArgs(request: JobRequest): string[] {
  const args = ['run', '--format', 'json', '--dir', request.worktree];
  if (request.model) {
    args.push('--model', request.model);
  }
  if (request.agent) {
    args.push('--agent', request.agent);
  }
  if (request.variant) {
    args.push('--variant', request.variant);
  }
  if (request.resumeSession) {
    args.push('--session', request.resumeSession);
  } else if (request.continueLast) {
    args.push('--continue');
  }
  if (request.attach) {
    args.push('--attach', request.attach);
  }
  if (request.auto) {
    args.push('--auto');
  }
  args.push(request.prompt);
  return args;
}

/** 위임 실행 기본 타임아웃 — 구현 작업은 길다. `ROCKY_OPENCODE_TIMEOUT_MS` 로 조정. */
export const OPENCODE_DEFAULT_TIMEOUT_MS = 30 * 60 * 1000;

/** 실행 타임아웃 (ms). 잘못된 값은 기본값으로 흡수한다. */
export function opencodeTimeoutMs(env: NodeJS.ProcessEnv = process.env): number {
  const parsed = Number.parseInt(env.ROCKY_OPENCODE_TIMEOUT_MS ?? '', 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : OPENCODE_DEFAULT_TIMEOUT_MS;
}

/**
 * `node:child_process` 백엔드.
 *
 * `Bun.spawn` 이 아니라 node API 를 쓰는 이유는 **`detached`** 때문이다. opencode 를 자체
 * 프로세스 그룹의 리더로 띄워야 타임아웃 시 `kill(-pid)` 로 opencode 가 만든 도구 프로세스
 * (`bash` 등)까지 한 번에 끊을 수 있다. 단일 pid 만 죽이면 손자들이 살아남아, 잡이 `failed` 로
 * 기록된 뒤에도 worktree 를 계속 건드린다.
 *
 * 그 대가로 opencode 는 더 이상 워커의 프로세스 그룹에 속하지 않는다 — 그래서 실행 즉시
 * `onSpawn` 으로 pid 를 넘겨 잡에 기록하고, `cancel` / `SessionEnd` 가 **워커 그룹과 opencode
 * 그룹을 모두** 끊도록 한다.
 *
 * 타임아웃은 AbortSignal 이 아니라 직접 건다: `signal` 은 SIGTERM 만 보내므로 자식이 이를
 * 무시하면 그대로 매달린다.
 */
export function createOpencodeExecutor(bin: string = opencodeBin()): OpencodeExecutor {
  return {
    run(args, options) {
      return new Promise<OpencodeRunResult>((resolve, reject) => {
        const child = spawn(bin, args, {
          cwd: options?.cwd,
          stdio: ['ignore', 'pipe', 'pipe'],
          detached: true,
        });
        if (child.pid) {
          options?.onSpawn?.(child.pid);
        }

        let stdout = '';
        let stderr = '';
        child.stdout?.on('data', (chunk) => {
          stdout += chunk;
        });
        child.stderr?.on('data', (chunk) => {
          stderr += chunk;
        });

        const timeoutMs = options?.timeoutMs ?? opencodeTimeoutMs();
        let timedOut = false;
        const timer = setTimeout(() => {
          timedOut = true;
          if (child.pid) {
            killProcessGroup(child.pid, 'SIGKILL');
          }
          reject(new Error(`opencode 실행이 ${timeoutMs}ms 안에 끝나지 않아 중단했습니다.`));
        }, timeoutMs);

        child.on('error', (error) => {
          clearTimeout(timer);
          reject(error);
        });
        child.on('close', (code, signal) => {
          clearTimeout(timer);
          if (timedOut) {
            return; // 이미 reject 했다.
          }
          // 신호로 죽었으면 exit code 가 null 이다 — 성공(0)으로 오해하지 않게 별도 코드를 준다.
          resolve({ stdout, stderr, exitCode: code ?? (signal ? 137 : 0) });
        });
      });
    },
  };
}

/** NDJSON 스트림에서 뽑아낸 요약. */
export interface RunEventSummary {
  /** opencode 세션 id — 후속 `--session` 재개에 쓴다. */
  sessionId?: string;
  /** 마지막 assistant 텍스트 (최종 출력). */
  text: string;
  /** 스트림에서 관측한 에러 메시지들. */
  errors: string[];
  /** 파싱된 이벤트 수 — 0 이면 NDJSON 이 아니었다는 뜻. */
  eventCount: number;
}

/** 중첩 객체에서 첫 번째로 발견한 문자열 필드를 꺼낸다 (스키마 변화에 관대하게). */
function pickString(source: unknown, keys: string[]): string | undefined {
  if (!source || typeof source !== 'object') {
    return undefined;
  }
  const record = source as Record<string, unknown>;
  for (const key of keys) {
    const value = record[key];
    if (typeof value === 'string' && value.trim().length > 0) {
      return value;
    }
  }
  return undefined;
}

/**
 * `opencode run --format json` 의 NDJSON 출력을 요약한다.
 *
 * opencode 의 이벤트 스키마는 버전마다 흔들리므로 **키 이름을 관대하게** 훑는다 — 텍스트로
 * 보이는 필드를 모으고, 세션 id 로 보이는 필드를 잡고, 에러를 따로 담는다. NDJSON 이 전혀
 * 아니면 (eventCount 0) 호출자가 stdout 원문을 그대로 결과로 쓴다.
 */
export function summarizeRunEvents(stdout: string): RunEventSummary {
  // part id → 최신 텍스트. Map 이 삽입 순서를 보존하므로 등장 순서대로 이어붙일 수 있다.
  // 같은 part 가 갱신되며 여러 번 나올 수 있어 (스트리밍) 단순 append 하면 중복 누적된다.
  const parts = new Map<string, string>();
  const loose: string[] = [];
  const errors: string[] = [];
  let sessionId: string | undefined;
  let eventCount = 0;

  for (const line of stdout.split('\n')) {
    const trimmed = line.trim();
    if (trimmed.length === 0 || !trimmed.startsWith('{')) {
      continue;
    }
    let event: Record<string, unknown>;
    try {
      event = JSON.parse(trimmed) as Record<string, unknown>;
    } catch {
      continue;
    }
    eventCount += 1;

    sessionId ??=
      pickString(event, ['sessionID', 'sessionId', 'session_id']) ??
      pickString(event.properties, ['sessionID', 'sessionId', 'session_id']) ??
      pickString(event.info, ['sessionID', 'sessionId', 'session_id', 'id']);

    const type = typeof event.type === 'string' ? event.type : '';
    if (type.includes('error') || event.error) {
      // 실제 메시지는 `error.data.message` 에 있다. `error.name` 은 'UnknownError' 같은
      // 분류명뿐이라 먼저 집으면 정작 원인을 잃는다 — data → message → name 순으로 본다.
      const errorNode = event.error as Record<string, unknown> | undefined;
      const message =
        pickString(errorNode?.data, ['message']) ??
        pickString(errorNode, ['message']) ??
        pickString(event, ['message']) ??
        pickString(errorNode, ['name']) ??
        trimmed.slice(0, 300);
      errors.push(message);
      continue;
    }

    const text =
      pickString(event.part, ['text']) ??
      pickString(event, ['text']) ??
      pickString(event.properties, ['text']);
    if (!text) {
      continue;
    }
    const partId =
      pickString(event.part, ['id']) ?? pickString(event.properties, ['id']) ?? undefined;
    if (partId) {
      parts.set(partId, text);
    } else {
      loose.push(text);
    }
  }

  const text = [...parts.values(), ...loose].join('').trim();
  return { sessionId, text, errors, eventCount };
}

/**
 * 실행 결과를 사람이 읽을 최종 텍스트로 정리한다.
 * NDJSON 파싱이 아무것도 못 건졌으면 stdout 원문으로 폴백한다 — 포맷이 바뀌어도 결과를 잃지 않게.
 */
export function renderRunOutput(result: OpencodeRunResult, summary: RunEventSummary): string {
  if (summary.text.length > 0) {
    return summary.text;
  }
  if (summary.eventCount === 0 && result.stdout.trim().length > 0) {
    return result.stdout.trim();
  }
  if (summary.errors.length > 0) {
    return summary.errors.join('\n');
  }
  return result.stderr.trim();
}
