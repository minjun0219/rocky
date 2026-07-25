import { spawn } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { createOpencodeExecutor, detectOpencode, opencodeBin } from './core/opencode-cli';
import {
  type JobRequest,
  type JobStore,
  createJobStoreFromEnv,
  filterBySession,
  isTerminal,
  matchJobReference,
} from './core/opencode-jobs';
import { buildStatusSnapshot, renderResult, renderStatus, toSummary } from './core/opencode-render';
import { killJobProcess, runJob } from './core/opencode-runner';
import { loadConfig } from './core/rocky-config';

/**
 * `/rocky:opencode` 위임 런타임 — 슬래시 커맨드가 Bash 로 부르는 단일 진입점.
 *
 * 서브커맨드: `check` / `task` / `job-worker` / `status` / `result` / `cancel`.
 *
 * `job-worker` 는 사용자가 직접 부르는 게 아니라 `task --background` 가 **자기 자신을**
 * detached 로 재실행할 때 쓰는 내부 분기다. 별도 워커 파일을 두지 않는 이유는 요청 payload 를
 * argv 로 나르지 않기 위해서다 — 워커는 `--job-id` 하나만 받고 나머지는 잡 파일에서 읽는다.
 *
 * MCP 도구를 추가하지 않는다: 이 런타임은 Claude Code 슬래시 커맨드 전용이라
 * `src/index.ts` 의 도구 표면과 무관하다 (Codex / opencode 호스트에는 영향 없음).
 */

/** 파싱된 커맨드라인. */
export interface ParsedArgs {
  command: string;
  flags: Record<string, string | boolean>;
  positionals: string[];
}

/** boolean 으로 취급할 플래그 — 뒤 토큰을 값으로 삼키지 않는다. */
const BOOLEAN_FLAGS = new Set(['auto', 'background', 'json', 'continue', 'all', 'help']);

/**
 * 최소 인자 파서. `--key value` / `--key=value` / `--bool` / positional 만 다룬다.
 * 외부 파서를 들이지 않는 이유는 이 표면이 이 파일 안에서 끝나기 때문 (의존성 0 유지).
 */
export function parseArgs(argv: string[]): ParsedArgs {
  const [command = 'help', ...rest] = argv;
  const flags: Record<string, string | boolean> = {};
  const positionals: string[] = [];
  for (let i = 0; i < rest.length; i += 1) {
    const token = rest[i]!;
    if (!token.startsWith('--')) {
      positionals.push(token);
      continue;
    }
    const body = token.slice(2);
    const eq = body.indexOf('=');
    if (eq >= 0) {
      flags[body.slice(0, eq)] = body.slice(eq + 1);
      continue;
    }
    if (BOOLEAN_FLAGS.has(body)) {
      flags[body] = true;
      continue;
    }
    const next = rest[i + 1];
    if (next === undefined || next.startsWith('--')) {
      flags[body] = true;
      continue;
    }
    flags[body] = next;
    i += 1;
  }
  return { command, flags, positionals };
}

/** 문자열 플래그만 꺼낸다 (boolean 으로 들어온 값은 무시). */
function str(flags: ParsedArgs['flags'], key: string): string | undefined {
  const value = flags[key];
  return typeof value === 'string' && value.trim().length > 0 ? value.trim() : undefined;
}

/** stdin 을 전부 읽는다 (TTY 면 즉시 빈 문자열). */
async function readStdin(): Promise<string> {
  if (process.stdin.isTTY) {
    return '';
  }
  let raw = '';
  for await (const chunk of process.stdin) {
    raw += chunk;
  }
  return raw;
}

/**
 * 프롬프트 입력 3경로를 **`--prompt-file` → positional → stdin** 순으로 본다.
 *
 * 파일을 맨 앞에 두는 이유는 슬래시 커맨드가 긴 가드레일 프롬프트를 파일로 넘기기 때문이다 —
 * 명시적으로 파일을 지정했는데 뒤따르는 positional 이 그걸 덮어쓰면 조용히 엉뚱한 프롬프트로
 * 위임하게 된다.
 */
async function resolvePrompt(parsed: ParsedArgs): Promise<string> {
  const file = str(parsed.flags, 'prompt-file');
  if (file) {
    return readFileSync(file, 'utf8');
  }
  if (parsed.positionals.length > 0) {
    return parsed.positionals.join(' ');
  }
  return (await readStdin()).trim();
}

