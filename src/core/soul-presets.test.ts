import { describe, it, expect } from 'bun:test';
import { listSouls, readSoul, resolveDefaultSoulDirs } from './soul';

describe('bundled preset souls', () => {
  const dirs = resolveDefaultSoulDirs();

  it('ships rocky / senior / terse presets', () => {
    const names = listSouls(dirs)
      .map((s) => s.name)
      .sort();
    expect(names).toEqual(expect.arrayContaining(['rocky', 'senior', 'terse']));
  });

  for (const name of ['rocky', 'senior', 'terse']) {
    it(`preset ${name} parses with a description and non-empty body`, () => {
      const soul = readSoul(name, dirs);
      expect(soul).not.toBeNull();
      expect(soul!.description.length).toBeGreaterThan(0);
      expect(soul!.body.trim().length).toBeGreaterThan(0);
      expect(soul!.source).toBe('preset');
    });
  }
});
