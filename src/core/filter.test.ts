import { describe, it, expect } from "bun:test";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseSpecText } from "./parser";
import { indexSpec } from "./indexer";
import { filterEndpoints } from "./filter";

const FIX = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  "__fixtures__",
);
const PETSTORE_3 = path.join(FIX, "petstore-3.0.json");

async function loadEndpoints() {
  const raw = await readFile(PETSTORE_3, "utf8");
  const { document } = await parseSpecText(raw);
  return indexSpec("petstore", document).endpoints;
}

describe("filterEndpoints", () => {
  it("filters by tag", async () => {
    const endpoints = await loadEndpoints();
    const tagFiltered = filterEndpoints(endpoints, { tag: "store" });
    expect(tagFiltered.map((e) => e.path).sort()).toEqual([
      "/store/inventory",
      "/store/order",
    ]);
  });

  it("filters by HTTP method", async () => {
    const endpoints = await loadEndpoints();
    const posts = filterEndpoints(endpoints, { method: "POST" });
    expect(posts.map((e) => e.path).sort()).toEqual(["/pet", "/store/order"]);
  });

  it("keyword matches operationId, path, summary, description", async () => {
    const endpoints = await loadEndpoints();
    const hits = filterEndpoints(endpoints, { keyword: "inventory" });
    expect(hits.map((e) => e.path)).toEqual(["/store/inventory"]);
  });

  it("orders matches by where the keyword hits (operationId > path > summary)", async () => {
    const endpoints = await loadEndpoints();
    const ranked = filterEndpoints(endpoints, { keyword: "pet" });
    expect(ranked[0]).toBeDefined();
    expect(ranked[0]?.operationId?.toLowerCase().includes("pet")).toBe(true);
  });
});
