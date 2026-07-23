import { existsSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join, resolve } from 'node:path';

/**
 * openapi-core 의 단일 config 로더 + 검증.
 *
 * 두 위치를 병합한다 (project 가 leaf 단위로 user 를 덮어쓴다):
 *   1. user:    ~/.config/rocky/rocky.json
 *   2. project: <projectRoot>/rocky.json
 *
 * 스키마는 repo 루트의 `rocky.schema.json` 에 동일 모양으로 박혀 있다 — IDE 자동완성용.
 * 런타임은 외부 JSON Schema 라이브러리에 의존하지 않고 직접 검증한다 (의존성 0 유지).
 *
 * v0.3 부터는 OpenAPI 도메인만 다룬다. 이전 버전의 `mysql` / `spec` / `github` 필드는
 * `archive/pre-openapi-only-slim` 브랜치에 박제되어 있으며, 도메인이 재추가될 때 별도 PR 로
 * 다시 합류한다.
 */

/**
 * spec 등록 단위. host → env → spec → leaf 평면 트리.
 *
 * leaf 형태:
 *   - **string** (legacy / 기본): spec URL 만. baseUrl 미선언 → `openapi_endpoint`
 *     의 fullUrl 합성 시 path 자체로만 떨어진다.
 *   - **object**: `{ url, baseUrl?, format? }` — 환경별 baseUrl 을 선언해 fullUrl
 *     합성 가능. format 으로 `openapi3` / `swagger2` / `auto` 강제 가능 (기본 auto).
 */
export interface OpenapiRegistryLeafObject {
  /** 다운로드할 OpenAPI / Swagger 본문 URL (http / https / file). */
  url: string;
  /** 환경의 실제 API base URL — `openapi_endpoint` 의 fullUrl 합성에 사용. */
  baseUrl?: string;
  /** 형식 hint. 미지정이면 본문의 `openapi` / `swagger` 필드로 자동 감지. */
  format?: 'openapi3' | 'swagger2' | 'auto';
}

export type OpenapiRegistryLeaf = string | OpenapiRegistryLeafObject;

export interface OpenapiRegistry {
  [host: string]: {
    [env: string]: {
      [spec: string]: OpenapiRegistryLeaf;
    };
  };
}

/** leaf 가 string 이든 object 이든 spec URL 을 꺼낸다. */
export function getRegistryUrl(leaf: OpenapiRegistryLeaf): string {
  return typeof leaf === 'string' ? leaf : leaf.url;
}

/** leaf 의 baseUrl. string leaf 또는 baseUrl 미선언 object 면 undefined. */
export function getRegistryBaseUrl(leaf: OpenapiRegistryLeaf): string | undefined {
  if (typeof leaf === 'string') {
    return undefined;
  }
  return leaf.baseUrl;
}

/** leaf 의 format hint. string leaf 또는 format 미선언 object 면 undefined. */
export function getRegistryFormat(
  leaf: OpenapiRegistryLeaf,
): 'openapi3' | 'swagger2' | 'auto' | undefined {
  if (typeof leaf === 'string') {
    return undefined;
  }
  return leaf.format;
}

/**
 * `seo_validate` 도구 기본값. 모든 필드는 도구 호출 인자로 override 된다.
 */
export interface SeoConfig {
  /**
   * true 면 private / loopback / link-local 호스트 fetch 를 허용. 기본 false.
   * `seo_validate` 도구 호출 인자 (`allowPrivateHosts`) 가 우선.
   */
  allowPrivateHosts?: boolean;
  /** fetch timeout (ms). 1..30000. 도구 호출 인자 (`timeoutMs`) 가 우선. */
  timeoutMs?: number;
}

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

