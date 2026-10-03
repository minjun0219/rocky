import {
  CircleAlert,
  CircleDot,
  CircleHelp,
  CircleOff,
  CircleX,
  GitMerge,
  GitPullRequest,
  GitPullRequestDraft,
  type LucideIcon,
  TriangleAlert,
} from 'lucide-react';
import { useEffect, useState } from 'react';
import {
  formatAge,
  mineCount,
  type NowGlyph,
  type NowRow,
  needsSecondTick,
  nowRows,
  type PrStatus,
} from '../lib';
import { useUiStore } from '../store';
import { logUsage } from '../usage';

/**
 * 현재 시각 — 초가 흐르는 행(1시간 미만의 진행중)이 있으면 1초, 없으면 1분마다. 곁눈으로 보는
 * 화면이라 움직이는 숫자는 정말 필요한 자리에만 둔다(`web/DESIGN.md` "Time Display").
 */
export function useNow(rows: NowRow[], extra: number): number {
  const [now, setNow] = useState(() => Date.now());
  const fast = needsSecondTick(rows, now);
  // 시각을 보여 주는 행이 하나라도 있으면 돈다 — PR 행만 있는 보드도("12분" 이 멈추지 않게).
  const visible = rows.length + extra;
  useEffect(() => {
    if (visible === 0) {
      return;
    }
    const id = setInterval(() => setNow(Date.now()), fast ? 1000 : 60_000);
    return () => clearInterval(id);
  }, [fast, visible]);
  return now;
}

/**
 * 행 앞 상태 아이콘 — 색만으로 말하지 않도록 모양이 상태마다 다르다(`web/DESIGN.md` "State
 * Vocabulary"). lucide 아이콘이라 글꼴에 따라 모양이 흔들리지 않는다. 움직이지 않는다.
 */
const GLYPH: Record<NowGlyph, { Icon: LucideIcon; className: string; label: string }> = {
  run: { Icon: CircleDot, className: 'text-run', label: '돌고 있음' },
  mine: { Icon: CircleAlert, className: 'text-mine', label: '내 차례' },
  dead: { Icon: CircleOff, className: 'text-dead', label: '세션 없음' },
  unknown: { Icon: CircleHelp, className: 'text-faint', label: '세션 모름' },
};

/** PR 상태 아이콘 — 머지 가능·충돌은 내 차례 색, 대기·초안은 무채색. */
export const PR_ICON: Record<PrStatus, { Icon: LucideIcon; className: string; label: string }> = {
  conflict: { Icon: TriangleAlert, className: 'text-dead', label: '충돌' },
  ready: { Icon: GitMerge, className: 'text-mine', label: '확인·머지 가능' },
  failing: { Icon: CircleX, className: 'text-dead', label: 'CI 실패' },
  decide: { Icon: CircleAlert, className: 'text-mine', label: '결정 필요' },
  waiting: { Icon: GitPullRequest, className: 'text-muted', label: '대기' },
  draft: { Icon: GitPullRequestDraft, className: 'text-faint', label: '초안' },
};

/** 아이콘 한 칸 — 행 제목의 첫 줄에 맞춘다. */
export function StateIcon(props: { Icon: LucideIcon; className: string; label: string }) {
  const { Icon } = props;
  return (
    <span
      className={`mt-0.5 flex w-4 shrink-0 justify-center ${props.className}`}
      role="img"
      aria-label={props.label}
    >
      <Icon size={14} strokeWidth={2.25} aria-hidden />
    </span>
  );
}

/**
 * 내 차례의 행 — 보고 있는 보드와 무관하게 전 보드를 본다. 무엇을 어떤 순서로 싣는지는 `nowRows`(순수)가
 * 정한다. PR 은 피드의 PR 알림이 따로 맡는다 — 여기서는 할 일만(넘김·멈춘 진행·읽지 않은 댓글·수집함).
 */
export function useNowRows(expanded = false): NowRow[] {
  const nowTodos = useUiStore((s) => s.nowTodos);
  const handoffs = useUiStore((s) => s.nowHandoffs);
  const seenComments = useUiStore((s) => s.seenComments);
  const collect = useUiStore((s) => s.collect);
  return nowRows({ todos: nowTodos, handoffs, seen: seenComments, collect, expanded });
}

/** 피드의 "내 차례" — 예전엔 할 일 화면 맨 위 "지금" 표에 있었다(2026-10-02 피드로 옮김). */
export function MineSection() {
  const [expanded, setExpanded] = useState(false);
  const rows = useNowRows(expanded);
  const mine = rows.filter((r) => r.group !== 'run');
  const now = useNow(mine, 0);
  return (
    <section className="mb-6" aria-label="내 차례">
      <NowGroupHead title="내 차례" count={mineCount(rows)} tone="mine" />
      {mine.length === 0 ? (
        <p className="m-0 text-meta text-muted">내 차례 없음</p>
      ) : (
        <ul className="m-0 list-none overflow-hidden rounded-lg border border-line bg-surface p-0 shadow-xs">
          {mine.map((row) =>
            row.group === 'more' ? (
              <MoreLine
                key={row.key}
                row={row}
                onExpand={row.key === 'mine:more' ? () => setExpanded(true) : undefined}
              />
            ) : (
              <NowItem key={row.key} row={row} now={now} />
            ),
          )}
        </ul>
      )}
    </section>
  );
}

