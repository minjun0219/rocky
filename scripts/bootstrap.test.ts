import { afterEach, beforeEach, describe, expect, setDefaultTimeout, test } from 'bun:test';
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  readlinkSync,
  renameSync,
  rmSync,
  statSync,
  symlinkSync,
  utimesSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

/**
 * `bin/rocky`(셸 부트스트랩) 스모크 — 다운로드 없이 검증할 수 있는 경로만 본다.
 * 실제 tarball 을 받는 경로는 GitHub 에 닿아야 해서 테스트에 넣지 않는다 — 대신
 * 존재하지 않는 미러(`ROCKY_RELEASE_BASE`)와 격리된 `XDG_DATA_HOME` 으로
 * "받아야 하는데 못 받는" 상황을 만든다.
 */
const bin = join(import.meta.dir, '..', 'plugin', 'bin', 'rocky');

// macOS 는 새로 쓴 실행 파일의 첫 실행마다 검사를 거친다 — 평소 100~300ms 인데, 옆에서 다른
// 세션의 `cargo test`(doctest 마다 새 바이너리)가 돌면 그 줄이 밀려 한 번에 30초~2분까지 걸렸다.
// 검사는 파일(inode) 단위이고 결과가 프로세스를 넘어 남으므로, 가짜 실행 파일은 실행 사이에도
// 남는 `fixtures` 에 내용마다 한 번만 쓰고 테스트에는 심볼릭 링크로만 건다 — 이미 검사된 파일을
// 링크로 띄우면 부하 중에도 10ms 안팎이다. 시간 제한은 그 파일들을 처음 만드는 실행 몫이다.
setDefaultTimeout(30_000);

const fixtures = join(tmpdir(), 'rocky-bootstrap-fixtures');

/** 내용이 `body` 인 실행 파일 — 내용의 해시를 이름으로 삼아 실행이 바뀌어도 같은 파일을 쓴다. */
function sharedExec(body: string): string {
  const path = join(fixtures, Bun.hash(body).toString(16));
  if (!existsSync(path)) {
    mkdirSync(fixtures, { recursive: true });
    // 동시에 도는 다른 실행(데몬 검증과 세션 게이트)이 반쯤 쓴 파일을 띄우지 않게 rename 으로 건다.
    const tmp = `${path}.${process.pid}`;
    writeFileSync(tmp, body);
    chmodSync(tmp, 0o755);
    renameSync(tmp, path);
  }
  return path;
}

/** `path` 에 내용이 `body` 인 가짜 실행 파일을 건다 — 새 파일 대신 `sharedExec` 로의 링크. */
function fakeExec(path: string, body: string): void {
  symlinkSync(sharedExec(body), path);
}

let dir: string;
beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'rocky-bootstrap-'));
});
afterEach(() => {
  rmSync(dir, { recursive: true, force: true });
});

function run(args: string[], env: Record<string, string>) {
  const proc = Bun.spawnSync({
    cmd: [bin, ...args],
    stdout: 'pipe',
    stderr: 'pipe',
    env: {
      PATH: process.env.PATH ?? '',
      HOME: dir,
      XDG_DATA_HOME: join(dir, 'data'),
      // 진짜 릴리스에 닿지 않게 — 아무도 안 듣는 루프백 포트
      ROCKY_RELEASE_BASE: 'http://127.0.0.1:9/none',
      ...env,
    },
  });
  return { code: proc.exitCode, out: proc.stdout.toString(), err: proc.stderr.toString() };
}

/** `run` 의 비동기판 — 같은 프로세스의 `Bun.serve` 가 응답해야 하는 테스트는 spawnSync 로
 * 이벤트 루프를 막으면 안 된다. */
async function runAsync(args: string[], env: Record<string, string>) {
  const proc = Bun.spawn({
    cmd: [bin, ...args],
    stdout: 'pipe',
    stderr: 'pipe',
    env: {
      PATH: process.env.PATH ?? '',
      HOME: dir,
      XDG_DATA_HOME: join(dir, 'data'),
      ROCKY_RELEASE_BASE: 'http://127.0.0.1:9/none',
      ...env,
    },
  });
  const [out, err, code] = await Promise.all([
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
    proc.exited,
  ]);
  return { code, out, err };
}

