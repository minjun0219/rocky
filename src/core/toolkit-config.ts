import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join, resolve } from "node:path";

/**
 * openapi-core 의 단일 config 로더 + 검증.
 *
 * 두 위치를 병합한다 (project 가 leaf 단위로 user 를 덮어쓴다):
 *   1. user:    ~/.config/opencode/agent-toolkit/agent-toolkit.json
 *   2. project: <projectRoot>/.opencode/agent-toolkit.json
 *
 * 스키마는 repo 루트의 `agent-toolkit.schema.json` 에 동일 모양으로 박혀 있다 — IDE 자동완성용.
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
  format?: "openapi3" | "swagger2" | "auto";
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
  return typeof leaf === "string" ? leaf : leaf.url;
}

/** leaf 의 baseUrl. string leaf 또는 baseUrl 미선언 object 면 undefined. */
export function getRegistryBaseUrl(
  leaf: OpenapiRegistryLeaf,
): string | undefined {
  if (typeof leaf === "string") return undefined;
  return leaf.baseUrl;
}

/** leaf 의 format hint. string leaf 또는 format 미선언 object 면 undefined. */
export function getRegistryFormat(
  leaf: OpenapiRegistryLeaf,
): "openapi3" | "swagger2" | "auto" | undefined {
  if (typeof leaf === "string") return undefined;
  return leaf.format;
}

export interface ToolkitConfig {
  $schema?: string;
  openapi?: {
    registry?: OpenapiRegistry;
  };
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
  config: ToolkitConfig;
  /** 파싱 / 검증에 실패한 파일별 에러. caller 가 logging / surfacing 결정. */
  errors: LoadConfigError[];
}

/** user-level config 기본 경로. `AGENT_TOOLKIT_CONFIG` 로 오버라이드. */
export const USER_CONFIG_PATH = join(
  homedir(),
  ".config",
  "opencode",
  "agent-toolkit",
  "agent-toolkit.json",
);

/** project-level config 의 상대 경로. */
export const PROJECT_CONFIG_RELATIVE = ".opencode/agent-toolkit.json";

/**
 * host / env / spec 식별자 본문 (anchor 없음).
 * 다른 모듈 (`openapi-registry.ts`) 이 핸들 / 스코프 정규식을 합성할 때 재사용한다 —
 * 이 한 군데만 바꾸면 schema / registry 둘 다 같이 따라간다.
 */
export const ID_BODY = "[a-zA-Z0-9_-]+";

/** host / env / spec 식별자 정규식 (앵커 포함). 콜론은 handle separator 로 예약. */
export const ID_PATTERN = new RegExp(`^${ID_BODY}$`);

/** 레지스트리 leaf URL 에 허용되는 스킴. spec 다운로드 단이 받는 종류와 동일. */
const URL_SCHEMES = new Set(["http:", "https:", "file:"]);

/** registry leaf object 에서 허용하는 키 (오타 가드, 스키마 lockstep). */
const ALLOWED_REGISTRY_LEAF_KEYS = new Set(["url", "baseUrl", "format"]);

/** registry leaf object 의 format 필드에 허용되는 값. */
const ALLOWED_REGISTRY_LEAF_FORMATS = new Set(["openapi3", "swagger2", "auto"]);

/**
 * 파싱된 JSON 값이 ToolkitConfig 인지 검증한다. 어긋나면 throw — 메시지에 source(path) 포함.
 * 부분 적합도 OK (모든 필드 optional). registry 가 있으면 깊이 끝까지 식별자 / URL 검증.
 */
export function validateConfig(input: unknown, source: string): ToolkitConfig {
  if (input === null || typeof input !== "object" || Array.isArray(input)) {
    throw new Error(`${source}: config must be a JSON object`);
  }
  const config = input as Record<string, unknown>;
  if (config.openapi !== undefined) {
    if (
      config.openapi === null ||
      typeof config.openapi !== "object" ||
      Array.isArray(config.openapi)
    ) {
      throw new Error(`${source}: openapi must be an object`);
    }
    const oapi = config.openapi as Record<string, unknown>;
    if (oapi.registry !== undefined) {
      validateRegistry(oapi.registry, source);
    }
  }
  return config as ToolkitConfig;
}

function validateRegistry(
  reg: unknown,
  source: string,
): asserts reg is OpenapiRegistry {
  if (reg === null || typeof reg !== "object" || Array.isArray(reg)) {
    throw new Error(`${source}: openapi.registry must be an object`);
  }
  for (const [host, envs] of Object.entries(reg as Record<string, unknown>)) {
    if (!ID_PATTERN.test(host)) {
      throw new Error(
        `${source}: host name "${host}" must match ${ID_PATTERN} (alphanumeric, "_" or "-" only — colons are reserved for handle separators)`,
      );
    }
    if (envs === null || typeof envs !== "object" || Array.isArray(envs)) {
      throw new Error(
        `${source}: openapi.registry["${host}"] must be an object of environments`,
      );
    }
    for (const [env, specs] of Object.entries(
      envs as Record<string, unknown>,
    )) {
      if (!ID_PATTERN.test(env)) {
        throw new Error(
          `${source}: env name "${host}:${env}" must match ${ID_PATTERN}`,
        );
      }
      if (specs === null || typeof specs !== "object" || Array.isArray(specs)) {
        throw new Error(
          `${source}: openapi.registry["${host}"]["${env}"] must be an object of specs`,
        );
      }
      for (const [spec, leaf] of Object.entries(
        specs as Record<string, unknown>,
      )) {
        if (!ID_PATTERN.test(spec)) {
          throw new Error(
            `${source}: spec name "${host}:${env}:${spec}" must match ${ID_PATTERN}`,
          );
        }
        validateRegistryLeaf(
          leaf,
          `${source}: openapi.registry["${host}"]["${env}"]["${spec}"]`,
        );
      }
    }
  }
}

