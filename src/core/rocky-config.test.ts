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

  it('accepts a registry with valid identifiers', () => {
    const config: RockyConfig = {
      openapi: {
        registry: {
          acme: { dev: { users: 'https://example.com/users.json' } },
        },
      },
    };
    expect(validateConfig(config, 'test')).toEqual(config);
  });

  it('rejects non-object root', () => {
    expect(() => validateConfig(null, 'p')).toThrow(/must be a JSON object/);
    expect(() => validateConfig([], 'p')).toThrow(/must be a JSON object/);
    expect(() => validateConfig('str', 'p')).toThrow(/must be a JSON object/);
  });

  it('rejects host name with colon', () => {
    expect(() =>
      validateConfig({ openapi: { registry: { 'ac:me': { dev: { users: 'u' } } } } }, 'p'),
    ).toThrow(/host name/);
  });

  it('rejects env name with whitespace', () => {
    expect(() =>
      validateConfig({ openapi: { registry: { acme: { 'de v': { users: 'u' } } } } }, 'p'),
    ).toThrow(/env name/);
  });

  it('rejects empty / whitespace-only URL', () => {
    expect(() =>
      validateConfig({ openapi: { registry: { acme: { dev: { users: '' } } } } }, 'p'),
    ).toThrow(/non-empty URL/);
    expect(() =>
      validateConfig({ openapi: { registry: { acme: { dev: { users: '   ' } } } } }, 'p'),
    ).toThrow(/non-empty URL/);
  });

  it('rejects non-string URL', () => {
    expect(() =>
      validateConfig({ openapi: { registry: { acme: { dev: { users: 42 } } } } }, 'p'),
    ).toThrow(/non-empty URL/);
  });

  it('rejects unparseable URL string', () => {
    expect(() =>
      validateConfig(
        {
          openapi: { registry: { acme: { dev: { users: 'not a url' } } } },
        },
        'p',
      ),
    ).toThrow(/not a valid URL/);
  });

  it('rejects unsupported URL scheme', () => {
    expect(() =>
      validateConfig(
        {
          openapi: {
            registry: {
              acme: { dev: { users: 'ftp://example.com/spec.json' } },
            },
          },
        },
        'p',
      ),
    ).toThrow(/unsupported scheme/);
  });

  it('accepts http / https / file URLs', () => {
    for (const url of [
      'http://example.com/spec.json',
      'https://example.com/spec.json',
      'file:///tmp/spec.json',
    ]) {
      expect(() =>
        validateConfig({ openapi: { registry: { acme: { dev: { users: url } } } } }, 'p'),
      ).not.toThrow();
    }
  });

  it('rejects unknown top-level keys', () => {
    // top-level 은 명시적 allowlist ($schema/openapi/seo/worklog) 로 좁혔다 — 오타 /
    // 제거된 도메인 키(mysql 등)는 즉시 reject 되어야 한다.
    expect(() => validateConfig({ futureFeature: { foo: 'bar' } } as any, 'p')).toThrow(
      /unknown top-level key "futureFeature"/,
    );
    expect(() => validateConfig({ mysql: { connections: {} } } as any, 'p')).toThrow(
      /unknown top-level key "mysql"/,
    );
  });

  it('rejects legacy top-level journal key', () => {
    expect(() => validateConfig({ journal: { dir: '/j' } } as any, 'test')).toThrow(
      /unknown top-level key "journal"/,
    );
  });
});

describe('mergeConfigs', () => {
  it('project overrides user at the leaf', () => {
    const user: RockyConfig = {
      openapi: {
        registry: {
          acme: {
            dev: {
              users: 'https://user/u.json',
              orders: 'https://user/o.json',
            },
          },
        },
      },
    };
    const project: RockyConfig = {
      openapi: {
        registry: {
          acme: { dev: { users: 'https://project/u.json' } },
        },
      },
    };
    const merged = mergeConfigs(user, project);
    expect(merged.openapi?.registry?.acme?.dev?.users).toBe('https://project/u.json');
    // user-only spec survives.
    expect(merged.openapi?.registry?.acme?.dev?.orders).toBe('https://user/o.json');
  });

  it('project can introduce new host / env / spec', () => {
    const user: RockyConfig = {
      openapi: {
        registry: { acme: { dev: { users: 'https://u.example/u.json' } } },
      },
    };
    const project: RockyConfig = {
      openapi: {
        registry: {
          acme: { prod: { users: 'https://p.example/u.json' } },
          beta: { dev: { svc: 'https://b.example/svc.json' } },
        },
      },
    };
    const merged = mergeConfigs(user, project);
    expect(merged.openapi?.registry?.acme?.prod?.users).toBe('https://p.example/u.json');
    expect(merged.openapi?.registry?.beta?.dev?.svc).toBe('https://b.example/svc.json');
    expect(merged.openapi?.registry?.acme?.dev?.users).toBe('https://u.example/u.json');
  });

  it('returns a deep clone — mutating the result does not touch input', () => {
    const user: RockyConfig = {
      openapi: { registry: { acme: { dev: { users: 'https://u/u.json' } } } },
    };
    const merged = mergeConfigs(user, {});
    merged.openapi!.registry!.acme!.dev!.users = 'MUTATED';
    expect(user.openapi?.registry?.acme?.dev?.users).toBe('https://u/u.json');
  });

  it('project seo fields override user seo, field by field', () => {
    const user: RockyConfig = { seo: { allowPrivateHosts: true, timeoutMs: 5000 } };
    const project: RockyConfig = { seo: { timeoutMs: 9000 } };
    const merged = mergeConfigs(user, project);
    // timeoutMs 는 project 값, allowPrivateHosts 는 user 값 유지.
    expect(merged.seo?.timeoutMs).toBe(9000);
    expect(merged.seo?.allowPrivateHosts).toBe(true);
  });
});

