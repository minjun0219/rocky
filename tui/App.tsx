/**
 * 화면 — 머리(보드·연결) → 줄 목록(지금 + 보드 트리) → 상태 줄. 줄이 화면보다 많으면 커서가 보이게 창을 민다.
 */
import { Box, Text, useApp, useInput, useWindowSize } from 'ink';
import { useEffect, useMemo, useState } from 'react';
import { useStore } from 'zustand';
import { formatAge, formatDue, isOverdue, needsSecondTick, type NowRow } from '../web/lib';
import type { TodoView } from '../web/types';
import { buildLines, cursorAt, GLYPH, type Line, moveCursor, todoGlyph } from './rows';
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

export function App({ store }: { store: TuiStore }) {
  const { exit } = useApp();
  const { columns, rows } = useWindowSize();
  const state = useStore(store);
  const [fast, setFast] = useState(false);
  const now = useNow(fast);
  const board = state.boards.find((b) => b.key === state.boardKey);

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

  // 키 처리는 렌더 시점의 값이 아니라 스토어의 지금 값을 읽는다 — 키가 다시 그리기보다 빨리 연달아 오면(j 연타)
  // 렌더 시점의 커서로는 앞 키의 결과가 안 보여 커서가 덜 움직인다.
  useInput((input, key) => {
    const s = store.getState();
    if (input === 'q') {
      exit();
      return;
    }
    const move = (delta: number) => {
      const to = moveCursor(lines, cursorAt(lines, s.cursorKey, s.cursorIndex), delta);
      if (to >= 0) {
        s.setCursor(lines[to]?.key, to);
      }
    };
    if (input === 'j' || key.downArrow) {
      move(1);
    } else if (input === 'k' || key.upArrow) {
      move(-1);
    } else if (input === ']') {
      s.shiftBoard(1);
    } else if (input === '[') {
      s.shiftBoard(-1);
    } else if (input === 'r') {
      void s.load();
    }
  });

  // 머리 한 줄 + 상태 줄 한 줄을 뺀 만큼이 목록 창이다. 커서를 창의 위쪽 1/3 근처에 둔다.
  const height = Math.max(3, (rows || 24) - 2);
  const start =
    lines.length <= height
      ? 0
      : Math.min(lines.length - height, Math.max(0, cursor - Math.floor(height / 3)));
  const visible = lines.slice(start, start + height);

  return (
    <Box flexDirection="column" width={columns}>
      <Header
        title={board ? board.title : '보드 없음'}
        index={state.boards.findIndex((b) => b.key === state.boardKey)}
        total={state.boards.length}
        connected={state.connected}
        retryInMs={state.retryInMs}
      />
      <Box flexDirection="column" height={height}>
        {!state.ready && !state.error ? (
          <Text dimColor>데몬에서 읽는 중이에요…</Text>
        ) : (
          visible.map((line, i) => (
            <LineView key={line.key} line={line} selected={start + i === cursor} now={now} />
          ))
        )}
      </Box>
      {state.error ? (
        <Text color="red" wrap="truncate-end">
          {state.error}
        </Text>
      ) : (
        <Text dimColor wrap="truncate-end">
          j/k 이동 · [ ] 보드 · r 새로고침 · q 끝
        </Text>
      )}
    </Box>
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
