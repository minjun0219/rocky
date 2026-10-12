/**
 * 할 일 상세 — 읽기만. 메타(상태·누가·우선순위·마감·라벨) → 링크 → 본문(마크다운 원문) → 타임라인(댓글·히스토리,
 * 최신 우선). 높이를 넘는 아래쪽은 잘린다.
 */
import { Box, Text } from 'ink';
import {
  formatAge,
  formatDue,
  formatStamp,
  isOverdue,
  mergeTimeline,
  type TimelineItem,
} from '../web/lib';
import type { TodoView } from '../web/types';
import type { TodoDetail } from './api';
import { todoGlyph } from './rows';

const STATUS_LABEL: Record<TodoView['status'], string> = {
  todo: '열림',
  doing: '진행 중',
  done: '완료',
};

/** 타임라인에 싣는 최대 항목 — 화면 높이와 무관하게 오래된 것까지 다 그리지 않는다. */
const TIMELINE_MAX = 12;

export function Detail(props: {
  detail?: TodoDetail;
  /** 받기 전이라도 목록에서 아는 만큼은 먼저 그린다. */
  fallback?: TodoView;
  height: number;
  width?: number;
  now: number;
}) {
  const todo = props.detail?.todo ?? props.fallback;
  if (!todo) {
    return (
      <Box height={props.height} width={props.width}>
        <Text dimColor>할 일을 고르면 여기 상세가 보여요</Text>
      </Box>
    );
  }
  const glyph = todoGlyph(todo);
  const meta = [
    `${todo.ref}`,
    todo.status === 'doing'
      ? `${STATUS_LABEL.doing}${todo.doingBy ? ` · ${todo.doingBy}` : ''}${
          todo.doingSince ? ` ${formatAge(todo.doingSince, props.now, { live: true })}` : ''
        }`
      : STATUS_LABEL[todo.status],
    todo.priority === 'p4' ? '' : todo.priority,
    todo.labels.length > 0 ? todo.labels.map((l) => `#${l}`).join(' ') : '',
  ].filter(Boolean);
  const overdue = todo.due && todo.status !== 'done' ? isOverdue(todo.due) : false;
  const timeline = props.detail
    ? mergeTimeline(props.detail.history, props.detail.comments).slice(0, TIMELINE_MAX)
    : [];

  return (
    <Box flexDirection="column" height={props.height} width={props.width} overflowY="hidden">
      <Text bold wrap="wrap">
        <Text color={glyph.color}>{glyph.char}</Text> {todo.title}
      </Text>
      <Text dimColor wrap="truncate-end">
        {meta.join(' · ')}
        {todo.due ? (
          <Text color={overdue ? 'red' : undefined}>{` · 마감 ${formatDue(todo.due)}`}</Text>
        ) : null}
      </Text>
      {todo.links.map((link) => (
        <Text key={link.url} color="cyan" wrap="truncate-end">
          {link.title ? `${link.title} ` : ''}
          {link.url}
        </Text>
      ))}
      {todo.description.trim() ? (
        <Box marginTop={1}>
          <Text wrap="wrap">{todo.description.trim()}</Text>
        </Box>
      ) : null}
      {props.detail ? (
        <Box flexDirection="column" marginTop={1}>
          <Text bold color="gray">
            타임라인{timeline.length === 0 ? ' — 아직 없어요' : ''}
          </Text>
          {timeline.map((item) => (
            <TimelineLine key={keyOf(item)} item={item} />
          ))}
        </Box>
      ) : (
        <Text dimColor>읽는 중이에요…</Text>
      )}
    </Box>
  );
}

function keyOf(item: TimelineItem): string {
  if (item.kind === 'comment') {
    return `c:${item.comment.id}`;
  }
  if (item.kind === 'history') {
    return `h:${item.entry.id}`;
  }
  return `w:${item.work.id}`;
}

function TimelineLine({ item }: { item: TimelineItem }) {
  const stamp = formatStamp(item.at);
  if (item.kind === 'comment') {
    // 댓글은 첫 줄만 — 상세 패널은 훑어보는 자리다. 전부는 웹에서.
    const first = item.comment.body.trim().split('\n')[0] ?? '';
    return (
      <Text wrap="truncate-end">
        <Text dimColor>{stamp}</Text> {item.comment.actor}: {first}
      </Text>
    );
  }
  if (item.kind === 'history') {
    const status = item.entry.changes?.status;
    const change = status ? ` ${String(status[0])}→${String(status[1])}` : '';
    return (
      <Text dimColor wrap="truncate-end">
        {stamp} {item.entry.actor} {item.entry.action}
        {change}
      </Text>
    );
  }
  return null;
}
