import { describe, it, expect, beforeEach } from 'bun:test';
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { loadConfig, mergeConfigs, validateConfig, type RockyConfig } from './rocky-config';

let userDir: string;
let userPath: string;
let projectRoot: string;

beforeEach(() => {
  const root = mkdtempSync(join(tmpdir(), 'rocky-config-'));
  userDir = join(root, 'user');
  mkdirSync(userDir, { recursive: true });
  userPath = join(userDir, 'rocky.json');
  projectRoot = mkdtempSync(join(tmpdir(), 'rocky-project-'));
});

const writeUser = (config: RockyConfig) => {
  writeFileSync(userPath, `${JSON.stringify(config, null, 2)}\n`, 'utf8');
};

const writeProject = (config: RockyConfig) => {
  writeFileSync(join(projectRoot, 'rocky.json'), `${JSON.stringify(config, null, 2)}\n`, 'utf8');
};

describe('validateConfig', () => {
  it('accepts an empty object', () => {
    expect(validateConfig({}, 'test')).toEqual({});
  });

  it('rejects non-object root', () => {
    expect(() => validateConfig(null, 'p')).toThrow(/must be a JSON object/);
    expect(() => validateConfig([], 'p')).toThrow(/must be a JSON object/);
    expect(() => validateConfig('str', 'p')).toThrow(/must be a JSON object/);
  });

  it('rejects unknown top-level keys', () => {
    // top-level 은 명시적 allowlist ($schema/worklog/todo) 로 좁혔다 — 오타 /
    // 제거된 도메인 키는 즉시 reject 되어야 한다.
    expect(() => validateConfig({ futureFeature: { foo: 'bar' } } as any, 'p')).toThrow(
      /unknown top-level key "futureFeature"/,
    );
    expect(() => validateConfig({ mysql: { connections: {} } } as any, 'p')).toThrow(
      /unknown top-level key "mysql"/,
    );
  });

  it('rejects the openapi / seo keys — v0.23 에서 도구와 함께 걷어냈다', () => {
    expect(() =>
      validateConfig({ openapi: { registry: { acme: { dev: { users: 'u' } } } } } as any, 'p'),
    ).toThrow(/unknown top-level key "openapi"/);
    expect(() => validateConfig({ seo: { timeoutMs: 5000 } } as any, 'p')).toThrow(
      /unknown top-level key "seo"/,
    );
  });

  it('rejects legacy top-level journal key', () => {
    expect(() => validateConfig({ journal: { dir: '/j' } } as any, 'test')).toThrow(
      /unknown top-level key "journal"/,
    );
  });

  it('rejects the opencode key — 위임 런타임과 함께 걷어냈다', () => {
    expect(() => validateConfig({ opencode: { model: 'x/y' } } as any, 'test')).toThrow(
      /unknown top-level key "opencode"/,
    );
  });
});

describe('validateConfig — worklog', () => {
  it('accepts a well-formed worklog block', () => {
    const config = { worklog: { dir: '~/notes/w' } };
    expect(validateConfig(config, 'test')).toEqual(config);
  });

  it('accepts an empty / omitted worklog block', () => {
    expect(() => validateConfig({ worklog: {} }, 'test')).not.toThrow();
  });

  it('rejects a non-object worklog', () => {
    expect(() => validateConfig({ worklog: 'nope' } as any, 'p')).toThrow(
      /worklog must be an object/,
    );
    expect(() => validateConfig({ worklog: [] } as any, 'p')).toThrow(/worklog must be an object/);
  });

  it('rejects unknown worklog keys', () => {
    expect(() => validateConfig({ worklog: { ttl: 10 } } as any, 'p')).toThrow(/unknown key "ttl"/);
  });

  it('rejects empty / non-string dir', () => {
    expect(() => validateConfig({ worklog: { dir: '' } } as any, 'p')).toThrow(
      /worklog.dir must be a non-empty string/,
    );
  });

  it('rejects worklog.wikiDir (removed key)', () => {
    expect(() => validateConfig({ worklog: { wikiDir: '/x' } } as any, 'p')).toThrow(
      /unknown key "wikiDir"/,
    );
  });

  it('accepts worklog.autoCapture / captureMaxChars / digestThreshold', () => {
    const cfg = validateConfig(
      { worklog: { dir: '/tmp/w', autoCapture: false, captureMaxChars: 500, digestThreshold: 10 } },
      'test',
    );
    expect(cfg.worklog?.autoCapture).toBe(false);
    expect(cfg.worklog?.captureMaxChars).toBe(500);
    expect(cfg.worklog?.digestThreshold).toBe(10);
  });

  it('rejects non-boolean worklog.autoCapture', () => {
    expect(() => validateConfig({ worklog: { autoCapture: 'yes' } } as any, 'p')).toThrow(
      /autoCapture must be a boolean/,
    );
  });

  it('rejects non-positive-integer captureMaxChars / digestThreshold', () => {
    expect(() => validateConfig({ worklog: { captureMaxChars: 0 } } as any, 'p')).toThrow(
      /captureMaxChars must be a positive integer/,
    );
    expect(() => validateConfig({ worklog: { captureMaxChars: 1.5 } } as any, 'p')).toThrow(
      /captureMaxChars must be a positive integer/,
    );
    expect(() => validateConfig({ worklog: { digestThreshold: -1 } } as any, 'p')).toThrow(
      /digestThreshold must be a positive integer/,
    );
  });
});

