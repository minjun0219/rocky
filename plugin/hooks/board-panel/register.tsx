// rocky 보드 패널 — Claude Code function hooks(early access)로 세션 옆 패널에 이 폴더의 보드를 띄운다
// (docs/features/board-panel.md). 데몬을 읽어 그리기만 하고 쓰지 않는다. 판정은 data.ts(bun 테스트).
import { atom, read, update } from 'claude-code';
import type { EngineInterface, Register } from 'claude-code';

import {
  type BoardItem,
  boardTouched,
  EMPTY_VIEW,
  type Get,
  parseChanges,
  readItems,
  readSummary,
} from './data';

const PANE = 'rocky-board';
const TITLE = 'rocky 보드';
/** plugin.json 의 MCP 주소와 같은 데몬. */
const DAEMON = 'http://127.0.0.1:8636';
/**
 * 사용 로그가 패널의 요청을 라우트로 세지 않게 다는 `x-rocky-client`(`rocky_core::usage::PANEL_CLIENT`). 세션마다 한 번씩만
 * `x-rocky-usage` 를 더한다 — 첫 보드 읽기에 `open`(패널이 떴다), `/rocky-board`·패널 포커스에 `use`(사람이 손을 댔다).
 * 폴링이 사용 횟수를 부풀리지 않게.
 */
const CLIENT = 'claude-code-panel';
/** 바뀐 게 있는지 묻는 간격 — 보드 히스토리의 새 행만 읽는 가벼운 요청이다. 이 보드를 건드렸을 때만 할 일을 다시 읽는다. */
const POLL_MS = 5_000;
/** 한 번에 받는 새 히스토리 행 — 꽉 차면 더 있을 수 있어 이 보드를 건드린 것으로 본다. */
const CHANGES_LIMIT = 50;
/** 보드 key·수집함 수(요약)를 다시 읽는 간격 — 수집함은 히스토리에 남지 않아 변화로는 알 수 없다. */
const SUMMARY_MS = 60_000;

const view = atom({ plugin: 'rocky', key: 'boardPanel' } as const, EMPTY_VIEW);

/** 한 번 로드된 동안의 상태 — 리로드하면 새로 만들어진다. */
type Panel = {
  /** 마지막으로 요약을 읽은 세션 폴더 — `/cd` 로 바뀌면 다시 읽는다. */
  cwd: string | null;
  /** 그 폴더의 보드 key — 붙은 보드가 없으면 null. */
  board: string | null;
  collect: number | null;
  /** 마지막으로 요약을 읽은 시각(ms) — 0 이면 아직. */
  summaryAt: number;
  /** 마지막으로 본 보드 히스토리 id — null 이면 아직(첫 읽기는 행 없이 id 만). */
  lastId: number | null;
  inFlight: boolean;
  /** 사용 로그에 남긴 표시 — 세션마다 하나씩(`open` 노출, `use` 사용). */
  marked: Set<UsageMark>;
  /** 보드가 있는 폴더에서 패널을 한 번 열었나 — 사람이 닫은 패널을 폴링이 다시 열지 않게. */
  opened: boolean;
};

type UsageMark = 'open' | 'use';

/** 이 세션의 패널 — `session.start` 가 만든다. 리로드하면 새로 만들어진다(엔진이 `session.start` 를 다시 부른다). */
let current: Panel | null = null;

/** 데몬 GET — 패널 클라이언트 표시를 달고, `mark` 가 이 세션에 아직 없으면 사용 표시를 더한다. */
function daemonGet($: EngineInterface, panel: Panel, mark?: UsageMark): Get {
  return async (path) => {
    const headers: Record<string, string> = { 'x-rocky-client': CLIENT };
    const marking = mark !== undefined && !panel.marked.has(mark);
    if (marking) {
      headers['x-rocky-usage'] = mark;
      // 응답을 기다리는 사이 같은 표시가 또 붙지 않게 먼저 적는다 — 실패하면 뗀다(다음에 다시).
      panel.marked.add(mark);
    }
    try {
      const res = await $.http.fetch(`${DAEMON}${path}`, { headers });
      if (marking && !res.ok) {
        panel.marked.delete(mark);
      }
      return res;
    } catch (err) {
      if (marking) {
        panel.marked.delete(mark);
      }
      throw err;
    }
  };
}

/** 사람이 패널에 손을 댔다(`/rocky-board`·포커스) — 세션마다 한 번, 가장 가벼운 읽기에 `use` 를 실어 남긴다. */
async function markUse($: EngineInterface): Promise<void> {
  const panel = current;
  if (panel === null || panel.marked.has('use')) {
    return;
  }
  try {
    await daemonGet($, panel, 'use')('/api/changes?sinceId=0&limit=0');
  } catch {
    // 데몬이 꺼져 있으면 남기지 못한다 — 다음 손댐에 다시.
  }
}

/** 읽지 못한 사유를 남긴다 — 지난 목록은 그대로 둔다. 같은 사유면 다시 쓰지 않는다(다시 그리지 않게). */
async function fail($: EngineInterface, error: string): Promise<void> {
  const prev = await read($, view);
  if (prev.error !== error) {
    await update($, view, (v) => ({ ...v, error }));
  }
}

/**
 * 한 바퀴 — 폴더가 바뀌었거나 60초가 지났으면 요약(보드 key·수집함 수), 그다음 새 히스토리. 이 보드를 건드린 행이 있거나
 * 보드·폴더가 바뀌었거나 지난번에 실패했으면 할 일을 다시 읽는다. 바뀐 게 없으면 상태를 쓰지 않는다(다시 그리지 않는다).
 */