describe('validateConfig — seo', () => {
  it('accepts a well-formed seo block', () => {
    const config = { seo: { allowPrivateHosts: true, timeoutMs: 8000 } };
    expect(validateConfig(config, 'test')).toEqual(config);
  });

  it('accepts an empty / omitted seo block', () => {
    expect(() => validateConfig({ seo: {} }, 'test')).not.toThrow();
  });

  it('rejects a non-object seo', () => {
    expect(() => validateConfig({ seo: 'nope' } as any, 'p')).toThrow(/seo must be an object/);
    expect(() => validateConfig({ seo: [] } as any, 'p')).toThrow(/seo must be an object/);
  });

  it('rejects unknown seo keys', () => {
    expect(() => validateConfig({ seo: { retries: 3 } } as any, 'p')).toThrow(
      /unknown key "retries"/,
    );
  });

  it('rejects a non-boolean allowPrivateHosts', () => {
    expect(() => validateConfig({ seo: { allowPrivateHosts: 'yes' } } as any, 'p')).toThrow(
      /seo.allowPrivateHosts must be a boolean/,
    );
  });

  it('rejects out-of-range / non-integer timeoutMs', () => {
    expect(() => validateConfig({ seo: { timeoutMs: 0 } } as any, 'p')).toThrow(
      /between 1 and 30000/,
    );
    expect(() => validateConfig({ seo: { timeoutMs: 30001 } } as any, 'p')).toThrow(
      /between 1 and 30000/,
    );
    expect(() => validateConfig({ seo: { timeoutMs: 12.5 } } as any, 'p')).toThrow(
      /between 1 and 30000/,
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

describe('mergeConfigs — worklog', () => {
  it('project worklog fields override user worklog, field by field', () => {
    const user: RockyConfig = { worklog: { dir: '/u/w', autoCapture: true } };
    const project: RockyConfig = { worklog: { autoCapture: false } };
    const merged = mergeConfigs(user, project);
    expect(merged.worklog?.dir).toBe('/u/w');
    expect(merged.worklog?.autoCapture).toBe(false);
  });
});

describe('loadConfig', () => {
  it('returns {} with no errors when neither file exists', async () => {
    const r = await loadConfig({ userPath, projectRoot });
    expect(r.config).toEqual({});
    expect(r.errors).toEqual([]);
  });

  it('loads user-only when project is absent', async () => {
    writeUser({
      openapi: { registry: { acme: { dev: { users: 'https://u/u.json' } } } },
    });
    const r = await loadConfig({ userPath, projectRoot });
    expect(r.config.openapi?.registry?.acme?.dev?.users).toBe('https://u/u.json');
    expect(r.errors).toEqual([]);
  });

  it('loads project-only when user is absent', async () => {
    writeProject({
      openapi: { registry: { acme: { prod: { users: 'https://p/u.json' } } } },
    });
    const r = await loadConfig({ userPath, projectRoot });
    expect(r.config.openapi?.registry?.acme?.prod?.users).toBe('https://p/u.json');
    expect(r.errors).toEqual([]);
  });

  it('merges with project taking precedence', async () => {
    writeUser({
      openapi: {
        registry: { acme: { dev: { users: 'https://user/u.json' } } },
      },
    });
    writeProject({
      openapi: {
        registry: { acme: { dev: { users: 'https://project/u.json' } } },
      },
    });
    const r = await loadConfig({ userPath, projectRoot });
    expect(r.config.openapi?.registry?.acme?.dev?.users).toBe('https://project/u.json');
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
    writeUser({
      openapi: { registry: { 'bad:host': { dev: { users: 'u' } } } } as any,
    });
    const r = await loadConfig({ userPath, projectRoot });
    expect(r.errors.length).toBe(1);
    expect(r.errors[0]?.message).toMatch(/host name/);
  });

  it('preserves valid project config when user file is malformed', async () => {
    writeFileSync(userPath, '{ broken', 'utf8');
    writeProject({
      openapi: {
        registry: { acme: { prod: { users: 'https://api.acme/u.json' } } },
      },
    });
    const r = await loadConfig({ userPath, projectRoot });
    expect(r.errors.length).toBe(1);
    expect(r.errors[0]?.source).toBe(userPath);
    expect(r.config.openapi?.registry?.acme?.prod?.users).toBe('https://api.acme/u.json');
  });

  it('preserves valid user config when project file is malformed', async () => {
    writeUser({
      openapi: {
        registry: { acme: { dev: { users: 'https://dev.acme/u.json' } } },
      },
    });
    const projectFile = join(projectRoot, 'rocky.json');
    writeFileSync(projectFile, '{ also broken', 'utf8');
    const r = await loadConfig({ userPath, projectRoot });
    expect(r.errors.length).toBe(1);
    expect(r.errors[0]?.source).toBe(projectFile);
    expect(r.config.openapi?.registry?.acme?.dev?.users).toBe('https://dev.acme/u.json');
  });

  it('collects errors from both files when both are malformed', async () => {
    writeFileSync(userPath, '{ user broken', 'utf8');
    writeFileSync(join(projectRoot, 'rocky.json'), '{ project broken', 'utf8');
    const r = await loadConfig({ userPath, projectRoot });
    expect(r.errors.length).toBe(2);
    expect(r.config).toEqual({});
  });
});