describe('validateConfig — todo (daemon key)', () => {
  it('rocky.json 의 todo 블록을 통과시킨다 — Rust 데몬이 읽는 키라 여기서 검증하지 않는다', () => {
    const config = { worklog: { dir: '/w' }, todo: { port: 8636, anything: true } };
    expect(() => validateConfig(config as any, 'test')).not.toThrow();
    expect(validateConfig(config as any, 'test').worklog?.dir).toBe('/w');
  });
});

describe('mergeConfigs', () => {
  it('project worklog fields override user worklog, field by field', () => {
    const user: RockyConfig = { worklog: { dir: '/u/w', autoCapture: true } };
    const project: RockyConfig = { worklog: { autoCapture: false } };
    const merged = mergeConfigs(user, project);
    expect(merged.worklog?.dir).toBe('/u/w');
    expect(merged.worklog?.autoCapture).toBe(false);
  });

  it('returns a deep clone — mutating the result does not touch input', () => {
    const user: RockyConfig = { worklog: { dir: '/u/w' } };
    const merged = mergeConfigs(user, {});
    merged.worklog!.dir = 'MUTATED';
    expect(user.worklog?.dir).toBe('/u/w');
  });
});

describe('loadConfig', () => {
  it('returns {} with no errors when neither file exists', async () => {
    const r = await loadConfig({ userPath, projectRoot });
    expect(r.config).toEqual({});
    expect(r.errors).toEqual([]);
  });

  it('loads user-only when project is absent', async () => {
    writeUser({ worklog: { dir: '/u/w' } });
    const r = await loadConfig({ userPath, projectRoot });
    expect(r.config.worklog?.dir).toBe('/u/w');
    expect(r.errors).toEqual([]);
  });

  it('loads project-only when user is absent', async () => {
    writeProject({ worklog: { dir: '/p/w' } });
    const r = await loadConfig({ userPath, projectRoot });
    expect(r.config.worklog?.dir).toBe('/p/w');
    expect(r.errors).toEqual([]);
  });

  it('merges with project taking precedence', async () => {
    writeUser({ worklog: { dir: '/u/w', captureMaxChars: 400 } });
    writeProject({ worklog: { dir: '/p/w' } });
    const r = await loadConfig({ userPath, projectRoot });
    expect(r.config.worklog?.dir).toBe('/p/w');
    expect(r.config.worklog?.captureMaxChars).toBe(400);
  });

  it('reports malformed JSON in errors[] without throwing', async () => {
    writeFileSync(userPath, '{ not json', 'utf8');
    const r = await loadConfig({ userPath, projectRoot });
    expect(r.errors.length).toBe(1);
    expect(r.errors[0]?.source).toBe(userPath);
    expect(r.errors[0]?.message).toMatch(/Failed to parse/);
    expect(r.config).toEqual({});
  });

  it('reports schema-violating config in errors[] without throwing', async () => {
    writeUser({ worklog: { dir: '' } } as any);
    const r = await loadConfig({ userPath, projectRoot });
    expect(r.errors.length).toBe(1);
    expect(r.errors[0]?.message).toMatch(/worklog.dir must be a non-empty string/);
  });

  it('preserves valid project config when user file is malformed', async () => {
    writeFileSync(userPath, '{ broken', 'utf8');
    writeProject({ worklog: { dir: '/p/w' } });
    const r = await loadConfig({ userPath, projectRoot });
    expect(r.errors.length).toBe(1);
    expect(r.errors[0]?.source).toBe(userPath);
    expect(r.config.worklog?.dir).toBe('/p/w');
  });

  it('preserves valid user config when project file is malformed', async () => {
    writeUser({ worklog: { dir: '/u/w' } });
    const projectFile = join(projectRoot, 'rocky.json');
    writeFileSync(projectFile, '{ also broken', 'utf8');
    const r = await loadConfig({ userPath, projectRoot });
    expect(r.errors.length).toBe(1);
    expect(r.errors[0]?.source).toBe(projectFile);
    expect(r.config.worklog?.dir).toBe('/u/w');
  });

  it('collects errors from both files when both are malformed', async () => {
    writeFileSync(userPath, '{ user broken', 'utf8');
    writeFileSync(join(projectRoot, 'rocky.json'), '{ project broken', 'utf8');
    const r = await loadConfig({ userPath, projectRoot });
    expect(r.errors.length).toBe(2);
    expect(r.config).toEqual({});
  });
});