async function tick($: EngineInterface, panel: Panel): Promise<void> {
  if (panel.inFlight) {
    return;
  }
  panel.inFlight = true;
  try {
    const now = await $.clock.now();
    const cwd = await $.session.cwd();
    const prev = await read($, view);
    let dirty = prev.error !== null;
    let reload = prev.error !== null;
    if (cwd !== panel.cwd || now - panel.summaryAt >= SUMMARY_MS) {
      const summary = await readSummary(daemonGet($, panel), cwd);
      if (!summary.ok) {
        await fail($, summary.error);
        return;
      }
      const { board, collect } = summary.value;
      reload ||= cwd !== panel.cwd || board !== panel.board;
      dirty ||= reload || collect !== panel.collect;
      Object.assign(panel, { cwd, board, collect, summaryAt: now });
    }
    const since = panel.lastId;
    const res = await daemonGet(
      $,
      panel,
    )(
      since === null
        ? '/api/changes?sinceId=0&limit=0'
        : `/api/changes?sinceId=${since}&limit=${CHANGES_LIMIT}`,
    );
    const changes = res.ok ? parseChanges(res.text) : null;
    if (changes === null) {
      // 실패해도 할 일을 몇 초마다 통째로 다시 읽지 않는다 — 사유만 남기고 다음 바퀴에 다시 묻는다.
      await fail($, res.ok ? '/api/changes 응답을 읽지 못했다' : `/api/changes 응답 ${res.status}`);
      return;
    }
    panel.lastId = changes.lastId;
    if (since === null) {
      reload = true;
    } else if (panel.board !== null && boardTouched(changes, panel.board, CHANGES_LIMIT)) {
      reload = true;
    }
    let items: BoardItem[] = prev.items;
    if (panel.board === null) {
      items = [];
    } else if (reload) {
      const loaded = await readItems(daemonGet($, panel, 'open'), panel.board);
      if (!loaded.ok) {
        await fail($, loaded.error);
        return;
      }
      items = loaded.value;
    }
    if (dirty || reload || prev.at === null) {
      await update($, view, () => ({
        board: panel.board,
        items,
        collect: panel.collect,
        error: null,
        at: new Date(now).toISOString(),
      }));
    }
    if (panel.board !== null && !panel.opened) {
      panel.opened = true;
      // `isPlaced: false` 는 "열렸지만 좁아서 기다린다" 다 — 터미널이 넓어지면 엔진이 그린다(`$.ui.open` 문서: "waits
      // undrawn … until they open it or the terminal widens"). 그래서 다시 열지 않고, 열기 자체가 실패했을 때만 다음 바퀴에.
      void $.ui.open({ id: PANE, title: TITLE }).catch(() => {
        panel.opened = false;
      });
    }
  } catch (err) {
    // $.http.fetch 가 거절됐다(데몬이 꺼졌거나 포트가 다르다) 또는 엔진 호출이 실패했다.
    await fail($, `데몬에 닿지 않는다(${String(err).slice(0, 60)}) — rocky daemon status`);
  } finally {
    panel.inFlight = false;
  }
}

export const register: Register = (on) => {
  on('session.start', async ($, e, next) => {
    // 패널을 그릴 데가 없는 실행(`-p`·SDK·원격 제어)에서는 아무것도 하지 않는다.
    if (!e.isInteractive) {
      return next(e);
    }
    const panel: Panel = {
      cwd: null,
      board: null,
      collect: null,
      summaryAt: 0,
      lastId: null,
      inFlight: false,
      marked: new Set(),
      opened: false,
    };
    current = panel;
    await $.command.register({
      name: 'rocky-board',
      description: '이 폴더의 rocky 보드를 패널에 띄운다',
      immediate: true,
    });
    void tick($, panel);
    $.clock.every(POLL_MS, () => void tick($, panel));
    return next(e);
  });

  on('command.run', { command: 'rocky-board' }, async ($) => {
    await $.ui.open({ id: PANE, title: TITLE });
    void markUse($);
    return { text: 'rocky 보드 패널을 열었다.' };
  });

  on('ui.render', { component: 'Pane', requestId: PANE }, async ($, e) => {
    const { Box, Text } = $.ui.resolve(e);
    // 사람이 패널에 키보드를 줬다(Tab 으로 옮겨 스크롤하려고) — 사용으로 센다.
    if (e.props.isFocused) {
      void markUse($);
    }
    const v = await read($, view);
    const doing = v.items.filter((i) => i.status === 'doing').length;
    const todo = v.items.length - doing;

    return (
      <Box flexDirection="column">
        <Text>
          <Text bold>{v.board ?? '보드 없음'}</Text>
          <Text dimColor>
            {' '}
            · 진행 {doing} · 할 일 {todo}
            {v.collect ? ` · 수집함 ${v.collect}` : ''}
          </Text>
        </Text>
        {v.error && <Text color="yellow">{v.error}</Text>}
        {v.board === null && v.error === null && v.at !== null && (
          <Text dimColor>이 폴더에 붙은 보드가 없다.</Text>
        )}
        {/* 표시·번호는 고정 칸, 제목만 남은 폭에서 줄을 바꾼다 — 둘째 줄부터 제목 첫 글자에 맞춰 들어간다. 다 그리고
            넘치는 것은 엔진이 맡는 본문 스크롤로 본다(패널에 포커스를 두고 화살표). */}
        {v.items.map((i) => (
          <Box key={i.ref} flexDirection="row">
            <Box flexShrink={0}>
              <Text color={i.status === 'doing' ? 'green' : undefined}>
                {i.status === 'doing' ? '▶ ' : '· '}
              </Text>
              <Text dimColor>{i.ref} </Text>
            </Box>
            <Box flexGrow={1} flexShrink={1}>
              <Text wrap="wrap">{i.title}</Text>
            </Box>
          </Box>
        ))}
      </Box>
    );
  });
};
