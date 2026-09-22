import { existsSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join, resolve } from 'node:path';

/**
 * `rocky.json` 의 단일 config 로더 + 검증.
 *
 * 두 위치를 병합한다 (project 가 필드 단위로 user 를 덮어쓴다):
 *   1. user:    ~/.config/rocky/rocky.json
 *   2. project: <projectRoot>/rocky.json
 *
 * 스키마는 repo 루트의 `rocky.schema.json` 에 동일 모양으로 박혀 있다 — IDE 자동완성용.
 * 런타임은 외부 JSON Schema 라이브러리에 의존하지 않고 직접 검증한다 (의존성 0 유지).
 *
 * `openapi` / `seo` 블록은 v0.23 에서 도구와 함께 제거됐다 — 이제 알 수 없는 키로 거부된다.
 * `todo` 블록은 Rust 데몬(`crates/`)이 읽는다; 이 로더는 파싱만 통과시킨다.
 */

/** `worklog_*` 도구 + Stop hook 자동 기록 + `/recall` 다이제스트 설정. */
export interface WorklogConfig {
  /** 저널 JSONL 저장 디렉터리. 미지정 시 프로젝트별 기본 경로(`~/.config/rocky/worklog/<key>`). */
  dir?: string;
  /** Stop hook 자동 워크로그 기록 on/off. 기본 true. env `ROCKY_WORKLOG_AUTO_CAPTURE` 우선. */
  autoCapture?: boolean;
  /** turn 엔트리 req/did 최대 글자 수. 기본 800. */
  captureMaxChars?: number;
  /** `/recall` Haiku↔Sonnet 임계(신규 엔트리 수). 기본 40. */
  digestThreshold?: number;
}

export interface RockyConfig {
  $schema?: string;
  worklog?: WorklogConfig;
}

export interface LoadConfigOptions {
  /** user config 경로 override. 기본 `USER_CONFIG_PATH`. */
  userPath?: string;
  /** project root. 기본 `process.cwd()`. */
  projectRoot?: string;
}

export interface LoadConfigError {
  /** 실패한 파일의 절대 경로. */
  source: string;
  /** 파싱 또는 검증 실패 메시지 (Error.message 또는 stringified). */
  message: string;
}

export interface LoadConfigResult {
  /** user + project 를 필드 단위로 merge 한 결과. 둘 다 실패 / 둘 다 부재면 빈 객체. */
  config: RockyConfig;
  /** 파싱 / 검증에 실패한 파일별 에러. caller 가 logging / surfacing 결정. */
  errors: LoadConfigError[];
}

/** user-level config 기본 경로. `ROCKY_CONFIG` 로 오버라이드. */
export const USER_CONFIG_PATH = join(homedir(), '.config', 'rocky', 'rocky.json');

/** project-level config 의 상대 경로. */
export const PROJECT_CONFIG_RELATIVE = 'rocky.json';

/**
 * top-level 에서 허용하는 키 (오타 / 제거된 도메인 키 가드, 스키마 lockstep).
 * `todo` 는 이 로더가 소비하지 않지만 통과시킨다 — 같은 파일을 Rust 데몬이 읽으므로,
 * 여기서 모양을 검증하면 두 로더가 어긋날 때 파일 전체가 거부된다.
 */
const ALLOWED_TOP_KEYS = new Set(['$schema', 'worklog', 'todo']);

/**
 * 파싱된 JSON 값이 RockyConfig 인지 검증한다. 어긋나면 throw — 메시지에 source(path) 포함.
 * 부분 적합도 OK (모든 필드 optional).
 */
export function validateConfig(input: unknown, source: string): RockyConfig {
  if (input === null || typeof input !== 'object' || Array.isArray(input)) {
    throw new Error(`${source}: config must be a JSON object`);
  }
  const config = input as Record<string, unknown>;
  for (const key of Object.keys(config)) {
    if (!ALLOWED_TOP_KEYS.has(key)) {
      throw new Error(`${source}: unknown top-level key "${key}"`);
    }
  }
  if (config.worklog !== undefined) {
    validateWorklog(config.worklog, source);
  }
  return config as RockyConfig;
}