/**
 * "돌고 있음" — 할 일 화면 맨 위. 세션이 붙어 진행 중인 일(전 보드). 없으면 자리를 차지하지 않는다.
 * 손댈 것(내 차례)은 피드로 옮겼다.
 */
export function NowTable() {
  const rows = useNowRows();
  const run = rows.filter((r) => r.group === 'run');
  const now = useNow(run, 0);
  if (run.length === 0) {
    return null;
  }
  return (
    <section className="now border-b border-line px-4 pb-3.5 pt-3" aria-label="돌고 있음">
      <NowGroupHead title="돌고 있음" count={run.length} tone="run" />
      <ul className="m-0 list-none overflow-hidden rounded-lg border border-line bg-surface p-0 shadow-xs">
        {run.map((row) => (
          <NowItem key={row.key} row={row} now={now} />
        ))}
      </ul>
    </section>
  );
}

/** 묶음 머리 — 이름 + 개수. 같은 상태를 행마다 반복하는 대신 여기서 한 번 말한다. */
function NowGroupHead(props: { title: string; count: number; tone: 'mine' | 'run' }) {
  const isMine = props.tone === 'mine';
  return (
    <h2 className="m-0 mb-2 flex items-baseline gap-2 font-mono text-chip font-medium text-faint">
      {props.title}
      {props.count > 0 ? (
        <span
          className={`rounded-[4px] px-1.5 py-0.2 font-mono text-chip font-semibold tabular-nums ${
            isMine ? 'bg-mine-soft text-mine' : 'bg-run-soft text-run'
          }`}
        >
          {props.count}
        </span>
      ) : null}
    </h2>
  );
}

/**
 * 한 행 — 첫 줄: 글리프 + 제목(두 줄까지), 둘째 줄: ref · 누가 · 시각 · 상태 글자.
 * 행 전체가 누르는 자리다(todo 면 상세, PR 이면 새 탭).
 */
function NowItem(props: { row: NowRow; now: number }) {
  const { row, now } = props;
  const openTodoDetail = useUiStore((s) => s.openTodoDetail);
  // PR 행은 PR 모양 아이콘으로 — 머지 가능이면 머지, 충돌이면 경고.
  const glyph =
    row.kind === 'pr' ? PR_ICON[row.glyph === 'dead' ? 'conflict' : 'ready'] : GLYPH[row.glyph];
  const age = row.since
    ? formatAge(row.since, now, {
        live: row.live,
        // 진행 기준 시각이면 "…부터" — 세션이 사라진 진행중(dead)도 같다.
        since: row.kind === 'doing' || row.kind === 'dead',
      })
    : '';
  const meta = [row.ref, row.who !== '—' ? row.who : '', age, row.state].filter(Boolean);
  const body = (
    <>
      <StateIcon {...glyph} />
      <span className="min-w-0 flex-1">
        <span className="now-title block text-sm leading-[1.45] text-text">
          {row.title}
          {row.unread > 0 ? (
            <span className="ml-1.5 font-mono text-chip font-semibold text-mine">
              💬 {row.unread}
            </span>
          ) : null}
        </span>
        <span className="mt-0.5 block truncate font-mono text-chip tabular-nums text-muted">
          {meta.join(' · ')}
        </span>
      </span>
    </>
  );
  const className =
    'now-item flex w-full items-start gap-2.5 px-3.5 py-2.5 text-left no-underline transition-colors duration-150 hover:bg-surface-2 focus-visible:bg-surface-2';
  return (
    <li className="border-t border-line/70 first:border-t-0">
      {row.todoId ? (
        <button
          type="button"
          className={className}
          onClick={() => {
            logUsage('web:now-row', { kind: row.kind });
            void openTodoDetail(row.todoId as string);
          }}
        >
          {body}
        </button>
      ) : row.url ? (
        <a
          className={className}
          href={row.url}
          target="_blank"
          rel="noreferrer"
          onClick={() => logUsage('web:now-row', { kind: row.kind })}
        >
          {body}
        </a>
      ) : (
        <div className={className}>{body}</div>
      )}
    </li>
  );
}

/** 접힌 나머지의 요약 한 줄. 내 차례의 "N 더" 는 누르면 펼친다. */
function MoreLine(props: { row: NowRow; onExpand?: () => void }) {
  const text = (
    <span className="font-mono text-chip font-medium text-muted">{props.row.title}</span>
  );
  return (
    <li className="border-t border-line/70 first:border-t-0">
      {props.onExpand ? (
        <button
          type="button"
          className="w-full px-3.5 py-2 text-left transition-colors duration-150 hover:bg-surface-2"
          onClick={props.onExpand}
        >
          {text}
        </button>
      ) : (
        <div className="px-3.5 py-2">{text}</div>
      )}
    </li>
  );
}
