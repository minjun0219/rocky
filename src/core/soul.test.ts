import { describe, it, expect, beforeEach } from 'bun:test';
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { parseFrontmatter, resolveSoulName, listSouls, readSoul, buildSoulContext } from './soul';

let presetDir: string;
let customDir: string;

const soulFile = (name: string, description: string, body: string) =>
  `---\nname: ${name}\ndescription: ${description}\n---\n\n${body}\n`;

beforeEach(() => {
  const root = mkdtempSync(join(tmpdir(), 'rocky-soul-'));
  presetDir = join(root, 'presets');
  customDir = join(root, 'custom');
  mkdirSync(presetDir, { recursive: true });
  mkdirSync(customDir, { recursive: true });
  writeFileSync(
    join(presetDir, 'rocky.md'),
    soulFile('rocky', '헤일메리 로키', '따뜻한 동료 본문'),
  );
  writeFileSync(join(presetDir, 'terse.md'), soulFile('terse', '간결', '답부터 본문'));
});

const dirs = () => ({ presetDir, customDir });

describe('parseFrontmatter', () => {
  it('extracts name/description and body', () => {
    const parsed = parseFrontmatter('---\nname: x\ndescription: y\n---\n\nbody text\n');
    expect(parsed.name).toBe('x');
    expect(parsed.description).toBe('y');
    expect(parsed.body.trim()).toBe('body text');
  });

  it('returns whole input as body when no frontmatter', () => {
    const parsed = parseFrontmatter('just body\n');
    expect(parsed.name).toBeUndefined();
    expect(parsed.body.trim()).toBe('just body');
  });
});

describe('resolveSoulName', () => {
  it('returns the configured soul', () => {
    expect(resolveSoulName({ soul: 'rocky' })).toBe('rocky');
  });
  it('returns undefined when unset', () => {
    expect(resolveSoulName({})).toBeUndefined();
  });
});

describe('listSouls', () => {
  it('lists preset souls with source tag', () => {
    const souls = listSouls(dirs());
    const rocky = souls.find((s) => s.name === 'rocky');
    expect(rocky?.source).toBe('preset');
    expect(rocky?.description).toBe('헤일메리 로키');
  });

  it('custom overrides a preset of the same name', () => {
    writeFileSync(join(customDir, 'rocky.md'), soulFile('rocky', '내 로키', '커스텀 본문'));
    const souls = listSouls(dirs());
    const rocky = souls.filter((s) => s.name === 'rocky');
    expect(rocky).toHaveLength(1);
    expect(rocky[0]!.source).toBe('custom');
    expect(rocky[0]!.description).toBe('내 로키');
  });
});

describe('readSoul', () => {
  it('reads a preset soul body', () => {
    const soul = readSoul('rocky', dirs());
    expect(soul?.body.trim()).toBe('따뜻한 동료 본문');
    expect(soul?.source).toBe('preset');
  });

  it('prefers a custom soul over a preset', () => {
    writeFileSync(join(customDir, 'rocky.md'), soulFile('rocky', '내 로키', '커스텀 본문'));
    const soul = readSoul('rocky', dirs());
    expect(soul?.body.trim()).toBe('커스텀 본문');
    expect(soul?.source).toBe('custom');
  });

  it('returns null for an unknown soul', () => {
    expect(readSoul('nope', dirs())).toBeNull();
  });

  it('identity is the filename stem, not the frontmatter name', () => {
    writeFileSync(join(presetDir, 'foo.md'), soulFile('bar', '이름 불일치', '본문'));

    const souls = listSouls(dirs());
    expect(souls.find((s) => s.name === 'foo')).toBeDefined();
    expect(souls.find((s) => s.name === 'bar')).toBeUndefined();

    expect(readSoul('foo', dirs())).not.toBeNull();
    expect(readSoul('bar', dirs())).toBeNull();
  });

  it('falls back to preset when the custom file exists but is unreadable', () => {
    // customDir 안에 rocky.md 라는 "디렉터리" 를 만들어 existsSync 는 true, readFileSync 는
    // EISDIR 로 던지는 상황을 재현한다.
    mkdirSync(join(customDir, 'rocky.md'));

    const soul = readSoul('rocky', dirs());
    expect(soul).not.toBeNull();
    expect(soul?.source).toBe('preset');
    expect(soul?.body.trim()).toBe('따뜻한 동료 본문');
  });
});

describe('buildSoulContext', () => {
  it('wraps body with a precedence preamble', () => {
    const soul = readSoul('rocky', dirs())!;
    const ctx = buildSoulContext(soul);
    expect(ctx).toContain('따뜻한 동료 본문');
    expect(ctx).toContain('AGENTS.md');
    expect(ctx).toContain('rocky');
  });

  it('appends a callsign line when opts.callsign is set', () => {
    const soul = readSoul('rocky', dirs())!;
    const ctx = buildSoulContext(soul, { callsign: '민준' });
    expect(ctx).toContain('사용자 호칭');
    expect(ctx).toContain('"민준"');
    // 호칭 라인은 본문 뒤에 온다.
    expect(ctx.indexOf('사용자 호칭')).toBeGreaterThan(ctx.indexOf('따뜻한 동료 본문'));
  });

  it('escapes quotes in the callsign via JSON.stringify', () => {
    const soul = readSoul('rocky', dirs())!;
    const ctx = buildSoulContext(soul, { callsign: '민"준' });
    expect(ctx).toContain(JSON.stringify('민"준'));
  });

  it('omits the callsign line when callsign is missing or blank', () => {
    const soul = readSoul('rocky', dirs())!;
    expect(buildSoulContext(soul)).not.toContain('사용자 호칭');
    expect(buildSoulContext(soul, {})).not.toContain('사용자 호칭');
    expect(buildSoulContext(soul, { callsign: '   ' })).not.toContain('사용자 호칭');
  });
});
