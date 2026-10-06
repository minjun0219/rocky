// rocky lab — Claude Code function hooks(early access)로 데몬을 세션 화면에 얹는 실험. 사용자 rocky.json 의
// `lab` 블록이 있을 때만 켜진다(docs/features/lab.md). 데몬을 읽어 그리기만 하고, 쓰거나 턴을 열지 않는다.
import { atom, read, update } from 'claude-code';
import type { EngineInterface, Register } from 'claude-code';

import type { LabBand } from '../../types';
import {
  limitsLine,
  parseLabConfig,
  rockyToast,
  summaryParts,
  type InboxSession,
  type LabConfig,
  type Summary,
} from './lib';

const band = atom({ plugin: 'rocky', key: 'labBand' } as const, null);

/** plugin.json 의 MCP 주소와 같은 데몬. */
const DAEMON = 'http://127.0.0.1:8636';
/** 사용 로그에서 lab 이 부른 요청을 가려 볼 수 있게(`x-rocky-client` → usage 의 `client`). */
const HEADERS = { 'x-rocky-client': 'claude-code-lab' };

/** 한 번 로드된 동안의 상태 — 리로드하면 새로 만들어진다. */
type Lab = {
  config: LabConfig | undefined;
  inFlight: boolean;
  /** 읽는 중에 다시 읽어 달라는 요청이 왔다 — 끝나면 한 번 더 돈다. */
  pending: boolean;
  lastBand: string;
  last: string | null;
  lastLimits: string | undefined;
  lastError: string | undefined;
};

/** 사용자 rocky.json(`ROCKY_CONFIG` > `~/.config/rocky/rocky.json`) 의 `lab` 블록. 못 읽으면 꺼짐. */
async function readLabConfig($: EngineInterface): Promise<LabConfig | undefined> {
  const home = await $.env.get('HOME');
  const explicit = await $.env.get('ROCKY_CONFIG');
  const raw = explicit ?? (home ? '~/.config/rocky/rocky.json' : undefined);
  if (raw === undefined) {
    return undefined;
  }
  const path = raw.startsWith('~/') && home ? `${home}${raw.slice(1)}` : raw;
  try {
    return parseLabConfig(await $.fs.read(path));
  } catch {
    return undefined;
  }
}

async function getJson<T>($: EngineInterface, lab: Lab, path: string): Promise<T | undefined> {
  try {
    const res = await $.http.fetch(`${DAEMON}${path}`, { headers: HEADERS });
    if (!res.ok) {
      lab.lastError = `${path} → HTTP ${res.status}`;
      return undefined;
    }
    return JSON.parse(res.text) as T;
  } catch (err) {
    lab.lastError = `${path} → ${String(err)}`;
    return undefined;
  }
}

/** 보드 요약과 이 세션의 받은편지함 등록을 다시 읽는다 — 턴 경계와 rocky 메시지가 올 때만(주기 폴링 없음). */
async function refresh($: EngineInterface, lab: Lab): Promise<void> {
  if (!lab.config?.band) {
    return;
  }
  if (lab.inFlight) {
    lab.pending = true;
    return;
  }
  lab.inFlight = true;
  try {
    do {
      lab.pending = false;
      await readBand($, lab);
    } while (lab.pending);
  } finally {
    lab.inFlight = false;
  }
}

async function readBand($: EngineInterface, lab: Lab): Promise<void> {
  const sessionId = await $.session.id();
  const cwd = await $.session.cwd();
  const summary = await getJson<Summary>(
    $,
    lab,
    `/api/summary?cached=true&cwd=${encodeURIComponent(cwd)}`,
  );
  const inbox = await getJson<{ sessions: InboxSession[] }>($, lab, '/api/deliveries');
  const mine = inbox?.sessions.find((s) => s.sessionId === sessionId);
  const next: LabBand = summary
    ? {
        daemon: 'ok',
        board: summary.board ?? mine?.board ?? null,
        doing: summary.doing,
        collect: summary.collect ?? 0,
        handoffsOpen: summary.handoffsOpen,
        overdue: summary.overdue,
        registered: inbox ? mine !== undefined : null,
        watching: mine?.receivesPrFor.length ?? 0,
        last: lab.last,
        error: null,
      }
    : {
        daemon: 'down',
        board: null,
        doing: 0,
        collect: 0,
        handoffsOpen: 0,
        overdue: 0,
        registered: null,
        watching: 0,
        last: lab.last,
        error: lab.lastError ?? null,
      };
  const json = JSON.stringify(next);
  if (json === lab.lastBand) {
    return;
  }
  await update($, band, () => next);
  lab.lastBand = json;
}

