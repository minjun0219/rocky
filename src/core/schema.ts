import { z } from "zod";

/**
 * `openapi-mcp` 단독 진입점 (`bin/openapi-mcp`) 이 받는 config 파일의 스키마.
 *
 * agent-toolkit 의 `openapi.registry` (host:env:spec → URL leaf) 와는 다른 모양이며,
 * 이쪽은 `specs.<name>.environments.<env>.baseUrl` 트리를 그대로 받는다 — 두 형태
 * 모두 `lib/openapi/adapter.ts` 가 SpecRegistry 입력으로 변환한다.
 *
 * zod v4 (agent-toolkit 의 prod dep) 기준으로 작성되어 있으므로 v3 호환 API 는 쓰지 않는다.
 */

const SpecFormatSchema = z.enum(["openapi3", "swagger2", "auto"]);

const SpecSourceSchema = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("url"),
    url: z.string().url(),
    format: SpecFormatSchema.optional(),
  }),
  z.object({
    type: z.literal("file"),
    path: z.string().min(1),
    format: SpecFormatSchema.optional(),
  }),
]);

const EnvironmentConfigSchema = z.object({
  baseUrl: z.string().url(),
  description: z.string().optional(),
  source: SpecSourceSchema.optional(),
});

const SpecConfigSchema = z.object({
  description: z.string().optional(),
  source: SpecSourceSchema,
  environments: z
    .record(z.string().min(1), EnvironmentConfigSchema)
    .refine((envs) => Object.keys(envs).length > 0, {
      message: "each spec must declare at least one environment",
    }),
  cacheTtlSeconds: z.number().int().positive().optional(),
});

const GlobalCacheConfigSchema = z.object({
  diskCache: z.boolean().optional(),
  diskCachePath: z.string().optional(),
});

const GlobalHttpConfigSchema = z.object({
  timeoutMs: z.number().int().positive().optional(),
  insecureTls: z.boolean().optional(),
  extraCaCerts: z.array(z.string()).optional(),
});

export const OpenApiMcpConfigSchema = z.object({
  specs: z
    .record(z.string().min(1), SpecConfigSchema)
    .refine((specs) => Object.keys(specs).length > 0, {
      message: "config.specs must contain at least one entry",
    }),
  cache: GlobalCacheConfigSchema.optional(),
  http: GlobalHttpConfigSchema.optional(),
});

export type SpecFormat = z.infer<typeof SpecFormatSchema>;
export type SpecSource = z.infer<typeof SpecSourceSchema>;
export type EnvironmentConfig = z.infer<typeof EnvironmentConfigSchema>;
export type SpecConfig = z.infer<typeof SpecConfigSchema>;
export type GlobalCacheConfig = z.infer<typeof GlobalCacheConfigSchema>;
export type GlobalHttpConfig = z.infer<typeof GlobalHttpConfigSchema>;
export type OpenApiMcpConfig = z.infer<typeof OpenApiMcpConfigSchema>;

export const DEFAULT_CACHE_TTL_SECONDS = 300;
export const DEFAULT_HTTP_TIMEOUT_MS = 10_000;