/** 잡 저장소를 연다 (`rocky.json` 의 `opencode` 블록 반영). */
async function openStore(cwd: string): Promise<JobStore> {
  const { config } = await loadConfig({ projectRoot: cwd });
  return createJobStoreFromEnv(config.opencode, process.env, cwd);
}

/** `rocky.json` 의 기본 model / agent 를 읽는다. */
async function defaults(cwd: string): Promise<{ model?: string; agent?: string }> {
  const { config } = await loadConfig({ projectRoot: cwd });
  return { model: config.opencode?.model, agent: config.opencode?.agent };
}

/** 현재 Claude 세션 id — SessionStart 훅이 주입한다. 없으면 세션 필터가 비활성. */
function sessionId(): string | undefined {
  const raw = process.env.ROCKY_SESSION_ID?.trim();
  return raw && raw.length > 0 ? raw : undefined;
}

function output(payload: unknown, rendered: string, asJson: boolean): void {
  process.stdout.write(asJson ? `${JSON.stringify(payload, null, 2)}\n` : `${rendered}\n`);
}

/** `check` — opencode CLI 설치 여부. */
async function handleCheck(parsed: ParsedArgs): Promise<number> {
  const detection = await detectOpencode(createOpencodeExecutor());
  const rendered = detection.available
    ? `opencode 사용 가능: ${detection.detail}`
    : `opencode 를 쓸 수 없습니다: ${detection.detail}\n설치: https://opencode.ai (또는 ROCKY_OPENCODE_CLI 로 경로 지정)`;
  output(detection, rendered, parsed.flags.json === true);
  return detection.available ? 0 : 1;
}

/** `task` — 잡 생성 후 foreground 실행 또는 detached 워커 스폰. */
async function handleTask(parsed: ParsedArgs): Promise<number> {
  const cwd = str(parsed.flags, 'cwd') ?? process.cwd();
  const worktree = str(parsed.flags, 'worktree');
  if (!worktree) {
    process.stderr.write('--worktree <path> 가 필요합니다 (opencode 가 작업할 격리 디렉터리).\n');
    return 2;
  }
  let prompt: string;
  try {
    prompt = await resolvePrompt(parsed);
  } catch (error) {
    // 프롬프트 파일이 없거나 못 읽는 건 흔한 사용자 실수다 — 스택 트레이스 대신 한 줄로 알린다.
    process.stderr.write(
      `프롬프트를 읽지 못했습니다: ${error instanceof Error ? error.message : String(error)}\n`,
    );
    return 2;
  }
  if (prompt.trim().length === 0) {
    process.stderr.write('프롬프트가 비어 있습니다 (positional / --prompt-file / stdin).\n');
    return 2;
  }

  const fallback = await defaults(cwd);
  const request: JobRequest = {
    prompt,
    worktree,
    branch: str(parsed.flags, 'branch'),
    model: str(parsed.flags, 'model') ?? fallback.model,
    agent: str(parsed.flags, 'agent') ?? fallback.agent,
    variant: str(parsed.flags, 'variant'),
    auto: parsed.flags.auto === true,
    resumeSession: str(parsed.flags, 'session'),
    continueLast: parsed.flags.continue === true,
    attach: str(parsed.flags, 'attach'),
  };

  const store = await openStore(cwd);
  const job = store.create({
    title: str(parsed.flags, 'title') ?? firstLine(prompt),
    workspaceRoot: cwd,
    request,
    sessionId: sessionId(),
  });

  if (parsed.flags.background === true) {
    const pid = spawnWorker(cwd, job.id);
    // pid 가 없으면 워커가 뜨지 못한 것이다. 이걸 성공처럼 보고하면 잡이 영원히 queued 로
    // 남아 사용자는 "시작됨" 만 보고 기다리게 된다 — 실패로 못박고 사유를 남긴다.
    if (!pid) {
      const failed = store.update(job.id, {
        status: 'failed',
        phase: 'done',
        errorMessage: '백그라운드 워커를 띄우지 못했습니다 (pid 없음).',
        completedAt: new Date().toISOString(),
      });
      store.appendLog(job.id, '백그라운드 워커 spawn 실패 (pid 없음)');
      process.stderr.write(`백그라운드 워커를 띄우지 못했습니다: ${job.id}\n`);
      output(failed, renderResult(failed, new Date()), parsed.flags.json === true);
      return 1;
    }
    const queued = store.update(job.id, { pid, phase: 'queued' });
    store.appendLog(job.id, `백그라운드 워커 spawn (pid ${pid})`);
    const jobsCommand = `/rocky:opencode-jobs`;
    output(
      queued,
      [
        `백그라운드 잡 시작: ${job.id}`,
        `worktree: ${worktree}`,
        `진행 확인: ${jobsCommand} status ${job.id}`,
        `결과 확인: ${jobsCommand} result ${job.id}`,
      ].join('\n'),
      parsed.flags.json === true,
    );
    return 0;
  }

  const done = await runJob(store, job.id, createOpencodeExecutor());
  output(done, renderResult(done, new Date()), parsed.flags.json === true);
  return done.status === 'completed' ? 0 : 1;
}

