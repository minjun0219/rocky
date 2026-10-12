/**
 * 화면 — 머리(보드·연결) → 줄 목록(지금 + 보드 트리) → 상태 줄. 줄이 화면보다 많으면 커서가 보이게 창을 민다.
 * 넓으면 오른쪽에 커서를 따라가는 상세, 좁으면 Enter 로 상세를 목록 자리에 띄운다.
 */
import { Box, Text, useApp, useInput, useWindowSize } from 'ink';
import { useEffect, useMemo, useState } from 'react';
import { useStore } from 'zustand';
import { formatAge, formatDue, isOverdue, needsSecondTick, type NowRow } from '../web/lib';
import type { TodoView } from '../web/types';
import { Detail } from './Detail';
import { buildLines, cursorAt, GLYPH, type Line, moveCursor, todoGlyph, todoIdOf } from './rows';
import type { TuiStore } from './store';

/** 지금 시각 — 초가 흐르는 행(1시간 미만의 진행 중)이 있으면 1초, 아니면 1분마다(웹 "Time Display"). */
function useNow(fast: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), fast ? 1000 : 60_000);
    return () => clearInterval(timer);
  }, [fast]);
  return now;
}

/** 이 폭 이상이면 두 열(목록 + 커서를 따라가는 상세), 아래면 한 열(Enter 로 상세를 밀어 넣는다). */
export const WIDE_COLUMNS = 100;