/** `worklog` 객체에서 허용하는 키 (오타 가드, 스키마 lockstep). */
const ALLOWED_WORKLOG_KEYS = new Set(['dir', 'autoCapture', 'captureMaxChars', 'digestThreshold']);

/**
 * `worklog` 객체 모양 검증. `worklog_*` 도구 + Stop hook 자동 기록 + `/recall` 다이제스트
 * 설정을 받는다 — 미지원 key 는 reject. 기본값 적용은 소비 지점 몫이라 여기서는 존재하는
 * 필드의 타입 / 범위만 검증한다.
 */
function validateWorklog(worklog: unknown, source: string): void {
  if (worklog === null || typeof worklog !== 'object' || Array.isArray(worklog)) {
    throw new Error(`${source}: worklog must be an object`);
  }
  const obj = worklog as Record<string, unknown>;
  for (const key of Object.keys(obj)) {
    if (!ALLOWED_WORKLOG_KEYS.has(key)) {
      throw new Error(`${source}: worklog: unknown key "${key}"`);
    }
  }
  if (obj.dir !== undefined && (typeof obj.dir !== 'string' || obj.dir.trim().length === 0)) {
    throw new Error(`${source}: worklog.dir must be a non-empty string`);
  }
  if (obj.autoCapture !== undefined && typeof obj.autoCapture !== 'boolean') {
    throw new Error(`${source}: worklog.autoCapture must be a boolean`);
  }
  for (const key of ['captureMaxChars', 'digestThreshold'] as const) {
    const v = obj[key];
    if (v !== undefined && (typeof v !== 'number' || !Number.isInteger(v) || v < 1)) {
      throw new Error(`${source}: worklog.${key} must be a positive integer`);
    }
  }
}

async function loadOne(path: string): Promise<RockyConfig | null> {
  if (!existsSync(path)) {
    return null;
  }
  const raw = await readFile(path, 'utf8');
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch (err) {
    throw new Error(`Failed to parse ${path} as JSON: ${(err as Error).message}`);
  }
  return validateConfig(parsed, path);
}

/**
 * user → project 순서로 병합. `worklog` 는 필드 단위로 project 가 user 를 덮어쓴다.
 */
export function mergeConfigs(user: RockyConfig, project: RockyConfig): RockyConfig {
  // Bun ≥ 1.0 / Node ≥ 17 모두 structuredClone 표준 지원. JSON round-trip 보다 성능과
  // 의도가 명확 — 입력은 plain JSON 모양이라 Date / Map / Set 호환은 신경 쓰지 않아도 된다.
  const out = structuredClone(user) as RockyConfig;
  if (project.worklog) {
    out.worklog = { ...out.worklog, ...project.worklog };
  }
  return out;
}

/**
 * user + project config 를 읽어 merge 된 결과 + 파일별 에러를 반환.
 *
 * 한 쪽 파일이 손상되어도 다른 쪽은 그대로 살린다 — 즉 잘못된 user 파일이 정상 project
 * 설정을 무력화하지 않는다 (반대도 마찬가지). caller 는 `errors` 를 보고 logging /
 * surfacing 을 결정한다.
 *
 * 두 파일 모두 없으면 `{ config: {}, errors: [] }`. `ROCKY_CONFIG` 환경변수가
 * 있으면 user 경로를 그 값으로 덮어쓴다.
 */
export async function loadConfig(options: LoadConfigOptions = {}): Promise<LoadConfigResult> {
  const userPath = options.userPath ?? process.env.ROCKY_CONFIG ?? USER_CONFIG_PATH;
  const projectRoot = options.projectRoot ?? process.cwd();
  const projectPath = resolve(projectRoot, PROJECT_CONFIG_RELATIVE);
  const errors: LoadConfigError[] = [];

  let user: RockyConfig = {};
  try {
    user = (await loadOne(userPath)) ?? {};
  } catch (err) {
    errors.push({
      source: userPath,
      message: err instanceof Error ? err.message : String(err),
    });
  }

  let project: RockyConfig = {};
  try {
    project = (await loadOne(projectPath)) ?? {};
  } catch (err) {
    errors.push({
      source: projectPath,
      message: err instanceof Error ? err.message : String(err),
    });
  }

  return { config: mergeConfigs(user, project), errors };
}