/** 프롬프트 첫 줄을 제목으로 (최대 60자). */
function firstLine(prompt: string): string {
  const line =
    prompt
      .split('\n')
      .find((l) => l.trim().length > 0)
      ?.trim() ?? 'opencode task';
  return line.length > 60 ? `${line.slice(0, 57)}...` : line;
}

/**
 * 자기 자신을 `job-worker` 로 detached 재실행한다.
 *
 * `detached: true` 로 새 프로세스 그룹을 만드는 게 핵심이다 — 이래야 (a) 부모 Bash 호출이
 * 끝나도 워커가 살아남고, (b) `cancel` 이 `kill(-pid)` 로 워커와 그 자식 opencode 를 한 번에
 * 끊을 수 있다. `stdio: "ignore"` 라 워커의 출력은 아무도 안 읽는다 — 결과는 전부 잡 파일로 간다.
 */
function spawnWorker(cwd: string, jobId: string): number {
  const scriptPath = fileURLToPath(import.meta.url);
  const child = spawn(
    process.execPath,
    [scriptPath, 'job-worker', '--cwd', cwd, '--job-id', jobId],
    {
      cwd,
      env: process.env,
      detached: true,
      stdio: 'ignore',
    },
  );
  child.unref();
  return child.pid ?? 0;
}

/**
 * `job-worker` — 내부 분기. detached 프로세스로 실행된다.
 *
 * 예외를 밖으로 내보내지 않는다. 워커는 stdout 이 버려진 detached 프로세스라 던져봐야
 * 아무도 읽지 못하고, 잡은 `queued` 인 채로 영원히 남아 사용자가 이유도 모른 채 기다리게 된다.
 * `runJob` 이 흡수하지 못하는 실패(config 로드 실패 / 잡 파일 손상 / 없는 job-id)까지
 * 여기서 받아 **가능한 한** 잡에 사유를 남긴다.
 */
async function handleWorker(parsed: ParsedArgs): Promise<number> {
  const cwd = str(parsed.flags, 'cwd') ?? process.cwd();
  const jobId = str(parsed.flags, 'job-id');
  if (!jobId) {
    return 2;
  }
  try {
    const store = await openStore(cwd);
    const done = await runJob(store, jobId, createOpencodeExecutor(), { pid: process.pid });
    return done.status === 'completed' ? 0 : 1;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    try {
      const store = await openStore(cwd);
      store.appendLog(jobId, `워커 실패: ${message}`);
      store.update(jobId, {
        status: 'failed',
        phase: 'done',
        errorMessage: `백그라운드 워커가 실패했습니다: ${message}`,
        completedAt: new Date().toISOString(),
      });
    } catch {
      // 저장소 자체를 열 수 없으면 남길 곳이 없다 — 종료 코드로만 알린다.
    }
    return 1;
  }
}

/** 이 세션이 볼 수 있는 잡 목록 (최신순). `--all` 이면 세션 필터를 끈다. */
async function visibleJobs(parsed: ParsedArgs, cwd: string) {
  const store = await openStore(cwd);
  const all = store.list();
  const jobs = parsed.flags.all === true ? all : filterBySession(all, sessionId());
  return { store, jobs };
}

