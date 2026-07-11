import os from "node:os";
import path from "node:path";
import { readFile } from "node:fs/promises";
import yaml from "js-yaml";
import { ZodError } from "zod";
import { OpenApiMcpConfigSchema, type OpenApiMcpConfig } from "./schema";

/**
 * `bin/openapi-mcp` 단독 진입점이 받는 config 파일 (`openapi-mcp.json` /
 * `.yaml` / `.yml`) 을 읽어 검증한다. agent-toolkit 의 `openapi.registry` 와는
 * 별도 — 이쪽은 `specs.<name>.environments.<env>.baseUrl` 트리를 그대로 받는다.
 */

export class ConfigError extends Error {
  constructor(
    message: string,
    public override readonly cause?: unknown,
  ) {
    super(message);
    this.name = "ConfigError";
  }
}

export interface LoadedConfig {
  config: OpenApiMcpConfig;
  path: string;
}

/** XDG 우선 기본 config 경로. CLI `--config` 가 없을 때 사용. */
export function defaultConfigPath(): string {
  const xdg = process.env.XDG_CONFIG_HOME;
  const base = xdg && xdg.length > 0 ? xdg : path.join(os.homedir(), ".config");
  return path.join(base, "openapi-mcp", "openapi-mcp.json");
}

/** XDG 우선 기본 디스크 캐시 디렉토리. config.cache.diskCachePath 가 없을 때 사용. */
export function defaultDiskCacheDir(): string {
  const xdg = process.env.XDG_CACHE_HOME;
  const base = xdg && xdg.length > 0 ? xdg : path.join(os.homedir(), ".cache");
  return path.join(base, "openapi-mcp");
}

export async function loadConfig(configPath: string): Promise<LoadedConfig> {
  const absolute = path.resolve(configPath);
  let raw: string;
  try {
    raw = await readFile(absolute, "utf8");
  } catch (err) {
    const reason = err instanceof Error ? err.message : String(err);
    throw new ConfigError(
      `failed to read config file at ${absolute}: ${reason}`,
      err,
    );
  }

  const parsed = parseByExtension(absolute, raw);
  try {
    const config = OpenApiMcpConfigSchema.parse(parsed);
    return { config, path: absolute };
  } catch (err) {
    if (err instanceof ZodError) {
      throw new ConfigError(formatZodError(absolute, err), err);
    }
    throw err;
  }
}

function parseByExtension(filePath: string, raw: string): unknown {
  const ext = path.extname(filePath).toLowerCase();
  if (ext === ".json") {
    try {
      return JSON.parse(raw);
    } catch (err) {
      throw new ConfigError(`failed to parse JSON config at ${filePath}`, err);
    }
  }
  if (ext === ".yaml" || ext === ".yml") {
    try {
      return yaml.load(raw);
    } catch (err) {
      throw new ConfigError(`failed to parse YAML config at ${filePath}`, err);
    }
  }
  throw new ConfigError(
    `unsupported config extension '${ext}' at ${filePath} (expected .json, .yaml, .yml)`,
  );
}

function formatZodError(filePath: string, err: ZodError): string {
  const lines = err.issues.map((issue) => {
    const where = issue.path.length > 0 ? issue.path.join(".") : "<root>";
    return `  - ${where}: ${issue.message}`;
  });
  return `invalid config at ${filePath}:\n${lines.join("\n")}`;
}