/**
 * registry leaf 검증. string (URL only) 또는 object (`{ url, baseUrl?, format? }`)
 * 모두 받는다. URL 은 둘 다 같은 스킴 / non-empty 검증을 통과해야 한다.
 */
function validateRegistryLeaf(leaf: unknown, where: string): void {
  if (typeof leaf === "string") {
    if (leaf.trim().length === 0) {
      throw new Error(`${where} must be a non-empty URL string`);
    }
    validateRegistryUrlString(leaf, where);
    return;
  }
  if (leaf === null || typeof leaf !== "object" || Array.isArray(leaf)) {
    throw new Error(
      `${where} must be a non-empty URL string or object { url, baseUrl?, format? }`,
    );
  }
  const obj = leaf as Record<string, unknown>;
  for (const key of Object.keys(obj)) {
    if (!ALLOWED_REGISTRY_LEAF_KEYS.has(key)) {
      throw new Error(
        `${where} has unsupported key "${key}" — allowed: ${[...ALLOWED_REGISTRY_LEAF_KEYS].join(", ")}`,
      );
    }
  }
  if (typeof obj.url !== "string" || obj.url.trim().length === 0) {
    throw new Error(`${where}.url must be a non-empty URL string`);
  }
  validateRegistryUrlString(obj.url, `${where}.url`);
  if (obj.baseUrl !== undefined) {
    if (typeof obj.baseUrl !== "string" || obj.baseUrl.trim().length === 0) {
      throw new Error(`${where}.baseUrl must be a non-empty string`);
    }
    let parsed: URL;
    try {
      parsed = new URL(obj.baseUrl);
    } catch {
      throw new Error(
        `${where}.baseUrl is not a valid URL — got "${obj.baseUrl}"`,
      );
    }
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
      throw new Error(
        `${where}.baseUrl uses unsupported scheme "${parsed.protocol}" — only http / https are accepted`,
      );
    }
  }
  if (obj.format !== undefined) {
    if (
      typeof obj.format !== "string" ||
      !ALLOWED_REGISTRY_LEAF_FORMATS.has(obj.format)
    ) {
      throw new Error(
        `${where}.format must be one of ${[...ALLOWED_REGISTRY_LEAF_FORMATS].join(" / ")}`,
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

async function loadOne(path: string): Promise<ToolkitConfig | null> {
  if (!existsSync(path)) return null;
  const raw = await readFile(path, "utf8");
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch (err) {
    throw new Error(
      `Failed to parse ${path} as JSON: ${(err as Error).message}`,
    );
  }
  return validateConfig(parsed, path);
}

/**
 * user → project 순서로 깊이 병합. 같은 leaf (host:env:spec) 는 project 가 이긴다.
 * 새 host / env / spec 은 project 쪽에서 추가 가능.
 */
export function mergeConfigs(
  user: ToolkitConfig,
  project: ToolkitConfig,
): ToolkitConfig {
  // Bun ≥ 1.0 / Node ≥ 17 모두 structuredClone 표준 지원. JSON round-trip 보다 성능과
  // 의도가 명확 — 입력은 plain JSON 모양이라 Date / Map / Set 호환은 신경 쓰지 않아도 된다.
  const out = structuredClone(user) as ToolkitConfig;
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
  return out;
}

/**
 * user + project config 를 읽어 merge 된 결과 + 파일별 에러를 반환.
 *
 * 한 쪽 파일이 손상되어도 다른 쪽은 그대로 살린다 — 즉 잘못된 user 파일이 정상 project
 * registry 를 무력화하지 않는다 (반대도 마찬가지). caller 는 `errors` 를 보고 logging /
 * surfacing 을 결정한다.
 *
 * 두 파일 모두 없으면 `{ config: {}, errors: [] }`. `AGENT_TOOLKIT_CONFIG` 환경변수가
 * 있으면 user 경로를 그 값으로 덮어쓴다.
 */
export async function loadConfig(
  options: LoadConfigOptions = {},
): Promise<LoadConfigResult> {
  const userPath =
    options.userPath ?? process.env.AGENT_TOOLKIT_CONFIG ?? USER_CONFIG_PATH;
  const projectRoot = options.projectRoot ?? process.cwd();
  const projectPath = resolve(projectRoot, PROJECT_CONFIG_RELATIVE);
  const errors: LoadConfigError[] = [];

  let user: ToolkitConfig = {};
  try {
    user = (await loadOne(userPath)) ?? {};
  } catch (err) {
    errors.push({
      source: userPath,
      message: err instanceof Error ? err.message : String(err),
    });
  }

  let project: ToolkitConfig = {};
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