/** `status` — 진행 중 / 최근 잡 스냅샷. */
async function handleStatus(parsed: ParsedArgs): Promise<number> {
  const cwd = str(parsed.flags, 'cwd') ?? process.cwd();
  const { store, jobs } = await visibleJobs(parsed, cwd);
  const now = new Date();
  // job-ref 를 주면 그 잡 하나로 좁힌다. 문서와 usage 가 `status [job-ref]` 를 약속하므로
  // 무시하고 전체를 뿌리면 잘못된 / 모호한 참조까지 조용히 통과해버린다.
  let target = jobs;
  const ref = parsed.positionals[0];
  if (ref) {
    try {
      target = [matchJobReference(jobs, ref)];
    } catch (error) {
      process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
      return 1;
    }
  }
  const summaries = target.map((job) =>
    toSummary(job, now, isTerminal(job.status) ? [] : store.readLogTail(job.id, 3)),
  );
  const snapshot = buildStatusSnapshot(summaries);
  output(snapshot, renderStatus(snapshot), parsed.flags.json === true);
  return 0;
}

/** `result` — 종료된 잡의 최종 출력. */
async function handleResult(parsed: ParsedArgs): Promise<number> {
  const cwd = str(parsed.flags, 'cwd') ?? process.cwd();
  const { jobs } = await visibleJobs(parsed, cwd);
  try {
    const job = matchJobReference(jobs, parsed.positionals[0]);
    output(job, renderResult(job, new Date()), parsed.flags.json === true);
    return isTerminal(job.status) ? 0 : 1;
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    return 1;
  }
}

/** `cancel` — 진행 중 잡의 프로세스 그룹을 끊고 cancelled 로 기록. */
async function handleCancel(parsed: ParsedArgs): Promise<number> {
  const cwd = str(parsed.flags, 'cwd') ?? process.cwd();
  const { store, jobs } = await visibleJobs(parsed, cwd);
  const active = jobs.filter((job) => !isTerminal(job.status));
  try {
    const job = matchJobReference(active, parsed.positionals[0]);
    const outcome = killJobProcess(job);
    store.appendLog(job.id, `취소 요청: ${outcome.detail}`);
    const cancelled = store.update(job.id, {
      status: 'cancelled',
      phase: 'done',
      errorMessage: `사용자 취소 (${outcome.detail})`,
      completedAt: new Date().toISOString(),
    });
    output(cancelled, `잡 ${job.id} 취소됨 — ${outcome.detail}`, parsed.flags.json === true);
    return 0;
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    return 1;
  }
}

const USAGE = `rocky opencode companion — /rocky:opencode 위임 런타임

실행: bun run "\${CLAUDE_PLUGIN_ROOT}/src/opencode-companion.ts" <서브커맨드> [옵션]
(사용자는 보통 이걸 직접 부르지 않는다 — /rocky:opencode 와 /rocky:opencode-jobs 가 감싼다)

  check                                opencode CLI 사용 가능 여부
  task --worktree <path> [옵션] <prompt>  위임 실행 (기본 foreground)
  status [job-ref]                       진행 중 / 최근 잡
  result [job-ref]                       종료된 잡의 최종 출력
  cancel [job-ref]                       진행 중 잡 취소

옵션: --background --json --all --cwd <path> --branch <name> --model <provider/model>
      --agent <name> --variant <effort> --auto --session <id> --continue
      --attach <url> --title <text> --prompt-file <path>

프롬프트는 positional / --prompt-file / stdin 중 하나로 준다.
바이너리는 ROCKY_OPENCODE_CLI (기본 ${opencodeBin()}) 로 지정한다.`;

async function main(): Promise<void> {
  const parsed = parseArgs(process.argv.slice(2));
  let code = 0;
  switch (parsed.command) {
    case 'check':
      code = await handleCheck(parsed);
      break;
    case 'task':
      code = await handleTask(parsed);
      break;
    case 'job-worker':
      code = await handleWorker(parsed);
      break;
    case 'status':
      code = await handleStatus(parsed);
      break;
    case 'result':
      code = await handleResult(parsed);
      break;
    case 'cancel':
      code = await handleCancel(parsed);
      break;
    default:
      process.stdout.write(`${USAGE}\n`);
      code = parsed.command === 'help' ? 0 : 2;
  }
  process.exit(code);
}

// 직접 실행될 때만 main 을 돈다 — 테스트가 parseArgs 만 import 할 수 있게.
if (import.meta.main) {
  await main();
}