export function App({ store }: { store: TuiStore }) {
  const { exit } = useApp();
  const { columns, rows } = useWindowSize();
  const state = useStore(store);
  const [fast, setFast] = useState(false);
  const now = useNow(fast);
  const board = state.boards.find((b) => b.key === state.boardKey);
  const wide = columns >= WIDE_COLUMNS;

  const lines = useMemo(
    () =>
      buildLines({
        todos: state.todos,
        handoffs: state.handoffs,
        boardId: board?.id,
        sections: state.sections,
        sectionsReady: state.sectionsBoard === state.boardKey,
        now,
      }),
    [
      state.todos,
      state.handoffs,
      board?.id,
      state.sections,
      state.sectionsBoard,
      state.boardKey,
      now,
    ],
  );
  const nowLines = lines.flatMap((l) => (l.kind === 'now' ? [l.row] : []));
  const needFast = needsSecondTick(nowLines, now);
  useEffect(() => setFast(needFast), [needFast]);

  const cursor = cursorAt(lines, state.cursorKey, state.cursorIndex);
  const cursorLine = cursor >= 0 ? lines[cursor] : undefined;
  const cursorId = cursorLine ? todoIdOf(cursorLine) : undefined;
  // "지금" 행은 다른 보드 것일 수 있다 — 할 일은 전 보드 목록에서 찾는다.
  const cursorTodo = cursorId ? state.todos.find((t) => t.id === cursorId) : undefined;

  // 넓은 화면은 상세가 커서를 따라간다 — j 를 누르고 있을 때 요청이 쏟아지지 않게 잠깐 멈춘 뒤에 받는다.
  const { showDetail } = state;
  useEffect(() => {
    if (!wide) {
      return;
    }
    const timer = setTimeout(() => void showDetail(cursorId), 80);
    return () => clearTimeout(timer);
  }, [wide, cursorId, showDetail]);

  // 키 처리는 렌더 시점의 값이 아니라 스토어의 지금 값을 읽는다 — 키가 다시 그리기보다 빨리 연달아 오면(빠른 타이핑·
  // 붙여넣기·j 연타) 렌더 시점의 값으로는 앞 키의 결과가 안 보여 글자가 빠지고 커서가 덜 움직인다.
  useInput((input, key) => {
    const s = store.getState();
    if (s.notice) {
      s.setNotice(undefined);
    }
    // 동작 실패(상태 변경·추가)는 다음 키에서 지운다 — 읽기 실패는 다음 읽기가 지운다.
    if (s.errorFrom === 'action') {
      s.setError(undefined);
    }
    // 빠른 추가 입력 중 — 다른 키는 글자다.
    if (s.draft !== undefined) {
      if (key.return) {
        void s.submitDraft();
      } else if (key.escape) {
        s.setDraft(undefined);
      } else if (key.backspace || key.delete) {
        s.setDraft(s.draft.slice(0, -1));
      } else if (input && !key.ctrl && !key.meta) {
        s.setDraft(s.draft + input);
      }
      return;
    }
    if (input === 'q') {
      exit();
      return;
    }
    const detailShown = s.detailOpen && !wide;
    if (detailShown && (key.escape || key.leftArrow)) {
      s.setDetailOpen(false);
      return;
    }
    const at = cursorAt(lines, s.cursorKey, s.cursorIndex);
    const atLine = at >= 0 ? lines[at] : undefined;
    const atId = atLine ? todoIdOf(atLine) : undefined;
    // 상태 변경 키가 닿는 할 일 — 좁은 화면에서 상세를 띄웠으면 화면에 보이는 그 할 일이다(SSE 갱신으로 커서가 옮겨도).
    const targetId = detailShown ? s.detailId : atId;
    const atTodo = targetId ? s.todos.find((t) => t.id === targetId) : undefined;
    const move = (delta: number) => {
      const to = moveCursor(lines, at, delta);
      if (to >= 0) {
        s.setCursor(lines[to]?.key, to);
      }
    };
    // 상세를 목록 자리에 띄운 동안 커서는 보이지 않는다 — 옮기지 않는다.
    if (!detailShown && (input === 'j' || key.downArrow)) {
      move(1);
    } else if (!detailShown && (input === 'k' || key.upArrow)) {
      move(-1);
    } else if ((key.return || key.rightArrow) && atId && !wide) {
      s.setDetailOpen(true);
      void s.showDetail(atId);
    } else if (input === 's' && atTodo) {
      void s.act(atTodo, 'start');
    } else if (input === 'x' && atTodo) {
      void s.act(atTodo, 'stop');
    } else if (input === 'd' && atTodo) {
      void s.act(atTodo, 'done');
    } else if (input === 'a' && s.boardKey) {
      s.setDraft('');
    } else if (input === ']') {
      s.shiftBoard(1);
    } else if (input === '[') {
      s.shiftBoard(-1);
    } else if (input === 'r') {
      void s.load();
    }
  });

  // 머리 한 줄 + 상태 줄 한 줄을 뺀 만큼이 본문 높이다. 커서를 창의 위쪽 1/3 근처에 둔다.
  const height = Math.max(3, (rows || 24) - 2);
  const start =
    lines.length <= height
      ? 0
      : Math.min(lines.length - height, Math.max(0, cursor - Math.floor(height / 3)));
  const visible = lines.slice(start, start + height);
  const list =
    !state.ready && !state.error ? (
      <Text dimColor>데몬에서 읽는 중이에요…</Text>
    ) : (
      visible.map((line, i) => (
        <LineView key={line.key} line={line} selected={start + i === cursor} now={now} />
      ))
    );
  const detailTodo = state.detail?.todo.id === state.detailId ? state.detail : undefined;
  const listWidth = Math.max(48, Math.floor(columns * 0.5));

  return (
    <Box flexDirection="column" width={columns}>
      <Header
        title={board ? board.title : '보드 없음'}
        index={state.boards.findIndex((b) => b.key === state.boardKey)}
        total={state.boards.length}
        connected={state.connected}
        retryInMs={state.retryInMs}
      />
      {wide ? (
        <Box height={height}>
          <Box flexDirection="column" width={listWidth}>
            {list}
          </Box>
          <Box
            flexGrow={1}
            paddingLeft={1}
            borderStyle="single"
            borderTop={false}
            borderRight={false}
            borderBottom={false}
            borderColor="gray"
          >
            <Detail detail={detailTodo} fallback={cursorTodo} height={height} now={now} />
          </Box>
        </Box>
      ) : state.detailOpen ? (
        <Detail
          detail={detailTodo}
          fallback={state.todos.find((t) => t.id === state.detailId)}
          height={height}
          now={now}
        />
      ) : (
        <Box flexDirection="column" height={height}>
          {list}
        </Box>
      )}
      <StatusLine
        draft={state.draft}
        error={state.error}
        notice={state.notice}
        hint={
          state.detailOpen && !wide
            ? 'Esc 목록 · s 착수 x 멈춤 d 완료 · q 끝'
            : `j/k 이동${wide ? '' : ' · Enter 상세'} · s 착수 x 멈춤 d 완료 · a 추가 · [ ] 보드 · q 끝`
        }
      />
    </Box>
  );
}

