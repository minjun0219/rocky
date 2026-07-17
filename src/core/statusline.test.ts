import { describe, expect, test } from 'bun:test';
import { mkdirSync, mkdtempSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  DEFAULT_TEMPLATE,
  bundledStatuslineDir,
  defaultStatuslinePath,
  parseTemplateName,
  syncStatusline,
} from './statusline';

function setup(): { bundledDir: string; targetPath: string } {
  const dir = mkdtempSync(join(tmpdir(), 'rocky-statusline-'));
  const bundledDir = join(dir, 'statusline');
  mkdirSync(bundledDir);
  return { bundledDir, targetPath: join(dir, 'installed.sh') };
}

function template(name: string, body: string): string {
  return `#!/bin/sh\n# rocky-statusline-template: ${name}\n${body}`;
}

describe('parseTemplateName', () => {
  test('헤더 마커에서 템플릿 이름을 읽는다', () => {
    expect(parseTemplateName(template('mini', 'echo hi\n'))).toBe('mini');
  });

  test('마커가 없거나 이름이 규칙에 어긋나면 null', () => {
    expect(parseTemplateName('#!/bin/sh\necho hi\n')).toBeNull();
    expect(parseTemplateName('# rocky-statusline-template: ../evil\n')).toBeNull();
  });
});

describe('syncStatusline', () => {
  test('target 이 없으면 not-installed — 설치 전 사용자에게 아무것도 만들지 않는다', () => {
    const { bundledDir, targetPath } = setup();
    writeFileSync(join(bundledDir, 'duo.sh'), template('duo', 'echo v1\n'));
    expect(syncStatusline({ bundledDir, targetPath })).toBe('not-installed');
  });

  test('설치본의 마커와 같은 템플릿에서 동기화한다 — 내용이 다르면 updated + 실행 권한', () => {
    const { bundledDir, targetPath } = setup();
    writeFileSync(join(bundledDir, 'duo.sh'), template('duo', 'echo duo-v2\n'));
    writeFileSync(join(bundledDir, 'mini.sh'), template('mini', 'echo mini-v2\n'));
    writeFileSync(targetPath, template('mini', 'echo mini-v1\n'));
    expect(syncStatusline({ bundledDir, targetPath })).toBe('updated');
    expect(readFileSync(targetPath, 'utf8')).toBe(template('mini', 'echo mini-v2\n'));
    expect(statSync(targetPath).mode & 0o100).toBe(0o100);
  });

  test('내용이 같으면 up-to-date, no-op', () => {
    const { bundledDir, targetPath } = setup();
    writeFileSync(join(bundledDir, 'mini.sh'), template('mini', 'echo v1\n'));
    writeFileSync(targetPath, template('mini', 'echo v1\n'));
    expect(syncStatusline({ bundledDir, targetPath })).toBe('up-to-date');
  });

  test('마커 없는 설치본은 기본 템플릿으로 동기화한다', () => {
    const { bundledDir, targetPath } = setup();
    writeFileSync(
      join(bundledDir, `${DEFAULT_TEMPLATE}.sh`),
      template(DEFAULT_TEMPLATE, 'echo v2\n'),
    );
    writeFileSync(targetPath, '#!/bin/sh\necho legacy\n');
    expect(syncStatusline({ bundledDir, targetPath })).toBe('updated');
    expect(readFileSync(targetPath, 'utf8')).toBe(template(DEFAULT_TEMPLATE, 'echo v2\n'));
  });

  test('설치본이 가리키는 템플릿이 번들에 없으면 컨텍스트를 담아 던진다', () => {
    const { bundledDir, targetPath } = setup();
    writeFileSync(targetPath, template('ghost', 'echo v1\n'));
    expect(() => syncStatusline({ bundledDir, targetPath })).toThrow(/ghost/);
  });
});

describe('paths', () => {
  test('defaultStatuslinePath 는 ~/.config/rocky/statusline.sh', () => {
    expect(defaultStatuslinePath()).toMatch(/\.config\/rocky\/statusline\.sh$/);
  });

  test('bundledStatuslineDir 는 <root>/statusline', () => {
    expect(bundledStatuslineDir('/plugin/root')).toBe('/plugin/root/statusline');
  });
});
