import { afterEach, describe, expect, it } from "bun:test";
import { mkdtempSync, rmSync, mkdirSync, writeFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createDiskCache } from "./cache";
import { createSpecRegistry, type SpecRegistry } from "./registry";
import {
  createFetcher,
  type ConditionalHeaders,
  type FetchOutcome,
  type SpecFetcher,
} from "./fetcher";
import type { OpenApiMcpConfig, SpecSource } from "./schema";

const FIX = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  "__fixtures__",
);
const PETSTORE_3 = path.join(FIX, "petstore-3.0.json");
const PETSTORE_2 = path.join(FIX, "petstore-2.0.json");

class ProgrammableFetcher implements SpecFetcher {
  fetchCount = 0;
  conditionalCalls: ConditionalHeaders[] = [];
  body: string;
  etag: string | undefined;
  notModifiedNext = false;

  constructor(body: string, opts: { etag?: string } = {}) {
    this.body = body;
    this.etag = opts.etag;
  }

  async fetch(
    _source: unknown,
    conditional?: ConditionalHeaders,
  ): Promise<FetchOutcome> {
    this.fetchCount += 1;
    this.conditionalCalls.push(conditional ?? {});
    if (this.notModifiedNext) {
      this.notModifiedNext = false;
      return {
        notModified: true,
        fetchedAt: new Date().toISOString(),
        source: { type: "file", path: "inline" },
      };
    }
    return {
      notModified: false,
      body: this.body,
      ...(this.etag !== undefined ? { etag: this.etag } : {}),
      fetchedAt: new Date().toISOString(),
      source: { type: "file", path: "inline" },
    };
  }
}

const tempDirs: string[] = [];
function makeTempDir(): string {
  const dir = mkdtempSync(path.join(os.tmpdir(), "openapi-mcp-cache-"));
  tempDirs.push(dir);
  return dir;
}

afterEach(() => {
  while (tempDirs.length > 0) {
    rmSync(tempDirs.pop()!, { recursive: true, force: true });
  }
});

function makeConfig(opts: { ttl?: number } = {}): OpenApiMcpConfig {
  return {
    specs: {
      petstore: {
        source: { type: "file", path: PETSTORE_3 },
        environments: {
          dev: { baseUrl: "https://api.dev.example.com/petstore" },
        },
        ...(opts.ttl !== undefined ? { cacheTtlSeconds: opts.ttl } : {}),
      },
    },
  };
}

async function loadAll(reg: SpecRegistry): Promise<void> {
  await reg.loadSpec("petstore", "dev");
}

async function loadBody(): Promise<string> {
  return readFile(PETSTORE_3, "utf8");
}

async function flushBackground(): Promise<void> {
  await new Promise((r) => setTimeout(r, 100));
}