/** rocky-todo 데몬 (공유 todo/스크래치패드 보드) 설정. */
export interface TodoConfig {
  /**
   * 마스터 스위치. 기본 **false** — 상주 데몬을 띄우는 기능이라 opt-in 이다.
   * 꺼져 있으면 UserPromptSubmit 훅·CLI 자동 기동·데몬 기동이 모두 비활성.
   * env `ROCKY_TODO_ENABLED` 우선.
   */
  enabled?: boolean;
  /** 데몬 포트. 기본 8636 (키패드 "todo"). env `ROCKY_TODO_PORT` 우선. */
  port?: number;
  /** 데이터 디렉터리 (todo.db). 기본 `~/.config/rocky/todo`. env `ROCKY_TODO_DIR` 우선. */
  dir?: string;
  /**
   * 보드 노출 채널 배열. 빈 배열/생략(기본) = 이 머신만(127.0.0.1). `"lan"` = 내부망
   * 개방(0.0.0.0), `"tailscale-serve"` = 테일넷 한정 프록시(tailscale serve, 바인딩은
   * 루프백 유지 — 테일넷 로그인이 사실상의 최소 안전장치, 자체 인증은 아님) —
   * 둘 다 넣으면 동시 개방. 보드에 인증이 없으므로 lan 은 신뢰하는 네트워크에서만.
   * tailscale-serve 채널이 없으면 tailscale 을 일절 건드리지 않는다 (회사 등 금지 환경 대비).
   * env `ROCKY_TODO_EXPOSE`(콤마 구분, 설정 시 통째로 우선 — `off` 로 강제 차단) 우선.
   * 채널 하나만 켤 땐 배열 대신 문자열도 허용 (`"expose": "lan"`). 문자열 전용 값
   * `"off"` 와 null 은 미설정(undefined)과 동일 — 채널 없음. 배열 안에는 못 넣는다.
   */
  expose?: ('lan' | 'tailscale-serve')[] | 'lan' | 'tailscale-serve' | 'off' | null;
  /** UserPromptSubmit 훅의 보드 변경 주입 on/off. 기본 true. env `ROCKY_TODO_WATCH` 우선. */
  watch?: boolean;
}

export interface RockyConfig {
  $schema?: string;
  /** 활성 소울(페르소나) 이름. SessionStart 훅이 이 이름으로 소울 파일을 찾아 주입한다. */
  soul?: string;
  /**
   * 소울이 사용자를 부르는 호칭. SessionStart 훅이 소울 컨텍스트에 함께 주입한다.
   * 소울 본문의 기본 호칭 규칙(예: rocky 의 "친구")보다 우선. 미설정 시 주입 없음.
   */
  callsign?: string;
  openapi?: {
    registry?: OpenapiRegistry;
  };
  seo?: SeoConfig;
  worklog?: WorklogConfig;
  todo?: TodoConfig;
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
  /** user + project 를 leaf 단위로 merge 한 결과. 둘 다 실패 / 둘 다 부재면 빈 객체. */
  config: RockyConfig;
  /** 파싱 / 검증에 실패한 파일별 에러. caller 가 logging / surfacing 결정. */
  errors: LoadConfigError[];
}

/** user-level config 기본 경로. `ROCKY_CONFIG` 로 오버라이드. */
export const USER_CONFIG_PATH = join(homedir(), '.config', 'rocky', 'rocky.json');

/** project-level config 의 상대 경로. */
export const PROJECT_CONFIG_RELATIVE = 'rocky.json';

/**
 * host / env / spec 식별자 본문 (anchor 없음).
 * 다른 모듈 (`openapi-registry.ts`) 이 핸들 / 스코프 정규식을 합성할 때 재사용한다 —
 * 이 한 군데만 바꾸면 schema / registry 둘 다 같이 따라간다.
 */
export const ID_BODY = '[a-zA-Z0-9_-]+';

/** host / env / spec 식별자 정규식 (앵커 포함). 콜론은 handle separator 로 예약. */
export const ID_PATTERN = new RegExp(`^${ID_BODY}$`);

/** 레지스트리 leaf URL 에 허용되는 스킴. spec 다운로드 단이 받는 종류와 동일. */
const URL_SCHEMES = new Set(['http:', 'https:', 'file:']);

/** registry leaf object 에서 허용하는 키 (오타 가드, 스키마 lockstep). */
const ALLOWED_REGISTRY_LEAF_KEYS = new Set(['url', 'baseUrl', 'format']);

/** registry leaf object 의 format 필드에 허용되는 값. */
const ALLOWED_REGISTRY_LEAF_FORMATS = new Set(['openapi3', 'swagger2', 'auto']);

/**
 * 파싱된 JSON 값이 RockyConfig 인지 검증한다. 어긋나면 throw — 메시지에 source(path) 포함.
 * 부분 적합도 OK (모든 필드 optional). registry 가 있으면 깊이 끝까지 식별자 / URL 검증.
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
  if (config.openapi !== undefined) {
    if (
      config.openapi === null ||
      typeof config.openapi !== 'object' ||
      Array.isArray(config.openapi)
    ) {
      throw new Error(`${source}: openapi must be an object`);
    }
    const oapi = config.openapi as Record<string, unknown>;
    if (oapi.registry !== undefined) {
      validateRegistry(oapi.registry, source);
    }
  }
  if (config.seo !== undefined) {
    validateSeo(config.seo, source);
  }
  if (config.worklog !== undefined) {
    validateWorklog(config.worklog, source);
  }
  if (config.todo !== undefined) {
    validateTodo(config.todo, source);
  }
  if (config.soul !== undefined) {
    validateSoul(config.soul, source);
  }
  if (config.callsign !== undefined) {
    validateCallsign(config.callsign, source);
  }
  return config as RockyConfig;
}

/** top-level 에서 허용하는 키 (오타 / 제거된 도메인 키 가드, 스키마 lockstep). */
const ALLOWED_TOP_KEYS = new Set([
  '$schema',
  'soul',
  'callsign',
  'openapi',
  'seo',
  'worklog',
  'todo',
]);

