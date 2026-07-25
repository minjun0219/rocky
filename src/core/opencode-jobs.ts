import { randomBytes } from 'node:crypto';
import {
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  unlinkSync,
  writeFileSync,
} from 'node:fs';
import { appendFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { defaultProjectKey, expandTilde } from './worklog';

/**
 * opencode 위임 잡(job) 저장소 — `/rocky:opencode` 백그라운드 실행의 상태 레이어.
 *
 * 설계 배경: 백그라운드 워커는 부모(슬래시 커맨드의 Bash 호출)와 **파일로만** 통신한다.
 * detached + `stdio:"ignore"` 로 떼어낸 자식의 stdout 은 아무도 읽지 않으므로, 요청 payload
 * 도 결과도 전부 디스크를 경유한다. 그래서 이 모듈이 유일한 IPC 채널이다.
 *
 * 디스크 레이아웃:
 *   <dir>/state.json          { version, jobs: JobIndexEntry[] }  ← 가벼운 목록 (정렬 / GC)
 *   <dir>/jobs/<id>.json      JobRecord 전체 (request / result 포함)
 *   <dir>/jobs/<id>.log       진행 로그 (append-only, "[iso] message")
 *
 * 2단으로 나눈 이유는 `status` 가 잡 목록만 보려고 무거운 payload 를 전부 읽지 않게 하려는 것.
 * 인덱스에서 prune 되어 떨어져 나간 잡은 payload / log 파일도 같이 지운다 (GC 를 저장 경로에 인라인).
 *
 * 동기 API 인 이유: 소비자가 (a) 짧게 살다 죽는 CLI 서브커맨드와 (b) hook 스크립트뿐이라
 * 비동기로 얻을 이득이 없고, 워커 종료 직전 마지막 기록이 유실되지 않는 편이 중요하다.
 */

/** 잡 디렉터리의 부모. `ROCKY_OPENCODE_JOBS_DIR` 로 통째로 덮어쓴다. */
export const DEFAULT_JOBS_ROOT = join(homedir(), '.config', 'rocky', 'jobs');

/** 인덱스 파일 이름. */
export const STATE_FILE = 'state.json';

/** 인덱스에 남기는 최대 잡 수. 넘치면 오래된 것부터 파일까지 지운다. */
export const MAX_JOBS = 50;

/** 저장 포맷 버전 — 뒤에 모양이 바뀌면 올린다. */
export const STATE_VERSION = 1;

/**
 * 잡 상태.
 * - `queued`: 레코드만 만들어졌고 아직 워커가 집지 않음
 * - `running`: 워커가 opencode 를 띄운 상태
 * - `completed` / `failed` / `cancelled`: 종료 상태 (더 이상 변하지 않음)
 */
export type JobStatus = 'queued' | 'running' | 'completed' | 'failed' | 'cancelled';

/** 종료 상태 판별 — `status` / `result` / `cancel` 이 공유한다. */
export function isTerminal(status: JobStatus): boolean {
  return status === 'completed' || status === 'failed' || status === 'cancelled';
}

/**
 * 워커가 opencode 를 다시 띄우는 데 필요한 요청 전량.
 * 부모는 이 객체를 잡 파일에 써두고 `--job-id` 만 넘긴다 — argv 로 프롬프트를 나르지 않는다.
 */
export interface JobRequest {
  /** opencode 에 넘길 프롬프트 본문. 워커가 **stdin 으로** 전달한다. */
  prompt: string;
  /** opencode 가 작업할 디렉터리 (격리 worktree 경로). */
  worktree: string;
  /** 위임에 사용한 브랜치 이름 (있으면 보고에 쓴다). */
  branch?: string;
  /**
   * `provider/model`. **항상 명시하는 것을 권장** — 미지정 시 opencode 는 config 의
   * top-level `model` 이 없으면 "마지막 사용 모델" 로 조용히 폴백한다.
   */
  model?: string;
  /** opencode agent 이름. 미지정 시 write 권한이 있는 `build` 로 폴백한다. */
  agent?: string;
  /** provider 별 reasoning effort (`--variant`). */
  variant?: string;
  /** 권한 자동 승인. worktree 격리를 전제로만 켠다. */
  auto?: boolean;
  /** 이어갈 opencode 세션 id (`-s`). */
  resumeSession?: string;
  /** 직전 세션 이어가기 (`-c`). */
  continueLast?: boolean;
  /**
   * 이미 떠 있는 opencode 서버 URL (`--attach`).
   *
   * 콜드 스타트로 `opencode run` 을 돌리면 MCP 부팅 때문에 수 분간 무출력으로 매달릴 수
   * 있다 (실측). 서버가 이미 떠 있으면 거기 붙는 편이 훨씬 빠르고 안전하다.
   */
  attach?: string;
}

/** 잡 레코드 한 건. */
export interface JobRecord {
  id: string;
  kind: 'task';
  title: string;
  status: JobStatus;
  /** 사람이 읽는 진행 단계 — 명시 갱신만 한다 (로그 문자열로 역추정하지 않는다). */
  phase: string;
  /** 잡을 만든 Claude 세션 id. 조회 시 이 값으로 남의 잡을 걸러낸다. */
  sessionId?: string;
  /** 위임을 건 저장소 루트. */
  workspaceRoot: string;
  request: JobRequest;
  /** 워커 프로세스 pid. `detached` 로 띄우므로 취소는 `kill(-pid)`. */
  pid?: number;
  /** 진행 로그 파일 절대 경로. */
  logFile: string;
  /**
   * 단조 증가 정렬 키. `createdAt` 만으로 정렬하면 같은 ms 에 만들어진 잡의 순서가
   * 정해지지 않아 "최신 잡" 이 흔들린다 — 그래서 별도 시퀀스를 둔다.
   */
  seq: number;
  /** opencode 가 알려준 세션 id — 후속 `-s` 재개에 쓴다. */
  sessionRef?: string;
  /** opencode 프로세스 종료 코드. */
  exitCode?: number;
  /** 최종 출력 (opencode 의 마지막 assistant 텍스트). */
  result?: string;
  /** 실패 사유. */
  errorMessage?: string;
  createdAt: string;
  startedAt?: string;
  updatedAt: string;
  completedAt?: string;
}

/** 인덱스에 남기는 축약형. payload 를 열지 않고도 목록 / 정렬 / GC 가 되게 한다. */
interface JobIndexEntry {
  id: string;
  title: string;
  status: JobStatus;
  phase: string;
  sessionId?: string;
  workspaceRoot: string;
  createdAt: string;
  updatedAt: string;
  seq: number;
}

interface StateFile {
  version: number;
  /** 다음 잡에 줄 시퀀스 번호. prune 되어도 되감기지 않는다. */
  nextSeq: number;
  jobs: JobIndexEntry[];
}

/** 최신 우선 비교자 — seq 내림차순, 없으면 createdAt 으로 폴백. */
function byNewest(a: JobIndexEntry, b: JobIndexEntry): number {
  if (a.seq !== b.seq) {
    return b.seq - a.seq;
  }
  return a.createdAt < b.createdAt ? 1 : -1;
}

/** `JobStore.create` 입력 — 나머지 필드는 저장소가 채운다. */
export interface CreateJobInput {
  title: string;
  workspaceRoot: string;
  request: JobRequest;
  sessionId?: string;
  phase?: string;
}

/** `rocky.json` 의 `opencode` 블록 중 잡 저장소가 쓰는 부분. */
export interface OpencodeJobsConfig {
  /** 잡 디렉터리를 통째로 지정. `~` 확장 지원. */
  dir?: string;
  /** 보관할 최대 잡 수 (기본 50). */
  maxJobs?: number;
}

/**
 * 잡 id — `<prefix>-<base36 timestamp>-<rand6>`.
 * 앞부분이 시간순이라 prefix 매칭이 자연스럽고, 뒤 6자로 같은 ms 충돌을 막는다.
 */
export function newJobId(prefix = 'oc'): string {
  const ts = Date.now().toString(36);
  const rand = randomBytes(3).toString('hex');
  return `${prefix}-${ts}-${rand}`;
}

/** 프로젝트별 기본 잡 디렉터리 (`~/.config/rocky/jobs/<project-key>`). */
export function resolveDefaultJobsDir(cwd: string = process.cwd()): string {
  return join(DEFAULT_JOBS_ROOT, defaultProjectKey(cwd));
}

/**
 * 잡 목록에서 참조 하나를 고른다.
 *
 * 우선순위: 정확한 id → **유일한** prefix → (ref 없으면) 최신 1건.
 * prefix 가 여러 잡에 걸리면 조용히 하나를 고르지 않고 에러를 던진다 — 손으로 짧게 친 id 가
 * 엉뚱한 잡을 취소하는 사고를 막기 위해서다.
 */
export function matchJobReference(jobs: JobRecord[], ref?: string): JobRecord {
  if (jobs.length === 0) {
    throw new Error('opencode 잡이 없습니다.');
  }
  if (!ref || ref.trim().length === 0) {
    return jobs[0]!;
  }
  const needle = ref.trim();
  const exact = jobs.find((job) => job.id === needle);
  if (exact) {
    return exact;
  }
  const prefixed = jobs.filter((job) => job.id.startsWith(needle));
  if (prefixed.length === 1) {
    return prefixed[0]!;
  }
  if (prefixed.length > 1) {
    throw new Error(
      `잡 참조 "${needle}" 가 모호합니다 (ambiguous). 후보: ${prefixed.map((j) => j.id).join(', ')}`,
    );
  }
  throw new Error(`잡 참조 "${needle}" 에 해당하는 opencode 잡이 없습니다.`);
}

/**
 * Claude 세션 id 로 잡을 거른다.
 * `sessionId` 가 없으면(= 세션 주입이 안 된 환경) 전부 통과시킨다. 반대로 세션 id 가 있는데
 * 매칭이 0건이면 **폴백하지 않는다** — 다른 세션의 잡을 보여주느니 비어 있는 편이 낫다.
 */
export function filterBySession(jobs: JobRecord[], sessionId?: string): JobRecord[] {
  if (!sessionId) {
    return jobs;
  }
  return jobs.filter((job) => job.sessionId === sessionId);
}

/** 잡 저장소. 한 인스턴스는 한 프로젝트(디렉터리)를 담당한다. */
export class JobStore {
  readonly dir: string;
  readonly maxJobs: number;

  constructor(options: { dir: string; maxJobs?: number }) {
    this.dir = options.dir;
    this.maxJobs = options.maxJobs ?? MAX_JOBS;
  }

  /** `jobs/` 하위 디렉터리 경로. */
  private get jobsDir(): string {
    return join(this.dir, 'jobs');
  }

  /** 인덱스 파일 경로. */
  private get statePath(): string {
    return join(this.dir, STATE_FILE);
  }

  /** payload 파일 경로. */
  payloadPath(id: string): string {
    return join(this.jobsDir, `${id}.json`);
  }

  /** 진행 로그 파일 경로. */
  logPath(id: string): string {
    return join(this.jobsDir, `${id}.log`);
  }

  private ensureDirs(): void {
    mkdirSync(this.jobsDir, { recursive: true });
  }

  private readState(): StateFile {
    try {
      const parsed = JSON.parse(readFileSync(this.statePath, 'utf8')) as StateFile;
      if (!Array.isArray(parsed?.jobs)) {
        return { version: STATE_VERSION, nextSeq: 1, jobs: [] };
      }
      const maxSeq = parsed.jobs.reduce((max, entry) => Math.max(max, entry.seq ?? 0), 0);
      return {
        version: parsed.version ?? STATE_VERSION,
        nextSeq: Math.max(parsed.nextSeq ?? 1, maxSeq + 1),
        jobs: parsed.jobs,
      };
    } catch {
      // 파일 없음 / 깨진 JSON 모두 "빈 상태" 로 취급한다 — 상태 파일 하나 때문에 위임이 막히면 안 된다.
      return { version: STATE_VERSION, nextSeq: 1, jobs: [] };
    }
  }

  /** 인덱스를 쓰고, 넘치는 잡의 payload / log 를 같이 지운다. */
  private writeState(state: StateFile): void {
    this.ensureDirs();
    const sorted = [...state.jobs].sort(byNewest);
    const kept = sorted.slice(0, this.maxJobs);
    for (const dropped of sorted.slice(this.maxJobs)) {
      this.deleteFiles(dropped.id);
    }
    writeAtomic(
      this.statePath,
      JSON.stringify({ version: STATE_VERSION, nextSeq: state.nextSeq, jobs: kept }, null, 2),
    );
  }

  private deleteFiles(id: string): void {
    for (const path of [this.payloadPath(id), this.logPath(id)]) {
      try {
        if (existsSync(path)) {
          unlinkSync(path);
        }
      } catch {
        // best-effort GC — 지우지 못해도 진행을 막지 않는다.
      }
    }
  }

  private toIndexEntry(job: JobRecord): JobIndexEntry {
    return {
      id: job.id,
      title: job.title,
      status: job.status,
      phase: job.phase,
      sessionId: job.sessionId,
      workspaceRoot: job.workspaceRoot,
      createdAt: job.createdAt,
      updatedAt: job.updatedAt,
      seq: job.seq,
    };
  }

  /** payload 가 사라진 잡을 인덱스 정보만으로 복원한다 (목록이 통째로 죽지 않게). */
  private fromIndexEntry(entry: JobIndexEntry): JobRecord {
    return {
      id: entry.id,
      kind: 'task',
      title: entry.title,
      status: entry.status,
      phase: entry.phase,
      sessionId: entry.sessionId,
      workspaceRoot: entry.workspaceRoot,
      request: { prompt: '', worktree: entry.workspaceRoot },
      logFile: this.logPath(entry.id),
      seq: entry.seq,
      createdAt: entry.createdAt,
      updatedAt: entry.updatedAt,
      errorMessage: 'payload 파일이 없습니다 (prune 되었거나 삭제됨).',
    };
  }

  private writePayload(job: JobRecord): void {
    this.ensureDirs();
    writeAtomic(this.payloadPath(job.id), JSON.stringify(job, null, 2));
  }

  /** 새 잡을 `queued` 로 만든다. */
  create(input: CreateJobInput): JobRecord {
    const now = new Date().toISOString();
    const id = newJobId();
    const state = this.readState();
    const seq = state.nextSeq;
    state.nextSeq = seq + 1;
    const job: JobRecord = {
      id,
      kind: 'task',
      title: input.title,
      status: 'queued',
      phase: input.phase ?? 'queued',
      sessionId: input.sessionId,
      workspaceRoot: input.workspaceRoot,
      request: input.request,
      logFile: this.logPath(id),
      seq,
      createdAt: now,
      updatedAt: now,
    };
    this.writePayload(job);
    state.jobs.push(this.toIndexEntry(job));
    this.writeState(state);
    return job;
  }

  /** payload 를 읽는다. 없으면 null. */
  get(id: string): JobRecord | null {
    try {
      return JSON.parse(readFileSync(this.payloadPath(id), 'utf8')) as JobRecord;
    } catch {
      return null;
    }
  }

  /** 최신순 전체 목록. */
  list(): JobRecord[] {
    const state = this.readState();
    return [...state.jobs]
      .sort(byNewest)
      .map((entry) => this.get(entry.id) ?? this.fromIndexEntry(entry));
  }

  /** 필드를 패치하고 `updatedAt` 을 갱신한다. 인덱스에도 상태를 반영한다. */
  update(id: string, patch: Partial<Omit<JobRecord, 'id' | 'kind' | 'createdAt'>>): JobRecord {
    const current = this.get(id);
    if (!current) {
      throw new Error(`opencode 잡 "${id}" 를 찾을 수 없습니다.`);
    }
    const next: JobRecord = { ...current, ...patch, updatedAt: new Date().toISOString() };
    this.writePayload(next);
    const state = this.readState();
    const idx = state.jobs.findIndex((entry) => entry.id === id);
    if (idx >= 0) {
      state.jobs[idx] = this.toIndexEntry(next);
    } else {
      state.jobs.push(this.toIndexEntry(next));
    }
    this.writeState(state);
    return next;
  }

  /** 진행 로그 한 줄 append. 실패해도 던지지 않는다 (로그 때문에 잡이 죽으면 안 된다). */
  appendLog(id: string, message: string): void {
    try {
      this.ensureDirs();
      appendFileSync(this.logPath(id), `[${new Date().toISOString()}] ${message}\n`, 'utf8');
    } catch {
      // 무시
    }
  }

  /** 진행 로그의 마지막 n 줄. */
  readLogTail(id: string, lines = 4): string[] {
    try {
      const raw = readFileSync(this.logPath(id), 'utf8');
      const all = raw.split('\n').filter((line) => line.trim().length > 0);
      return all.slice(-lines);
    } catch {
      return [];
    }
  }

  /** 잡 하나를 인덱스와 파일에서 모두 제거한다. */
  remove(id: string): void {
    const state = this.readState();
    state.jobs = state.jobs.filter((entry) => entry.id !== id);
    this.deleteFiles(id);
    this.writeState(state);
  }
}

/** 임시 파일 → rename 으로 원자적 쓰기. 중간에 죽어도 반쯤 쓰인 JSON 이 남지 않는다. */
function writeAtomic(path: string, content: string): void {
  const tmp = `${path}.${process.pid}.tmp`;
  writeFileSync(tmp, content, 'utf8');
  renameSync(tmp, path);
}

/**
 * env(우선) → config → 프로젝트별 기본값 순으로 잡 저장소를 만든다.
 * `ROCKY_OPENCODE_JOBS_DIR` 는 verbatim 사용, `config.dir` 은 `~` 를 확장한다.
 */
export function createJobStoreFromEnv(
  config?: OpencodeJobsConfig,
  env: NodeJS.ProcessEnv = process.env,
  cwd: string = process.cwd(),
): JobStore {
  const fromEnv = env.ROCKY_OPENCODE_JOBS_DIR?.trim();
  const dir = fromEnv
    ? fromEnv
    : config?.dir
      ? expandTilde(config.dir)
      : resolveDefaultJobsDir(cwd);
  return new JobStore({ dir, maxJobs: config?.maxJobs });
}
