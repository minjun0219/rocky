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

  it('allows unknown top-level keys for forward compatibility', () => {
    // 도메인 재추가 시 같은 파일 (`mysql` / `spec` / `github` 등) 이 future-key 자리에
    // 다시 들어올 수 있어야 한다 — top-level 미지원 키는 통과시킨다.
    expect(() => validateConfig({ futureFeature: { foo: 'bar' } } as any, 'p')).not.toThrow();
    expect(() => validateConfig({ mysql: { connections: {} } } as any, 'p')).not.toThrow();
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

describe('validateConfig — journal', () => {
  it('accepts a well-formed journal block', () => {
    const config = { journal: { dir: '~/notes/j', wikiDir: '~/Obsidian/v' } };
    expect(validateConfig(config, 'test')).toEqual(config);
  });

  it('accepts an empty / omitted journal block', () => {
    expect(() => validateConfig({ journal: {} }, 'test')).not.toThrow();
  });

  it('rejects a non-object journal', () => {
    expect(() => validateConfig({ journal: 'nope' } as any, 'p')).toThrow(
      /journal must be an object/,
    );
    expect(() => validateConfig({ journal: [] } as any, 'p')).toThrow(/journal must be an object/);
  });

  it('rejects unknown journal keys', () => {
    expect(() => validateConfig({ journal: { ttl: 10 } } as any, 'p')).toThrow(/unknown key "ttl"/);
  });

  it('rejects empty / non-string dir / wikiDir', () => {
    expect(() => validateConfig({ journal: { dir: '' } } as any, 'p')).toThrow(
      /journal.dir must be a non-empty string/,
    );
    expect(() => validateConfig({ journal: { wikiDir: 42 } } as any, 'p')).toThrow(
      /journal.wikiDir must be a non-empty string/,
    );
  });
});

describe('mergeConfigs — journal', () => {
  it('project journal fields override user journal, field by field', () => {
    const user: RockyConfig = { journal: { dir: '/u/j', wikiDir: '/u/v' } };
    const project: RockyConfig = { journal: { wikiDir: '/p/v' } };
    const merged = mergeConfigs(user, project);
    expect(merged.journal?.dir).toBe('/u/j');
    expect(merged.journal?.wikiDir).toBe('/p/v');
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
