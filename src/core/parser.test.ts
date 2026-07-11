import { describe, it, expect } from "bun:test";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseSpecText, SpecParseError } from "./parser";

const FIX = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  "__fixtures__",
);

describe("parseSpecText", () => {
  it("parses petstore 3.0 JSON with auto detection", async () => {
    const raw = await readFile(path.join(FIX, "petstore-3.0.json"), "utf8");
    const { document, detectedFormat } = await parseSpecText(raw);
    expect(detectedFormat).toBe("openapi3");
    expect(document.openapi?.startsWith("3.")).toBe(true);
    expect(document.paths?.["/pet/{petId}"]).toBeDefined();
  });

  it("parses petstore 3.0 YAML with auto detection", async () => {
    const raw = await readFile(path.join(FIX, "petstore-3.0.yaml"), "utf8");
    const { document, detectedFormat } = await parseSpecText(raw);
    expect(detectedFormat).toBe("openapi3");
    expect(document.openapi?.startsWith("3.")).toBe(true);
  });

  it("auto-converts swagger 2.0 to OpenAPI 3.0", async () => {
    const raw = await readFile(path.join(FIX, "petstore-2.0.json"), "utf8");
    const { document, detectedFormat } = await parseSpecText(raw);
    expect(detectedFormat).toBe("swagger2");
    expect(document.openapi?.startsWith("3.")).toBe(true);
    expect(
      (document as unknown as Record<string, unknown>).swagger,
    ).toBeUndefined();
  });

  it("throws SpecParseError when neither openapi nor swagger field present", async () => {
    await expect(
      parseSpecText(JSON.stringify({ info: {}, paths: {} })),
    ).rejects.toThrow(SpecParseError);
  });

  it("dereferences $ref pointers", async () => {
    const raw = await readFile(path.join(FIX, "petstore-3.0.json"), "utf8");
    const { document } = await parseSpecText(raw);
    const op = document.paths?.["/pet/{petId}"]?.get;
    // 'parameters' should be inline objects after deref, not $ref placeholders.
    expect(Array.isArray(op?.parameters)).toBe(true);
    for (const p of op?.parameters ?? []) {
      expect((p as unknown as Record<string, unknown>).$ref).toBeUndefined();
    }
  });
});