/** `seo` 객체에서 허용하는 키 (오타 가드, 스키마 lockstep). */
const ALLOWED_SEO_KEYS = new Set(['allowPrivateHosts', 'timeoutMs']);

/**
 * `seo` 객체 모양 검증. `seo_validate` 도구 기본값을 받는다 — 미지원 key 는 reject.
 */
function validateSeo(seo: unknown, source: string): void {
  if (seo === null || typeof seo !== 'object' || Array.isArray(seo)) {
    throw new Error(`${source}: seo must be an object`);
  }
  const obj = seo as Record<string, unknown>;
  for (const key of Object.keys(obj)) {
    if (!ALLOWED_SEO_KEYS.has(key)) {
      throw new Error(`${source}: seo: unknown key "${key}"`);
    }
  }
  if (obj.allowPrivateHosts !== undefined && typeof obj.allowPrivateHosts !== 'boolean') {
    throw new Error(`${source}: seo.allowPrivateHosts must be a boolean`);
  }
  if (obj.timeoutMs !== undefined) {
    if (
      typeof obj.timeoutMs !== 'number' ||
      !Number.isInteger(obj.timeoutMs) ||
      obj.timeoutMs < 1 ||
      obj.timeoutMs > 30_000
    ) {
      throw new Error(`${source}: seo.timeoutMs must be an integer between 1 and 30000`);
    }
  }
}

/** `worklog` 객체에서 허용하는 키 (오타 가드, 스키마 lockstep). */
const ALLOWED_WORKLOG_KEYS = new Set(['dir', 'autoCapture', 'captureMaxChars', 'digestThreshold']);

/**
 * `worklog` 객체 모양 검증. `worklog_*` 도구 + Stop hook 자동 기록 + `/recall` 다이제스트
 * 설정을 받는다 — 미지원 key 는 reject. 기본값 적용은 소비 지점(다른 태스크) 몫이라
 * 여기서는 존재하는 필드의 타입 / 범위만 검증한다.
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

/** `todo` 객체에서 허용하는 키 (오타 가드, 스키마 lockstep). */
const ALLOWED_TODO_KEYS = new Set(['enabled', 'port', 'dir', 'expose', 'watch']);

/** `todo.expose` 배열이 받는 채널 — src/todo/config.ts 의 EXPOSE_CHANNELS 와 lockstep. */
const TODO_EXPOSE_CHANNELS = new Set(['lan', 'tailscale-serve']);

/**
 * `todo` 객체 모양 검증. rocky-todo 데몬 설정을 받는다 — 미지원 key 는 reject.
 * 기본값 적용(포트 8636 / `~/.config/rocky/todo`)은 소비 지점(데몬/CLI) 몫.
 */
function validateTodo(todo: unknown, source: string): void {
  if (todo === null || typeof todo !== 'object' || Array.isArray(todo)) {
    throw new Error(`${source}: todo must be an object`);
  }
  const obj = todo as Record<string, unknown>;
  for (const key of Object.keys(obj)) {
    if (!ALLOWED_TODO_KEYS.has(key)) {
      throw new Error(`${source}: todo: unknown key "${key}"`);
    }
  }
  if (obj.port !== undefined) {
    if (
      typeof obj.port !== 'number' ||
      !Number.isInteger(obj.port) ||
      obj.port < 1 ||
      obj.port > 65_535
    ) {
      throw new Error(`${source}: todo.port must be an integer between 1 and 65535`);
    }
  }
  if (obj.dir !== undefined && (typeof obj.dir !== 'string' || obj.dir.trim().length === 0)) {
    throw new Error(`${source}: todo.dir must be a non-empty string`);
  }
  // "off"(문자열 전용) / null 은 미설정과 동일 취급 — 배열 안의 "off" 는 거부된다.
  if (obj.expose !== undefined && obj.expose !== null && obj.expose !== 'off') {
    if (!Array.isArray(obj.expose) && typeof obj.expose !== 'string') {
      throw new Error(`${source}: todo.expose must be a channel string or an array of channels`);
    }
    const channels = Array.isArray(obj.expose) ? obj.expose : [obj.expose];
    for (const channel of channels) {
      if (typeof channel !== 'string' || !TODO_EXPOSE_CHANNELS.has(channel)) {
        throw new Error(`${source}: todo.expose entries must be "lan" or "tailscale-serve"`);
      }
    }
  }
  for (const key of ['enabled', 'watch'] as const) {
    if (obj[key] !== undefined && typeof obj[key] !== 'boolean') {
      throw new Error(`${source}: todo.${key} must be a boolean`);
    }
  }
}