async function diagnose($: EngineInterface, lab: Lab): Promise<string> {
  const sessionId = await $.session.id();
  const envId = await $.env.get('CLAUDE_CODE_SESSION_ID');
  const health = await getJson<{ version: string; pid: number }>($, lab, '/api/health');
  const inbox = await getJson<{ sessions: InboxSession[] }>($, lab, '/api/deliveries');
  const mine = inbox?.sessions.find((s) => s.sessionId === sessionId);
  const prs = mine?.receivesPrFor ?? [];
  const c = lab.config;
  const onOff = (v: boolean | undefined) => (v ? 'on' : 'off');
  return [
    'rocky lab 진단',
    `- 세션 id(엔진): ${sessionId}`,
    `- CLAUDE_CODE_SESSION_ID(env): ${envId ?? '없음'}${envId && envId !== sessionId ? ' ← 엔진과 다름(부모에게서 물려받았거나 /clear·resume 뒤 낡은 값)' : ''}`,
    `- 데몬: ${health ? `v${health.version} (pid ${health.pid})` : `응답 없음${lab.lastError ? ` — ${lab.lastError}` : ''}`}`,
    `- 받은편지함: ${inbox === undefined ? `못 읽음${lab.lastError ? ` — ${lab.lastError}` : ''}` : mine ? `등록됨 · PR 구독 ${prs.length}건${prs.length ? ` (${prs.join(', ')})` : ''}${mine.muted ? ' · 알림 끔' : ''}` : '이 세션 id 로 등록 없음 — PR 알림이 이 세션에 오지 않는다'}`,
    `- 마지막 rocky 메시지: ${lab.last ?? '없음'}`,
    `- 엔진 한도(session.measure): ${lab.lastLimits ?? '아직 측정 없음'}`,
    `- 스위치: toast ${onOff(c?.toast)} · band ${onOff(c?.band)} · limits ${onOff(c?.limits)}`,
  ].join('\n');
}

export const register: Register = (on) => {
  const lab: Lab = {
    config: undefined,
    inFlight: false,
    pending: false,
    lastBand: '',
    last: null,
    lastLimits: undefined,
    lastError: undefined,
  };

  on('session.start', async ($, e, next) => {
    lab.config = await readLabConfig($);
    if (lab.config) {
      await $.command.register({
        name: 'rocky-lab',
        description:
          'rocky lab 진단 — 세션 id·데몬·받은편지함·한도·스위치 (인자 toast: 시험 toast)',
      });
      $.clock.after(0, () => void refresh($, lab));
    }
    return next(e);
  });

  // 데몬이 받은편지함 소켓에 쓴 메시지(PR 전이·리뷰·수집함·핸드오프)를 사람에게도 보인다. 모델이 읽는 본문은 그대로이고,
  // 아래 체인이 가져간(consumed) 배달은 받은 것으로 치지 않는다.
  on('session.receive', async ($, e, next) => {
    const passed = await next(e);
    if (lab.config && e.agentId === undefined && passed.consumed === undefined) {
      const line = rockyToast(e.text);
      if (line !== undefined) {
        lab.last = line;
        if (lab.config.toast) {
          $.ui.toast(line, { timeoutMs: 8000 });
        }
        $.clock.after(0, () => void refresh($, lab));
      }
    }
    return passed;
  });

  // 메인 대화의 턴만 — 서브에이전트 턴마다 읽으면 그게 곧 폴링이다.
  on('turn.complete', ($, e, next) => {
    if (lab.config?.band && e.agentId === undefined) {
      $.clock.after(0, () => void refresh($, lab));
    }
    return next(e);
  });

  on('session.measure', ($, e, next) => {
    if (lab.config?.limits) {
      lab.lastLimits = limitsLine(e.rateLimits, e.context.percent);
      $.ui.status(lab.lastLimits);
    }
    return next(e);
  });

  on('command.run', { command: 'rocky-lab' }, async ($, e) => {
    if (e.args.trim() === 'toast') {
      $.ui.toast('rocky · 시험 toast — 보이면 toast 경로는 산다', { timeoutMs: 8000 });
      return { text: '시험 toast 를 띄웠다.' };
    }
    return { text: await diagnose($, lab) };
  });

  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    if (!lab.config?.band || e.props.hasSurvey) {
      return next(e);
    }
    const b = await read($, band);
    if (b === null) {
      return next(e);
    }
    const { Box, Text } = $.ui.resolve(e);
    if (b.daemon === 'down') {
      return (
        <Box>
          <Text dimColor wrap="truncate-end">
            rocky · 데몬 응답 없음{b.error ? ` — ${b.error}` : ''}
          </Text>
        </Box>
      );
    }
    const parts = summaryParts(b);
    if (b.watching > 0) {
      parts.push(`PR ${b.watching}건 감시`);
    }
    return (
      <Box flexDirection="column">
        <Text wrap="truncate-end">
          <Text dimColor>rocky {b.board ?? '전체'} · </Text>
          {parts.join(' · ') || '조용함'}
          {b.registered === false && <Text color="yellow"> · 받은편지함 미등록</Text>}
        </Text>
        {b.last && (
          <Text dimColor wrap="truncate-end">
            {b.last}
          </Text>
        )}
      </Box>
    );
  });
};
