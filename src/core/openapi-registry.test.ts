import { describe, it, expect } from "bun:test";
import {
  isFullHandle,
  isScope,
  listRegistry,
  resolveHandleToUrl,
  resolveScopeToUrls,
} from "./openapi-registry";
import type { ToolkitConfig } from "./toolkit-config";

const CONFIG: ToolkitConfig = {
  openapi: {
    registry: {
      acme: {
        dev: {
          users: "https://dev.acme/users.json",
          orders: "https://dev.acme/orders.json",
        },
        prod: {
          users: "https://api.acme/users.json",
        },
      },
      beta: {
        dev: {
          svc: "https://dev.beta/svc.json",
        },
      },
    },
  },
};

describe("isFullHandle", () => {
  it("matches host:env:spec", () => {
    expect(isFullHandle("acme:dev:users")).toBe(true);
  });
  it("rejects partial / extra colons", () => {
    expect(isFullHandle("acme:dev")).toBe(false);
    expect(isFullHandle("acme")).toBe(false);
    expect(isFullHandle("acme:dev:users:extra")).toBe(false);
  });
  it("rejects URLs and hex keys", () => {
    expect(isFullHandle("https://example.com/spec.json")).toBe(false);
    expect(isFullHandle("0123456789abcdef")).toBe(false);
  });
});

describe("isScope", () => {
  it("accepts host / host:env / host:env:spec", () => {
    expect(isScope("acme")).toBe(true);
    expect(isScope("acme:dev")).toBe(true);
    expect(isScope("acme:dev:users")).toBe(true);
  });
  it("rejects URLs, hex keys, empty", () => {
    expect(isScope("https://example.com/spec.json")).toBe(false);
    expect(isScope("file:///tmp/spec.json")).toBe(false);
    expect(isScope("0123456789abcdef")).toBe(false);
    expect(isScope("")).toBe(false);
  });
  it("rejects identifiers with disallowed chars", () => {
    expect(isScope("ac me")).toBe(false);
    expect(isScope("acme:de v")).toBe(false);
  });
});

describe("resolveHandleToUrl", () => {
  it("returns the registered URL for a known handle", () => {
    expect(resolveHandleToUrl("acme:dev:users", CONFIG.openapi?.registry)).toBe(
      "https://dev.acme/users.json",
    );
  });
  it("throws on unregistered handle, with the handle in the message", () => {
    expect(() =>
      resolveHandleToUrl("acme:dev:nope", CONFIG.openapi?.registry),
    ).toThrow(/acme:dev:nope/);
  });
  it("throws on malformed handle", () => {
    expect(() =>
      resolveHandleToUrl("acme:dev", CONFIG.openapi?.registry),
    ).toThrow(/three colon-separated/i);
  });
  it("throws when registry is undefined", () => {
    expect(() => resolveHandleToUrl("acme:dev:users", undefined)).toThrow(
      /not found/i,
    );
  });
});

describe("resolveScopeToUrls", () => {
  const reg = CONFIG.openapi?.registry;

  it("expands a host scope to all of its specs", () => {
    const urls = resolveScopeToUrls("acme", reg);
    expect(urls.sort()).toEqual(
      [
        "https://api.acme/users.json",
        "https://dev.acme/orders.json",
        "https://dev.acme/users.json",
      ].sort(),
    );
  });

  it("expands a host:env scope to that env's specs only", () => {
    const urls = resolveScopeToUrls("acme:dev", reg);
    expect(urls.sort()).toEqual(
      ["https://dev.acme/orders.json", "https://dev.acme/users.json"].sort(),
    );
  });

  it("expands a host:env:spec scope to a one-element list", () => {
    expect(resolveScopeToUrls("acme:prod:users", reg)).toEqual([
      "https://api.acme/users.json",
    ]);
  });

  it("returns an empty array for unknown / malformed scopes", () => {
    expect(resolveScopeToUrls("nope", reg)).toEqual([]);
    expect(resolveScopeToUrls("acme:nope", reg)).toEqual([]);
    expect(resolveScopeToUrls("acme:dev:nope", reg)).toEqual([]);
    expect(resolveScopeToUrls("not a scope", reg)).toEqual([]);
    expect(resolveScopeToUrls("", reg)).toEqual([]);
  });

  it("returns an empty array when the registry is undefined", () => {
    expect(resolveScopeToUrls("acme:dev:users", undefined)).toEqual([]);
  });
});

describe("listRegistry", () => {
  it("flattens host/env/spec/url tree", () => {
    const flat = listRegistry(CONFIG);
    expect(flat.length).toBe(4);
    const got = flat.map((e) => `${e.host}:${e.env}:${e.spec}=${e.url}`).sort();
    expect(got).toEqual(
      [
        "acme:dev:users=https://dev.acme/users.json",
        "acme:dev:orders=https://dev.acme/orders.json",
        "acme:prod:users=https://api.acme/users.json",
        "beta:dev:svc=https://dev.beta/svc.json",
      ].sort(),
    );
  });

  it("returns [] for empty config", () => {
    expect(listRegistry({})).toEqual([]);
    expect(listRegistry({ openapi: {} })).toEqual([]);
    expect(listRegistry({ openapi: { registry: {} } })).toEqual([]);
  });
});