/**
 * `soul` 필드 검증. 활성 소울 이름 — 파일명으로 쓰이므로 `ID_PATTERN`
 * (`[a-zA-Z0-9_-]+`) 만 허용한다 (경로 이스케이프 / 콜론 방지).
 */
function validateSoul(soul: unknown, source: string): void {
  if (typeof soul !== 'string') {
    throw new Error(`${source}: soul must be a string`);
  }
  if (!ID_PATTERN.test(soul)) {
    throw new Error(
      `${source}: soul must match ${ID_PATTERN} (alphanumeric, "_" or "-" only) — got "${soul}"`,
    );
  }
}

/** 호칭 최대 길이 — 컨텍스트에 한 줄로 주입되므로 짧게 제한한다 (스키마 lockstep). */
const CALLSIGN_MAX_LENGTH = 40;

/**
 * `callsign` 필드 검증. 사용자를 부르는 호칭 — 컨텍스트에 한 줄로 주입되므로
 * 줄바꿈(유니코드 line separator 포함) 없는 문자열, 공백-only 불가, 원본 기준 최대
 * 40자만 허용한다. 한글 / 공백 OK — `soul` 과 달리 파일명으로 쓰이지 않아
 * `ID_PATTERN` 제약이 없다. 기준은 `rocky.schema.json` 의 `callsign` 과 lockstep —
 * 길이는 둘 다 원본(raw) 기준이라 에디터 검증과 런타임이 어긋나지 않는다.
 */
function validateCallsign(callsign: unknown, source: string): void {
  if (typeof callsign !== 'string') {
    throw new Error(`${source}: callsign must be a string`);
  }
  if (/[\r\n\u2028\u2029]/.test(callsign)) {
    throw new Error(`${source}: callsign must be a single line (no line breaks)`);
  }
  if (callsign.trim().length === 0) {
    throw new Error(`${source}: callsign must be a non-empty string`);
  }
  if (callsign.length > CALLSIGN_MAX_LENGTH) {
    throw new Error(
      `${source}: callsign must be at most ${CALLSIGN_MAX_LENGTH} characters — got ${callsign.length}`,
    );
  }
}

function validateRegistry(reg: unknown, source: string): asserts reg is OpenapiRegistry {
  if (reg === null || typeof reg !== 'object' || Array.isArray(reg)) {
    throw new Error(`${source}: openapi.registry must be an object`);
  }
  for (const [host, envs] of Object.entries(reg as Record<string, unknown>)) {
    if (!ID_PATTERN.test(host)) {
      throw new Error(
        `${source}: host name "${host}" must match ${ID_PATTERN} (alphanumeric, "_" or "-" only — colons are reserved for handle separators)`,
      );
    }
    if (envs === null || typeof envs !== 'object' || Array.isArray(envs)) {
      throw new Error(`${source}: openapi.registry["${host}"] must be an object of environments`);
    }
    for (const [env, specs] of Object.entries(envs as Record<string, unknown>)) {
      if (!ID_PATTERN.test(env)) {
        throw new Error(`${source}: env name "${host}:${env}" must match ${ID_PATTERN}`);
      }
      if (specs === null || typeof specs !== 'object' || Array.isArray(specs)) {
        throw new Error(
          `${source}: openapi.registry["${host}"]["${env}"] must be an object of specs`,
        );
      }
      for (const [spec, leaf] of Object.entries(specs as Record<string, unknown>)) {
        if (!ID_PATTERN.test(spec)) {
          throw new Error(`${source}: spec name "${host}:${env}:${spec}" must match ${ID_PATTERN}`);
        }
        validateRegistryLeaf(leaf, `${source}: openapi.registry["${host}"]["${env}"]["${spec}"]`);
      }
    }
  }
}

/**
 * registry leaf 검증. string (URL only) 또는 object (`{ url, baseUrl?, format? }`)
 * 모두 받는다. URL 은 둘 다 같은 스킴 / non-empty 검증을 통과해야 한다.
 */
