import { describe, expect, it } from 'bun:test';
import { diffMarkdownBySection, splitMarkdownSections } from './notion-diff';

describe('splitMarkdownSections', () => {
  it('splits markdown by heading path', () => {
    const sections = splitMarkdownSections('# A\n\none\n\n## B\n\ntwo');
    expect(sections.map((section) => section.path)).toEqual(['A', 'A > B']);
  });

  it('does not treat headings inside fenced code blocks as sections', () => {
    const sections = splitMarkdownSections(
      '# A\n\n````md\n``` nested text\n# not a heading\n````\n\n## B\n\ntwo',
    );
    expect(sections.map((section) => section.path)).toEqual(['A', 'A > B']);
    expect(sections[0]?.content).toContain('# not a heading');
  });

  it('uses content hash suffixes for duplicate heading paths', () => {
    const sections = splitMarkdownSections('# FAQ\n\none\n\n# FAQ\n\ntwo');
    expect(sections).toHaveLength(2);
    expect(sections[0]?.path).toMatch(/^FAQ \[[a-f0-9]{8}\]$/);
    expect(sections[1]?.path).toMatch(/^FAQ \[[a-f0-9]{8}\]$/);
    expect(sections[0]?.path).not.toEqual(sections[1]?.path);
  });
});

describe('diffMarkdownBySection', () => {
  it('returns changed heading sections with compact previews', () => {
    const before = '# 기획서\n\nintro\n\n## API\n\n- GET /orders\n\n## TODO\n\n- [ ] old';
    const after =
      '# 기획서\n\nintro changed\n\n## API\n\n- GET /orders\n- POST /orders\n\n## 새 섹션\n\n추가됨';

    const diff = diffMarkdownBySection(before, after);

    expect(diff.changed).toBe(true);
    expect(diff.sections.map((section) => [section.path, section.status])).toEqual([
      ['기획서', 'modified'],
      ['기획서 > API', 'modified'],
      ['기획서 > TODO', 'removed'],
      ['기획서 > 새 섹션', 'added'],
    ]);
    expect(diff.sections[1]?.preview).toContain('+ - POST /orders');
  });

  it('returns unchanged when content hashes match', () => {
    const diff = diffMarkdownBySection('# A\n\none', '# A\n\none');
    expect(diff.changed).toBe(false);
    expect(diff.sections).toEqual([]);
  });

  it('keeps unchanged duplicate heading sections stable when another duplicate is inserted', () => {
    const before = '# FAQ\n\none\n\n# FAQ\n\ntwo';
    const after = '# FAQ\n\nnew\n\n# FAQ\n\none\n\n# FAQ\n\ntwo';

    const diff = diffMarkdownBySection(before, after);

    expect(diff.sections).toHaveLength(1);
    expect(diff.sections[0]?.status).toBe('added');
    expect(diff.sections[0]?.preview).toContain('+ new');
  });

  it('returns changed sections in document order', () => {
    const before = '# A\n\none\n\n# C\n\nthree';
    const after = '# A\n\none\n\n# B\n\ntwo\n\n# C\n\nthree changed';

    const diff = diffMarkdownBySection(before, after);

    expect(diff.sections.map((section) => section.path)).toEqual(['B', 'C']);
  });

  it('skips expensive line previews for very large sections', () => {
    const before = `# A\n\n${Array.from({ length: 150 }, (_, i) => `old ${i}`).join('\n')}`;
    const after = `# A\n\n${Array.from({ length: 150 }, (_, i) => `new ${i}`).join('\n')}`;

    const diff = diffMarkdownBySection(before, after);

    expect(diff.sections[0]?.preview).toContain('Diff preview skipped');
  });
});
