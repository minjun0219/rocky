import { describe, it, expect } from "bun:test";
import {
  buildCombinedConfig,
  buildEphemeralSpec,
  ephemeralSpecName,
  flattenHandle,
  flattenRegistry,
  parseFlatHandle,
  registryToOpenApiMcpConfig,
} from "./adapter";
import type { OpenapiRegistry } from "./toolkit-config";

describe("flatten / parse handle", () => {
  it("round-trips host:env:spec via ':' separator", () => {
    expect(flattenHandle("acme", "dev", "users")).toBe("acme:dev:users");
    expect(parseFlatHandle("acme:dev:users")).toEqual({
      host: "acme",
      env: "dev",
      spec: "users",
    });
  });
  it("rejects shapes that aren't exactly three parts", () => {
    expect(parseFlatHandle("acme:dev")).toBeNull();
    expect(parseFlatHandle("acme:dev:users:extra")).toBeNull();
  });
  it("does not collide on identifiers that contain underscores", () => {
    // ID_BODY 가 `_` 를 허용하므로 구 separator (`__`) 였다면 충돌했을 두 핸들이
    // 새 separator (`:`) 에선 서로 다른 specName 으로 떨어진다.
    expect(flattenHandle("a", "b__c", "d")).not.toBe(
      flattenHandle("a__b", "c", "d"),
    );
  });
});

describe("registryToOpenApiMcpConfig", () => {
  it("flattens host:env:spec into specs.<host:env:spec>", () => {
    const reg: OpenapiRegistry = {
      acme: {
        dev: {
          users: "https://example.com/u.json",
          orders: {
            url: "https://example.com/o.json",
            baseUrl: "https://api.dev/o",
          },
        },
      },
    };
    const cfg = registryToOpenApiMcpConfig(reg);
    expect(Object.keys(cfg.specs).sort()).toEqual([
      "acme:dev:orders",
      "acme:dev:users",
    ]);
    expect(cfg.specs["acme:dev:users"]?.environments.default?.baseUrl).toBe("");
    expect(cfg.specs["acme:dev:orders"]?.environments.default?.baseUrl).toBe(
      "https://api.dev/o",
    );
  });

  it("returns empty specs map when registry is undefined", () => {
    expect(registryToOpenApiMcpConfig(undefined)).toEqual({ specs: {} });
  });

  it("injects defaultCacheTtlSeconds into each spec when provided", () => {
    const reg: OpenapiRegistry = {
      h: { e: { s: "https://example.com/x.json" } },
    };
    const cfg = registryToOpenApiMcpConfig(reg, 42);
    expect(cfg.specs["h:e:s"]?.cacheTtlSeconds).toBe(42);
  });
});

describe("ephemeral spec helpers", () => {
  it("ephemeralSpecName uses url:<sha1-16> shape", () => {
    const n = ephemeralSpecName("https://example.com/a.json");
    expect(n.startsWith("url:")).toBe(true);
    expect(n.length).toBe("url:".length + 16);
  });
  it("buildEphemeralSpec attaches an empty-baseUrl environment", () => {
    const { name, spec } = buildEphemeralSpec("https://example.com/a.json");
    expect(name.startsWith("url:")).toBe(true);
    expect(spec.environments.default?.baseUrl).toBe("");
  });
  it("buildEphemeralSpec applies defaultCacheTtlSeconds when provided", () => {
    const { spec } = buildEphemeralSpec("https://example.com/a.json", 99);
    expect(spec.cacheTtlSeconds).toBe(99);
  });
});

describe("buildCombinedConfig + flattenRegistry", () => {
  it("merges registry-derived specs with ad-hoc URL specs and dedupes URLs", () => {
    const reg: OpenapiRegistry = {
      acme: { dev: { u: "https://example.com/u.json" } },
    };
    const combined = buildCombinedConfig({
      registry: reg,
      ephemeralUrls: [
        "https://example.com/x.json",
        "https://example.com/x.json",
      ],
    });
    expect(Object.keys(combined.specs).length).toBe(2);
  });

  it("flattenRegistry includes baseUrl/format only when present", () => {
    const reg: OpenapiRegistry = {
      acme: {
        dev: {
          a: "https://example.com/a.json",
          b: {
            url: "https://example.com/b.json",
            baseUrl: "https://api/b",
            format: "swagger2",
          },
        },
      },
    };
    const rows = flattenRegistry(reg);
    const a = rows.find((r) => r.spec === "a");
    const b = rows.find((r) => r.spec === "b");
    expect(a?.baseUrl).toBeUndefined();
    expect(a?.format).toBeUndefined();
    expect(b?.baseUrl).toBe("https://api/b");
    expect(b?.format).toBe("swagger2");
  });
});
