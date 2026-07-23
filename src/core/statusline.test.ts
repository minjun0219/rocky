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

  test('CRLF 라인 엔딩(설치본을 Windows 에디터로 수정한 경우)에서도 마커를 읽는다', () => {
    expect(parseTemplateName(template('mini', 'echo hi\n').replaceAll('\n', '\r\n'))).toBe('mini');
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

  test('내용이 같으면 up-to-date — 덮어쓰지 않되 유실된 실행 비트는 복구한다', () => {
    const { bundledDir, targetPath } = setup();
    writeFileSync(join(bundledDir, 'mini.sh'), template('mini', 'echo v1\n'));
    writeFileSync(targetPath, template('mini', 'echo v1\n'), { mode: 0o644 });
    expect(syncStatusline({ bundledDir, targetPath })).toBe('up-to-date');
    expect(statSync(targetPath).mode & 0o100).toBe(0o100);
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

describe('full.sh smoke', () => {
  // git 세그먼트는 실행 환경의 실제 repo 상태에 의존하므로 비-git 디렉터리(current_dir
  // 가 존재하지 않는 경로)로 고정해 결정론을 확보한다 — git 세그먼트는 수동 검증 항목.
  function runFull(input: unknown): string[] {
    const proc = Bun.spawnSync(['sh', join(import.meta.dir, '../../statusline/full.sh')], {
      stdin: Buffer.from(JSON.stringify(input)),
    });
    return proc.stdout.toString().trimEnd().split('\n');
  }

  const DIM = '[38;5;245m';
  const WARN = '[33m';
  const DANGER = '[1;31m';

  const base = {
    model: { display_name: 'Opus' },
    workspace: { current_dir: '/rocky-statusline-test-no-such-dir' },
  };

  test('위험 구간 — 3줄 출력, ctx/left 빨강, 시간당 비용 표시', () => {
    const lines = runFull({
      ...base,
      context_window: { used_percentage: 95 },
      rate_limits: { five_hour: { used_percentage: 95 } },
      cost: {
        total_cost_usd: 4.2,
        total_duration_ms: 3_600_000,
        total_lines_added: 10,
        total_lines_removed: 3,
      },
    });
    expect(lines).toHaveLength(3);
    expect(lines[1]).toContain(DANGER);
    expect(lines[2]).toContain('/h)');
  });

  test('안전 구간 — ctx/left 는 둔한 회색, 경고/위험색 없음', () => {
    const lines = runFull({
      ...base,
      context_window: { used_percentage: 30 },
      rate_limits: { five_hour: { used_percentage: 20 } },
    });
    expect(lines[1]).toContain(DIM);
    expect(lines[1]).not.toContain(WARN);
    expect(lines[1]).not.toContain(DANGER);
  });

  test('경고 구간 — ctx 70%+ / left 30%- 는 노랑', () => {
    const lines = runFull({
      ...base,
      context_window: { used_percentage: 75 },
      rate_limits: { five_hour: { used_percentage: 75 } },
    });
    expect(lines[1]).toContain(WARN);
    expect(lines[1]).not.toContain(DANGER);
  });

  test('경과 5분 미만 — 비용은 있어도 시간당 비용은 생략', () => {
    const lines = runFull({
      ...base,
      cost: { total_cost_usd: 0.5, total_duration_ms: 60_000 },
    });
    expect(lines[2]).toContain('$0.50');
    expect(lines[2]).not.toContain('/h)');
  });

  test('최소 입력 — line 3 세그먼트가 전무하면 2줄만 출력', () => {
    expect(runFull(base)).toHaveLength(2);
  });
});