function validateRegistryLeaf(leaf: unknown, where: string): void {
  if (typeof leaf === 'string') {
    if (leaf.trim().length === 0) {
      throw new Error(`${where} must be a non-empty URL string`);
    }
    validateRegistryUrlString(leaf, where);
    return;
  }
  if (leaf === null || typeof leaf !== 'object' || Array.isArray(leaf)) {
    throw new Error(`${where} must be a non-empty URL string or object { url, baseUrl?, format? }`);
  }
  const obj = leaf as Record<string, unknown>;
  for (const key of Object.keys(obj)) {
    if (!ALLOWED_REGISTRY_LEAF_KEYS.has(key)) {
      throw new Error(
        `${where} has unsupported key "${key}" — allowed: ${[...ALLOWED_REGISTRY_LEAF_KEYS].join(', ')}`,
      );
    }
  }
  if (typeof obj.url !== 'string' || obj.url.trim().length === 0) {
    throw new Error(`${where}.url must be a non-empty URL string`);
  }
  validateRegistryUrlString(obj.url, `${where}.url`);
  if (obj.baseUrl !== undefined) {
    if (typeof obj.baseUrl !== 'string' || obj.baseUrl.trim().length === 0) {
      throw new Error(`${where}.baseUrl must be a non-empty string`);
    }
    let parsed: URL;
    try {
      parsed = new URL(obj.baseUrl);
    } catch {
      throw new Error(`${where}.baseUrl is not a valid URL — got "${obj.baseUrl}"`);
    }
    if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
      throw new Error(
        `${where}.baseUrl uses unsupported scheme "${parsed.protocol}" — only http / https are accepted`,
      );
    }
  }
  if (obj.format !== undefined) {
    if (typeof obj.format !== 'string' || !ALLOWED_REGISTRY_LEAF_FORMATS.has(obj.format)) {
      throw new Error(
        `${where}.format must be one of ${[...ALLOWED_REGISTRY_LEAF_FORMATS].join(' / ')}`,
      );
    }
  }
}

function validateRegistryUrlString(url: string, where: string): void {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    throw new Error(`${where} is not a valid URL — got "${url}"`);
  }
  if (!URL_SCHEMES.has(parsed.protocol)) {
    throw new Error(
      `${where} uses unsupported scheme "${parsed.protocol}" — only http / https / file are accepted`,
    );
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
 * user → project 순서로 깊이 병합. 같은 leaf (host:env:spec) 는 project 가 이긴다.
 * 새 host / env / spec 은 project 쪽에서 추가 가능.
 */
export function mergeConfigs(user: RockyConfig, project: RockyConfig): RockyConfig {
  // Bun ≥ 1.0 / Node ≥ 17 모두 structuredClone 표준 지원. JSON round-trip 보다 성능과
  // 의도가 명확 — 입력은 plain JSON 모양이라 Date / Map / Set 호환은 신경 쓰지 않아도 된다.
  const out = structuredClone(user) as RockyConfig;
  if (project.openapi?.registry) {
    out.openapi ??= {};
    out.openapi.registry ??= {};
    for (const [host, envs] of Object.entries(project.openapi.registry)) {
      out.openapi.registry[host] ??= {};
      for (const [env, specs] of Object.entries(envs)) {
        out.openapi.registry[host]![env] ??= {};
        for (const [spec, leaf] of Object.entries(specs)) {
          // leaf 는 string 또는 object — 둘 다 통째로 덮어쓴다 (project 가 이긴다).
          out.openapi.registry[host]![env]![spec] = leaf;
        }
      }
    }
  }
  // seo 는 leaf 별 병합이 아니라 필드 단위로 project 가 user 를 덮어쓴다.
  if (project.seo) {
    out.seo = { ...out.seo, ...project.seo };
  }
  // worklog 도 seo 와 동일 — 필드 단위로 project 가 user 를 덮어쓴다.
  if (project.worklog) {
    out.worklog = { ...out.worklog, ...project.worklog };
  }
  // todo 도 동일 — 필드 단위로 project 가 user 를 덮어쓴다.
  if (project.todo) {
    out.todo = { ...out.todo, ...project.todo };
  }
  // soul / callsign 은 스칼라 — project 가 있으면 user 를 덮어쓴다.
  if (project.soul !== undefined) {
    out.soul = project.soul;
  }
  if (project.callsign !== undefined) {
    out.callsign = project.callsign;
  }
  return out;
}

/**
 * user + project config 를 읽어 merge 된 결과 + 파일별 에러를 반환.
 *
 * 한 쪽 파일이 손상되어도 다른 쪽은 그대로 살린다 — 즉 잘못된 user 파일이 정상 project
 * registry 를 무력화하지 않는다 (반대도 마찬가지). caller 는 `errors` 를 보고 logging /
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
