import { describe, it, expect, beforeEach } from "bun:test";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { createAgentToolkitRegistry } from "./adapter";
import {
  handleSwaggerEndpoint,
  handleSwaggerEnvs,
  handleSwaggerGet,
  handleSwaggerRefresh,
  handleSwaggerSearch,
  handleSwaggerStatus,
  handleSwaggerTags,
} from "./handlers";
import type { OpenapiRegistry } from "./toolkit-config";

const FIXTURE_DIR = join(
  dirname(fileURLToPath(import.meta.url)),
  "__fixtures__",
);
const PETSTORE_3 = `file://${join(FIXTURE_DIR, "petstore-3.0.json")}`;
const PETSTORE_2 = `file://${join(FIXTURE_DIR, "petstore-2.0.json")}`;

describe("openapi handlers — file URL inputs", () => {
  it("openapi_get: file URL input gets parsed + dereferenced + cached", async () => {
    const reg = createAgentToolkitRegistry({ diskCacheDisabled: true });
    const r = await handleSwaggerGet(reg, PETSTORE_3);
    expect(r.fromCache).toBe(false);
    expect(r.document.openapi?.startsWith("3.")).toBe(true);
    expect(r.document.info?.title).toBe("Swagger Petstore");
    // 두 번째 호출은 메모리 캐시 hit.
    const r2 = await handleSwaggerGet(reg, PETSTORE_3);
    expect(r2.fromCache).toBe(true);
  });

  it("openapi_get: swagger 2.0 fixture is auto-converted to OpenAPI 3", async () => {
    const reg = createAgentToolkitRegistry({ diskCacheDisabled: true });
    const r = await handleSwaggerGet(reg, PETSTORE_2);
    expect(r.document.openapi?.startsWith("3.")).toBe(true);
    // swagger 본문엔 swagger 필드가 있었지만 변환 후엔 openapi 만 있어야 한다.
    expect(
      (r.document as unknown as Record<string, unknown>).swagger,
    ).toBeUndefined();
  });

  it("openapi_status / openapi_refresh: cache lifecycle", async () => {
    const reg = createAgentToolkitRegistry({ diskCacheDisabled: true });
    const before = await handleSwaggerStatus(reg, PETSTORE_3);
    expect(before.cacheStatus.cached).toBe(false);

    await handleSwaggerGet(reg, PETSTORE_3);
    const after = await handleSwaggerStatus(reg, PETSTORE_3);
    expect(after.cacheStatus.cached).toBe(true);

    const refreshed = await handleSwaggerRefresh(reg, PETSTORE_3);
    expect(refreshed.length).toBe(1);
    expect(refreshed[0]?.success).toBe(true);
  });

  it("openapi_search: scored keyword across loaded specs", async () => {
    const reg = createAgentToolkitRegistry({ diskCacheDisabled: true });
    await handleSwaggerGet(reg, PETSTORE_3);
    const matches = await handleSwaggerSearch(reg, "pet");
    expect(matches.length).toBeGreaterThan(0);
    // 점수화 검색은 operationId / path / summary / description 모두를 본다 — petstore
    // 의 store / user 도메인도 summary 에 "pet" 단어가 있으면 매칭될 수 있다. 핵심은
    // 첫 매칭이 path / operationId 같은 high-score 필드에서 잡혔는지.
    const top = matches[0];
    expect(top).toBeDefined();
    expect(
      top!.path.toLowerCase().includes("pet") ||
        top!.operationId.toLowerCase().includes("pet"),
    ).toBe(true);

    const limited = await handleSwaggerSearch(reg, "", { limit: 2 });
    expect(limited.length).toBe(2);
  });

  it("openapi_endpoint: returns full detail with fullUrl", async () => {
    const reg = createAgentToolkitRegistry({ diskCacheDisabled: true });
    const detail = await handleSwaggerEndpoint(reg, PETSTORE_3, {
      method: "GET",
      path: "/pet/{petId}",
    });
    expect(detail.endpoint.method).toBe("GET");
    expect(detail.endpoint.path).toBe("/pet/{petId}");
    expect(detail.endpoint.parameters.some((p) => p.name === "petId")).toBe(
      true,
    );
    // baseUrl 미선언이라 fullUrl 은 path 자체.
    expect(detail.endpoint.fullUrl).toBe("/pet/{petId}");
  });

  it("openapi_tags: returns tag summaries", async () => {
    const reg = createAgentToolkitRegistry({ diskCacheDisabled: true });
    const r = await handleSwaggerTags(reg, PETSTORE_3);
    expect(r.tags.length).toBeGreaterThan(0);
    expect(r.tags[0]).toHaveProperty("endpointCount");
  });

  it("rejects non-URL non-handle inputs clearly", async () => {
    const reg = createAgentToolkitRegistry({ diskCacheDisabled: true });
    await expect(handleSwaggerGet(reg, "not-a-url")).rejects.toThrow(
      /not a host:env:spec handle or http\(s\)\/file URL/,
    );
  });
});

describe("openapi handlers — registry handles", () => {
  let registry: OpenapiRegistry;

  beforeEach(() => {
    // string leaf (legacy) 와 object leaf (baseUrl 포함) 를 섞어 둔다.
    registry = {
      acme: {
        dev: {
          users: PETSTORE_3,
          // object leaf — baseUrl 합성을 검증.
          orders: { url: PETSTORE_3, baseUrl: "https://api.dev/orders" },
        },
        prod: {
          users: PETSTORE_3,
        },
      },
    };
  });

  it("openapi_get accepts a host:env:spec handle and returns flat spec name", async () => {
    const reg = createAgentToolkitRegistry({
      registry,
      diskCacheDisabled: true,
    });
    const r = await handleSwaggerGet(reg, "acme:dev:users", registry);
    expect(r.spec).toBe("acme:dev:users");
    expect(r.environment).toBe("default");
  });

  it("openapi_endpoint with a baseUrl-bearing leaf returns a synthesized fullUrl", async () => {
    const reg = createAgentToolkitRegistry({
      registry,
      diskCacheDisabled: true,
    });
    const detail = await handleSwaggerEndpoint(
      reg,
      "acme:dev:orders",
      { method: "GET", path: "/pet/{petId}" },
      registry,
    );
    expect(detail.endpoint.fullUrl).toBe("https://api.dev/orders/pet/{petId}");
  });

  it("openapi_get throws on unregistered handle", async () => {
    const reg = createAgentToolkitRegistry({
      registry,
      diskCacheDisabled: true,
    });
    await expect(
      handleSwaggerGet(reg, "acme:dev:missing", registry),
    ).rejects.toThrow(/acme:dev:missing/);
  });

  it("openapi_envs returns the flat registry list with baseUrl when present", () => {
    const flat = handleSwaggerEnvs({ openapi: { registry } });
    expect(flat.length).toBe(3);
    const orders = flat.find((r) => r.spec === "orders");
    expect(orders?.baseUrl).toBe("https://api.dev/orders");
    const users = flat.find((r) => r.spec === "users" && r.env === "dev");
    expect(users?.baseUrl).toBeUndefined();
  });

  it("openapi_envs returns [] for empty config", () => {
    expect(handleSwaggerEnvs({})).toEqual([]);
  });
});