/**
 * 플랫폼 검사를 통과시키는 가짜 `uname` — 부트스트랩은 `uname -s`/`-m` 으로 Apple Silicon 만
 * 받으므로, CI 의 ubuntu 에서는 다운로드 경로(백그라운드 잡·tarball)가 그 전에 죽어 마커
 * 단정이 레이스가 된다. PATH 앞에 이걸 두면 어느 러너에서든 같은 경로를 탄다.
 */
function darwinArm64Path(): string {
  const bin = join(dir, 'fake-uname-bin');
  if (!existsSync(bin)) {
    mkdirSync(bin);
    fakeExec(
      join(bin, 'uname'),
      '#!/bin/sh\ncase "$1" in -m) echo arm64 ;; *) echo Darwin ;; esac\n',
    );
  }
  return `${bin}:${process.env.PATH ?? ''}`;
}

function fakeBinary(): string {
  const path = join(dir, 'fake-rocky');
  fakeExec(path, '#!/bin/sh\necho "fake:$*"\nexit 7\n');
  return path;
}

describe('bin/rocky bootstrap', () => {
  test('ROCKY_BIN 이 있으면 그 바이너리로 인자 그대로 exec 한다', () => {
    const r = run(['hook', 'notify-todo', '--x'], { ROCKY_BIN: fakeBinary() });
    expect(r.out).toBe('fake:hook notify-todo --x\n');
    expect(r.code).toBe(7);
  });

  test('설치된 버전 디렉터리가 있으면 다운로드 없이 그것을 실행한다', () => {
    const version = '9.9.9-test.0';
    const root = join(dir, 'plugin');
    mkdirSync(join(root, '.claude-plugin'), { recursive: true });
    writeFileSync(
      join(root, '.claude-plugin', 'plugin.json'),
      JSON.stringify({ name: 'rocky', version, description: 'version 단어 함정' }),
    );
    const installed = join(dir, 'data', 'rocky', `v${version}`);
    mkdirSync(installed, { recursive: true });
    fakeExec(join(installed, 'rocky'), '#!/bin/sh\necho "installed:$*"\n');

    const r = run(['ls'], { CLAUDE_PLUGIN_ROOT: root });
    expect(r.err).toBe('');
    expect(r.out).toBe('installed:ls\n');
    expect(r.code).toBe(0);
  });

  describe('current 링크', () => {
    function pluginRoot(version: string): string {
      const root = join(dir, `plugin-${version}`);
      mkdirSync(join(root, '.claude-plugin'), { recursive: true });
      writeFileSync(join(root, '.claude-plugin', 'plugin.json'), JSON.stringify({ version }));
      return root;
    }
    function installed(version: string): string {
      const root = pluginRoot(version);
      const installDir = join(dir, 'data', 'rocky', `v${version}`);
      mkdirSync(installDir, { recursive: true });
      fakeExec(join(installDir, 'rocky'), `#!/bin/sh\necho "${version}:$*"\n`);
      return root;
    }
    const link = () => join(dir, 'data', 'rocky', 'current');

    test('SessionStart 가 current 를 자기 버전으로 걸고, 링크 너머 바이너리가 실행된다', () => {
      const r = run(['hook', 'ensure-daemon'], { CLAUDE_PLUGIN_ROOT: installed('1.0.0') });
      expect(r.code).toBe(0);
      expect(readlinkSync(link())).toBe('v1.0.0');
      const via = Bun.spawnSync({ cmd: [join(link(), 'rocky'), 'mcp', 'worklog'] });
      expect(via.stdout.toString()).toBe('1.0.0:mcp worklog\n');
    });

    test('다음 버전의 SessionStart 가 링크를 옮기고, 임시 링크를 남기지 않는다', () => {
      run(['hook', 'ensure-daemon'], { CLAUDE_PLUGIN_ROOT: installed('1.0.0') });
      run(['hook', 'ensure-daemon'], { CLAUDE_PLUGIN_ROOT: installed('1.1.0') });
      expect(readlinkSync(link())).toBe('v1.1.0');
      expect(readdirSync(join(dir, 'data', 'rocky')).sort()).toEqual([
        'current',
        'v1.0.0',
        'v1.1.0',
      ]);
    });

    test('SessionStart 가 ~/.local/bin/rocky 를 current/rocky 로 걸고, 그걸로 실행된다', () => {
      run(['hook', 'ensure-daemon'], { CLAUDE_PLUGIN_ROOT: installed('1.0.0') });
      const cli = join(dir, '.local', 'bin', 'rocky');
      expect(readlinkSync(cli)).toBe(join(dir, 'data', 'rocky', 'current', 'rocky'));
      const via = Bun.spawnSync({ cmd: [cli, 'today'] });
      expect(via.stdout.toString()).toBe('1.0.0:today\n');
      // 다음 버전도 같은 링크를 그대로 탄다 — current 만 옮겨진다.
      run(['hook', 'ensure-daemon'], { CLAUDE_PLUGIN_ROOT: installed('1.1.0') });
      expect(Bun.spawnSync({ cmd: [cli, 'today'] }).stdout.toString()).toBe('1.1.0:today\n');
    });

    test('~/.local/bin/rocky 가 남의 파일(링크 아님)이면 건드리지 않는다', () => {
      const cli = join(dir, '.local', 'bin', 'rocky');
      mkdirSync(join(dir, '.local', 'bin'), { recursive: true });
      writeFileSync(cli, '#!/bin/sh\necho theirs\n');
      run(['hook', 'ensure-daemon'], { CLAUDE_PLUGIN_ROOT: installed('1.0.0') });
      expect(readFileSync(cli, 'utf8')).toContain('theirs');
    });

    test('SessionStart 가 아닌 호출은 (이미 받은 버전이면) 링크를 건드리지 않는다', () => {
      const root = installed('1.0.0');
      run(['hook', 'notify-todo'], { CLAUDE_PLUGIN_ROOT: root });
      run(['ls'], { CLAUDE_PLUGIN_ROOT: root });
      expect(existsSync(link())).toBe(false);
      expect(existsSync(join(dir, '.local', 'bin', 'rocky'))).toBe(false);
    });

    /** 진짜 tarball 을 로컬 서버로 서빙해 다운로드 경로를 끝까지 태운다(가짜 uname 으로 어느 러너든). */
    function fakeRelease(version: string): { base: string; stop: () => void } {
      const stage = join(dir, `stage-${version}`);
      mkdirSync(stage, { recursive: true });
      // tarball 에도 링크째 담는다 — 풀린 새 파일을 띄우면 검사를 다시 거친다.
      for (const name of ['rocky', 'rockyd']) {
        fakeExec(join(stage, name), `#!/bin/sh\necho "${version}:$*"\n`);
      }
      const asset = `rocky-v${version}-aarch64-apple-darwin.tar.gz`;
      const tgz = join(dir, asset);
      const tar = Bun.spawnSync({ cmd: ['tar', '-czf', tgz, '-C', stage, 'rocky', 'rockyd'] });
      if (tar.exitCode !== 0) {
        throw new Error(`tar failed: ${tar.stderr.toString()}`);
      }
      const hasher = new Bun.CryptoHasher('sha256');
      hasher.update(readFileSync(tgz));
      const sums = `${hasher.digest('hex')}  ${asset}\n`;
      const server = Bun.serve({
        port: 0,
        hostname: '127.0.0.1',
        fetch: (req) => {
          const path = new URL(req.url).pathname;
          if (path.endsWith(`/${asset}`)) {
            return new Response(Bun.file(tgz));
          }
          if (path.endsWith('/SHA256SUMS')) {
            return new Response(sums);
          }
          return new Response('nope', { status: 404 });
        },
      });
      return { base: `http://127.0.0.1:${server.port}/dl`, stop: () => server.stop(true) };
    }

    // /reload-plugins 뒤 worklog MCP 서버 기동이 새 버전을 처음 받는 경우 — 0.27→0.28 에서
    // 이 경로가 받아 놓고 링크는 옛 버전에 남겨 `~/.local/bin/rocky` 가 구버전을 봤다.
    test('새 버전을 받은 호출은 SessionStart 가 아니어도 current 를 그 버전으로 건다', async () => {
      run(['hook', 'ensure-daemon'], { CLAUDE_PLUGIN_ROOT: installed('1.0.0') });
      expect(readlinkSync(link())).toBe('v1.0.0');
      const release = fakeRelease('1.1.0');
      try {
        const r = await runAsync(['mcp', 'worklog'], {
          CLAUDE_PLUGIN_ROOT: pluginRoot('1.1.0'),
          ROCKY_RELEASE_BASE: release.base,
          PATH: darwinArm64Path(),
        });
        expect(r.code, r.err).toBe(0);
        expect(r.out).toBe('1.1.0:mcp worklog\n');
      } finally {
        release.stop();
      }
      expect(readlinkSync(link())).toBe('v1.1.0');
      expect(readlinkSync(join(dir, '.local', 'bin', 'rocky'))).toBe(
        join(dir, 'data', 'rocky', 'current', 'rocky'),
      );
    });
  });

  // 백그라운드 다운로드를 관찰하려면 그 잡이 살아 있어야 한다 — 죽은 포트는 즉시 실패해
  // 마커가 단정 전에 사라진다(레이스). 응답을 영영 안 주는 로컬 서버에 붙여 붙들어 둔다.
  function hangingRelease(): { base: string; stop: () => void } {
    const server = Bun.serve({
      port: 0,
      hostname: '127.0.0.1',
      fetch: () => new Promise<Response>(() => {}),
    });
    return { base: `http://127.0.0.1:${server.port}/none`, stop: () => server.stop(true) };
  }

  test('바이너리가 없을 때 SessionStart 가 아닌 훅은 조용히 0 으로 끝나고, 다운로드는 백그라운드로 한 번만 띄운다', () => {
    const root = join(import.meta.dir, '..', 'plugin');
    const version = JSON.parse(readFileSync(join(root, '.claude-plugin', 'plugin.json'), 'utf8'))
      .version as string;
    const marker = join(dir, 'data', 'rocky', `v${version}.downloading`);
    const release = hangingRelease();
    try {
      for (const hook of ['notify-todo', 'handoff-stop']) {
        const r = run(['hook', hook], {
          CLAUDE_PLUGIN_ROOT: root,
          ROCKY_RELEASE_BASE: release.base,
          PATH: darwinArm64Path(),
        });
        expect(r.code).toBe(0);
        expect(r.out).toBe('');
        expect(r.err).toBe('');
        // /reload-plugins 뒤의 첫 프롬프트 — SessionStart 경로를 백그라운드로 띄운 흔적.
        // 두 번째 훅은 마커가 있어 새로 띄우지 않는다(마커는 백그라운드 잡이 끝나며 지운다).
        expect(existsSync(marker)).toBe(true);
      }
    } finally {
      release.stop();
    }
  });

  test('10분 넘은 다운로드 마커는 죽은 것으로 보고 다시 띄운다', () => {
    const root = join(import.meta.dir, '..', 'plugin');
    const version = JSON.parse(readFileSync(join(root, '.claude-plugin', 'plugin.json'), 'utf8'))
      .version as string;
    const marker = join(dir, 'data', 'rocky', `v${version}.downloading`);
    mkdirSync(marker, { recursive: true });
    const old = new Date(Date.now() - 11 * 60_000);
    utimesSync(marker, old, old);
    const release = hangingRelease();
    try {
      const r = run(['hook', 'notify-todo'], {
        CLAUDE_PLUGIN_ROOT: root,
        ROCKY_RELEASE_BASE: release.base,
        PATH: darwinArm64Path(),
      });
      expect(r.code).toBe(0);
      // 새로 만든 마커라 mtime 이 방금이다.
      expect(statSync(marker).mtimeMs).toBeGreaterThan(Date.now() - 60_000);
    } finally {
      release.stop();
    }
  });

  test('ensure-daemon 은 받으려 하고, 실패하면 fail-open(0) + stderr 한 줄', () => {
    const r = run(['hook', 'ensure-daemon'], {
      CLAUDE_PLUGIN_ROOT: join(import.meta.dir, '..', 'plugin'),
    });
    expect(r.code).toBe(0);
    // Apple Silicon 이 아닌 러너(CI 의 ubuntu)는 다운로드 전에 플랫폼에서 걸린다
    expect(r.err).toMatch(/다운로드 실패|미지원 플랫폼/);
  });

  test('CLI 직접 실행은 같은 실패를 exit 1 로 낸다', () => {
    const r = run(['ls'], { CLAUDE_PLUGIN_ROOT: join(import.meta.dir, '..', 'plugin') });
    expect(r.code).toBe(1);
    expect(r.err).toMatch(/다운로드 실패|미지원 플랫폼/);
  });
});