describe("caching behaviour", () => {
  it("persists fetched specs to disk and re-hydrates on a fresh registry", async () => {
    const dir = makeTempDir();
    const disk = createDiskCache(dir);
    const body = await loadBody();
    const fetcher1 = new ProgrammableFetcher(body, { etag: "v1" });
    const reg1 = createSpecRegistry(makeConfig(), fetcher1, {
      diskCache: disk,
    });
    await loadAll(reg1);
    expect(fetcher1.fetchCount).toBe(1);

    const fetcher2 = new ProgrammableFetcher(body, { etag: "v1" });
    const reg2 = createSpecRegistry(makeConfig({ ttl: 3600 }), fetcher2, {
      diskCache: disk,
    });
    await loadAll(reg2);
    expect(fetcher2.fetchCount).toBe(0);
  });

  it("serves stale data and triggers background refresh with conditional headers", async () => {
    const dir = makeTempDir();
    const disk = createDiskCache(dir);
    const body = await loadBody();
    const fetcher = new ProgrammableFetcher(body, { etag: "abc" });
    const reg = createSpecRegistry(makeConfig({ ttl: 1 }), fetcher, {
      diskCache: disk,
    });
    await loadAll(reg);
    expect(fetcher.fetchCount).toBe(1);

    await new Promise((r) => setTimeout(r, 1100));
    fetcher.notModifiedNext = true;
    await loadAll(reg);
    await flushBackground();

    expect(fetcher.fetchCount).toBe(2);
    expect(fetcher.conditionalCalls[1]).toEqual({ etag: "abc" });
  });

  it("hydrates a swagger2-formatted source from disk without re-fetching", async () => {
    const dir = makeTempDir();
    const disk = createDiskCache(dir);
    const swaggerBody = await readFile(PETSTORE_2, "utf8");
    const config: OpenApiMcpConfig = {
      specs: {
        petstore: {
          source: { type: "file", path: PETSTORE_2, format: "swagger2" },
          environments: {
            dev: { baseUrl: "https://api.dev.example.com/petstore" },
          },
          cacheTtlSeconds: 3600,
        },
      },
    };
    const fetcher1 = new ProgrammableFetcher(swaggerBody);
    const reg1 = createSpecRegistry(config, fetcher1, { diskCache: disk });
    await reg1.loadSpec("petstore", "dev");
    expect(fetcher1.fetchCount).toBe(1);

    const fetcher2 = new ProgrammableFetcher(swaggerBody);
    const reg2 = createSpecRegistry(config, fetcher2, { diskCache: disk });
    const indexed = await reg2.loadSpec("petstore", "dev");
    expect(fetcher2.fetchCount).toBe(0);
    expect(indexed.document.openapi).toMatch(/^3\./);
  });

  it("resolves relative file source paths against configDir", async () => {
    const dir = makeTempDir();
    const subdir = path.join(dir, "specs");
    mkdirSync(subdir, { recursive: true });
    const target = path.join(subdir, "pet.json");
    const body = await readFile(PETSTORE_3, "utf8");
    writeFileSync(target, body);

    const relativeSource: SpecSource = {
      type: "file",
      path: "./specs/pet.json",
    };
    const config: OpenApiMcpConfig = {
      specs: {
        petstore: {
          source: relativeSource,
          environments: {
            dev: { baseUrl: "https://api.dev.example.com/petstore" },
          },
        },
      },
    };
    const reg = createSpecRegistry(config, createFetcher(), { configDir: dir });
    const indexed = await reg.loadSpec("petstore", "dev");
    expect(indexed.byOperationId.get("addPet")).toBeDefined();
  });

  it("refresh_spec drops cache and re-fetches without conditional headers", async () => {
    const dir = makeTempDir();
    const disk = createDiskCache(dir);
    const body = await loadBody();
    const fetcher = new ProgrammableFetcher(body, { etag: "v1" });
    const reg = createSpecRegistry(makeConfig({ ttl: 3600 }), fetcher, {
      diskCache: disk,
    });
    await loadAll(reg);
    expect(fetcher.fetchCount).toBe(1);

    const result = await reg.refresh("petstore");
    expect(result[0]?.success).toBe(true);
    expect(fetcher.fetchCount).toBe(2);
    expect(fetcher.conditionalCalls[1]).toEqual({});
  });

  it("registerSpec adds a new ad-hoc spec at runtime", async () => {
    const fetcher = new ProgrammableFetcher(await loadBody());
    const reg = createSpecRegistry(makeConfig(), fetcher, {});
    expect(reg.hasSpec("petstore")).toBe(true);
    expect(reg.hasSpec("ad-hoc")).toBe(false);
    reg.registerSpec("ad-hoc", {
      source: { type: "file", path: PETSTORE_3 },
      environments: { default: { baseUrl: "" } },
    });
    expect(reg.hasSpec("ad-hoc")).toBe(true);
    const ix = await reg.loadSpec("ad-hoc", "default");
    expect(ix.endpoints.length).toBeGreaterThan(0);
  });
});