function StatusLine(props: { draft?: string; error?: string; notice?: string; hint: string }) {
  if (props.draft !== undefined) {
    return (
      <Text wrap="truncate-start">
        새 할 일: {props.draft}
        <Text inverse> </Text>
        <Text dimColor> Enter 추가 · Esc 취소</Text>
      </Text>
    );
  }
  if (props.error) {
    return (
      <Text color="red" wrap="truncate-end">
        {props.error}
      </Text>
    );
  }
  if (props.notice) {
    return (
      <Text color="green" wrap="truncate-end">
        ✓ {props.notice}
      </Text>
    );
  }
  return (
    <Text dimColor wrap="truncate-end">
      {props.hint}
    </Text>
  );
}

function Header(props: {
  title: string;
  index: number;
  total: number;
  connected: boolean;
  retryInMs?: number;
}) {
  return (
    <Box>
      <Box flexGrow={1}>
        <Text bold wrap="truncate-end">
          rocky · {props.title}
          {props.total > 1 ? <Text dimColor>{` ${props.index + 1}/${props.total}`}</Text> : null}
        </Text>
      </Box>
      {props.connected ? (
        <Text color="green">● 연결</Text>
      ) : (
        <Text color="red">
          끊김{props.retryInMs ? ` · ${Math.round(props.retryInMs / 1000)}초 뒤 다시` : ''}
        </Text>
      )}
    </Box>
  );
}

function LineView({ line, selected, now }: { line: Line; selected: boolean; now: number }) {
  const mark = selected ? '›' : ' ';
  if (line.kind === 'heading') {
    return (
      <Text bold color="gray" wrap="truncate-end">
        {line.title}
        {line.count ? ` ${line.count}` : ''}
      </Text>
    );
  }
  if (line.kind === 'empty') {
    return (
      <Text dimColor wrap="truncate-end">
        {`  ${line.text}`}
      </Text>
    );
  }
  if (line.kind === 'now') {
    return <NowLine row={line.row} mark={mark} selected={selected} now={now} />;
  }
  return <TodoLine todo={line.todo} depth={line.depth} mark={mark} selected={selected} now={now} />;
}

function NowLine(props: { row: NowRow; mark: string; selected: boolean; now: number }) {
  const { row } = props;
  const glyph = GLYPH[row.glyph];
  const age = row.since ? formatAge(row.since, props.now, { live: row.live }) : '';
  const meta = [row.who === '—' ? '' : row.who, row.state, age].filter(Boolean).join(' ');
  return (
    <Box>
      <Box flexGrow={1} flexShrink={1}>
        <Text inverse={props.selected} wrap="truncate-end">
          {props.mark}
          <Text color={glyph.color}>{glyph.char}</Text> {row.ref} {row.title}
        </Text>
      </Box>
      {meta ? <Text dimColor>{` ${meta}`}</Text> : null}
    </Box>
  );
}

function TodoLine(props: {
  todo: TodoView;
  depth: number;
  mark: string;
  selected: boolean;
  now: number;
}) {
  const { todo } = props;
  const glyph = todoGlyph(todo);
  const overdue = todo.due && todo.status !== 'done' ? isOverdue(todo.due) : false;
  const meta =
    todo.status === 'doing' && todo.doingSince
      ? formatAge(todo.doingSince, props.now, { live: true })
      : todo.due
        ? formatDue(todo.due)
        : '';
  return (
    <Box>
      <Box flexGrow={1} flexShrink={1}>
        <Text inverse={props.selected} wrap="truncate-end">
          {props.mark}
          {'  '.repeat(props.depth)}
          <Text color={glyph.color}>{glyph.char}</Text>
          <Text dimColor>{` #${todo.number}`}</Text> {todo.title}
          {todo.priority === 'p1' ? <Text color="yellow"> !</Text> : null}
        </Text>
      </Box>
      {meta ? (
        <Text color={overdue ? 'red' : undefined} dimColor={!overdue}>{` ${meta}`}</Text>
      ) : null}
    </Box>
  );
}
