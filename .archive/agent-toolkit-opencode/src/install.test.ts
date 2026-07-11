import { describe, expect, it } from "bun:test";
import {
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const PACKAGE_ROOT = resolve(import.meta.dir, "..");
const PACKAGE_JSON_PATH = join(PACKAGE_ROOT, "package.json");

function readPackageJson() {
  return JSON.parse(readFileSync(PACKAGE_JSON_PATH, "utf8")) as {
    name?: string;
    main?: string;
    exports?: string | Record<string, string | { import?: string }>;
    scripts?: Record<string, string>;
  };
}

function resolveExport(
  value: string | { import?: string } | undefined,
): string {
  if (typeof value === "string") return value;
  return value?.import ?? "";
}

describe("agent-toolkit-opencode install metadata", () => {
  it("keeps the published entrypoint aligned with src/index.ts", () => {
    const pkg = readPackageJson();
    const main = pkg.main ?? "";
    const exportsObj =
      typeof pkg.exports === "object" && pkg.exports !== null
        ? (pkg.exports as Record<string, string | { import?: string }>)
        : {};
    const rootImport = resolveExport(exportsObj["."]);
    const serverImport = resolveExport(exportsObj["./server"]);

    expect(main).toBe("./src/index.ts");
    expect(rootImport).toBe(main);
    expect(serverImport).toBe(main);
    expect(main.startsWith("./")).toBe(true);
    expect(main.includes("dist")).toBe(false);
    expect(existsSync(resolve(PACKAGE_ROOT, main))).toBe(true);

    const scripts = pkg.scripts ?? {};
    expect("build" in scripts).toBe(false);
    expect("prepare" in scripts).toBe(false);
    expect("prepublishOnly" in scripts).toBe(false);
  });

  it.each([
    ["root", "@minjun0219/agent-toolkit-opencode", "import-root.ts"],
    ["server", "@minjun0219/agent-toolkit-opencode/server", "import-server.ts"],
  ])("loads the plugin through the package %s export", (_name, specifier, fileName) => {
    const tempDir = mkdtempSync(join(tmpdir(), "agent-toolkit-install-"));

    try {
      const packageDir = join(
        tempDir,
        "node_modules",
        "@minjun0219",
        "agent-toolkit-opencode",
      );
      const corePackageDir = join(
        tempDir,
        "node_modules",
        "@minjun0219",
        "openapi-core",
      );
      mkdirSync(join(tempDir, "node_modules", "@minjun0219"), {
        recursive: true,
      });
      symlinkSync(PACKAGE_ROOT, packageDir, "dir");
      symlinkSync(
        resolve(PACKAGE_ROOT, "..", "openapi-core"),
        corePackageDir,
        "dir",
      );
      writeFileSync(
        join(tempDir, fileName),
        `import plugin from "${specifier}";\nconsole.log(typeof plugin);\n`,
      );

      const result = Bun.spawnSync(["bun", fileName], {
        cwd: tempDir,
        stdout: "pipe",
        stderr: "pipe",
      });

      expect(result.exitCode).toBe(0);
      expect(result.stdout.toString().trim()).toBe("function");
    } finally {
      rmSync(tempDir, { recursive: true, force: true });
    }
  });
});
