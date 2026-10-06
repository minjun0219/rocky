import { Circle, CircleAlert, CircleCheck, CircleDot, type LucideIcon } from 'lucide-react';
import { useEffect, useState } from 'react';
import { type AgentPhase, type AgentRow, agentSections, agentSince } from '../agents';
import { formatAge } from '../lib';
import { useUiStore } from '../store';
import { logUsage } from '../usage';
import { StateIcon, useNow } from './NowTable';

/** 다시 읽는 간격 — 탭을 보는 동안은 짧게, 아니면 피드의 "내 차례" 몫으로 길게. 데몬이 몇 초 캐시한다. */
const POLL_MS = 15_000;
const IDLE_POLL_MS = 60_000;

/** 상태 아이콘 — `web/DESIGN.md` "State Vocabulary". 사람 답을 기다리는 것만 `mine` 이다. */
const PHASE_ICON: Record<AgentPhase, { Icon: LucideIcon; className: string; label: string }> = {
  blocked: { Icon: CircleAlert, className: 'text-mine', label: '내 차례' },
  run: { Icon: CircleDot, className: 'text-run', label: '실행 중' },
  idle: { Icon: Circle, className: 'text-faint', label: '쉬는 중' },
  done: { Icon: CircleCheck, className: 'text-faint', label: '끝남' },
};

/**
 * 세션 목록 폴링 — 앱 한 곳(`main.tsx`)에서 돈다. 탭을 끄면 돌지 않는다(피드의 에이전트 행도 같이 사라진다).
 * `claude agents --json` 은 데몬이 돌리는 CLI 라 탭 밖에서는 1분에 한 번이면 된다.
 */
export function useAgentsPolling() {
  const enabled = useUiStore((s) => s.showAgents);
  const watching = useUiStore((s) => s.view === 'agents');
  const loadAgents = useUiStore((s) => s.loadAgents);
  useEffect(() => {
    if (!enabled) {
      return;
    }
    void loadAgents();
    const id = setInterval(() => void loadAgents(), watching ? POLL_MS : IDLE_POLL_MS);
    return () => clearInterval(id);
  }, [enabled, watching, loadAgents]);
}

/**
 * 에이전트 — `claude agents` 가 보는 세션(interactive · background)을 내 차례 → 실행 중 → 쉬는 중으로. 보드를 고르면
 * 그 보드의 세션만. 출력·로그는 두지 않고 Claude 가 남긴 요약 한 줄만(`web/DESIGN.md` "Not in the Panel").
 */
export function AgentsPane() {
  const agents = useUiStore((s) => s.agents);
  const boards = useUiStore((s) => s.boards);
  const selected = useUiStore((s) => s.selected);
  const doing = useUiStore((s) => s.nowTodos);
  const sections = agents?.available
    ? agentSections({ sessions: agents.list, boards, selected, doing })
    : [];
  const now = useNow([], sections.length);
  // 쓰던 메시지는 세션 id 로 여기 둔다 — 행이 다른 묶음으로 옮겨 가면(실행 중 → 쉬는 중) 다시 마운트돼 입력칸이 사라지므로.
  const [drafts, setDrafts] = useState<Record<string, string | undefined>>({});
  const setDraft = (sessionId: string, text: string | undefined) =>
    setDrafts((prev) => ({ ...prev, [sessionId]: text }));
  return (
    <main className="min-w-0 flex-1 overflow-y-auto px-4 pb-6 pt-1" aria-label="에이전트">
      {agents === null ? null : !agents.available ? (
        <p className="mt-3 text-meta text-muted" title={agents.reason}>
          세션 목록을 읽지 못했어요{agents.reason ? ` — ${agents.reason}` : ''}
        </p>
      ) : sections.length === 0 ? (
        <p className="mt-3 text-meta text-muted">
          {selected === 'all' ? '떠 있는 에이전트가 없어요' : '이 보드에서 도는 에이전트가 없어요'}
        </p>
      ) : (
        sections.map((section) => (
          <section key={section.group} aria-label={section.title}>
            <h2 className="m-0 mb-2 mt-[18px] flex items-baseline gap-2 font-mono text-chip font-medium text-faint">
              {section.title}
              <span
                className={`rounded-[4px] px-1.5 py-0.2 font-mono text-chip font-semibold tabular-nums ${
                  section.group === 'mine'
                    ? 'bg-mine-soft text-mine'
                    : section.group === 'run'
                      ? 'bg-run-soft text-run'
                      : 'text-faint'
                }`}
              >
                {section.rows.length}
              </span>
            </h2>
            <ul className="m-0 list-none overflow-hidden rounded-lg border border-line bg-surface p-0 shadow-xs">
              {section.rows.map((row) => (
                <AgentItem
                  key={row.session.sessionId}
                  row={row}
                  now={now}
                  draft={drafts[row.session.sessionId]}
                  onDraft={(text) => setDraft(row.session.sessionId, text)}
                />
              ))}
            </ul>
          </section>
        ))
      )}
    </main>
  );
}

/**
 * 한 행 — 첫 줄: 아이콘 + 세션 이름, 둘째 줄: 어디서 · 백그라운드 · 시각 · 든 할 일, 셋째 줄: 기다리는 것(내 차례)
 * 또는 지금 하는 일. 할 일을 든 세션이면 행을 눌러 그 할 일을 연다.
 */
function AgentItem(props: {
  row: AgentRow;
  now: number;
  /** 쓰던 메시지 — undefined 면 입력칸이 닫혀 있다. */
  draft?: string;
  onDraft: (text: string | undefined) => void;
}) {
  const { row, now } = props;
  const openTodoDetail = useUiStore((s) => s.openTodoDetail);
  const { session, todo } = row;
  // 세션을 움직이는 일이라 로컬 화면에서만(세션 띄우기와 같은 경계 — 서버도 거절한다). 끝난 세션과 pid 없이 잠든
  // background 세션은 받은편지함 소켓을 들을 프로세스가 없다.
  const canMessage =
    useUiStore((s) => s.spawnAllowed) &&
    row.phase !== 'done' &&
    !(session.kind === 'background' && session.pid === undefined);
  const job = session.job;
  const summary = row.phase === 'blocked' ? (job?.needs ?? job?.detail) : job?.detail;
  const meta = [
    row.place,
    session.kind === 'background' ? '백그라운드' : null,
    formatAge(agentSince(row), now, { since: row.phase !== 'blocked' }),
    row.phase === 'done' ? '끝남' : null,
    todo?.ref,
  ].filter(Boolean);
  const body = (
    <>
      <StateIcon {...PHASE_ICON[row.phase]} />
      <span className="min-w-0 flex-1">
        <span className="now-title block text-sm leading-[1.45] text-text">{session.name}</span>
        <span className="mt-0.5 block truncate font-mono text-chip tabular-nums text-muted">
          {meta.join(' · ')}
        </span>
        {summary ? (
          <span className="mt-1 line-clamp-2 block text-meta text-muted">{summary}</span>
        ) : null}
      </span>
    </>
  );
  const className = 'flex min-w-0 flex-1 items-start gap-2.5 px-3.5 py-2.5 text-left';
  return (
    <li className="border-t border-line/70 first:border-t-0" title={session.cwd}>
      <div className="flex items-start">
        {todo ? (
          <button
            type="button"
            className={`${className} transition-colors duration-150 hover:bg-surface-2 focus-visible:bg-surface-2`}
            onClick={() => {
              logUsage('web:agent-row');
              void openTodoDetail(todo.id);
            }}
          >
            {body}
          </button>
        ) : (
          <div className={className}>{body}</div>
        )}
        {canMessage ? (
          <MessageToggle
            sessionId={session.sessionId}
            draft={props.draft}
            onDraft={props.onDraft}
          />
        ) : null}
      </div>
    </li>
  );
}

/**
 * 세션에 한 줄 보내기 — 받은편지함으로 들어가 쉬는 세션이면 턴이 열린다. 받는 쪽에는 "다른 세션이 보낸 메시지" 로
 * 보여 사용자 승인으로 쓰이지 않는다(그 사실을 입력칸 옆에 밝힌다). ⌘/Ctrl+Enter 로 보낸다.
 */
function MessageToggle({
  sessionId,
  draft,
  onDraft,
}: {
  sessionId: string;
  draft?: string;
  onDraft: (text: string | undefined) => void;
}) {
  const send = useUiStore((s) => s.sendSessionMessage);
  const open = draft !== undefined;
  const text = draft ?? '';
  const [sending, setSending] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);
  const submit = async () => {
    const body = text.trim();
    if (!body || sending) {
      return;
    }
    setSending(true);
    try {
      await send(sessionId, body);
      onDraft(undefined);
      setResult({ ok: true, text: '보냈어요' });
    } catch (error) {
      setResult({ ok: false, text: error instanceof Error ? error.message : String(error) });
    } finally {
      setSending(false);
    }
  };
  if (!open) {
    return (
      <div className="flex shrink-0 flex-col items-end gap-1 px-3 py-2.5">
        <button
          type="button"
          className="drawer-btn"
          onClick={() => {
            setResult(null);
            onDraft('');
          }}
        >
          메시지
        </button>
        {result ? (
          <span role="status" className={`text-meta ${result.ok ? 'text-muted' : 'text-p1'}`}>
            {result.text}
          </span>
        ) : null}
      </div>
    );
  }
  return (
    <form
      className="flex w-[min(320px,55%)] shrink-0 flex-col gap-1.5 px-3 py-2.5"
      onSubmit={(e) => {
        e.preventDefault();
        void submit();
      }}
    >
      <textarea
        aria-label="보낼 메시지"
        rows={2}
        className="w-full min-w-0 rounded-md border border-line bg-surface px-2 py-1.5 text-sm text-text"
        value={text}
        onChange={(e) => onDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            void submit();
          }
        }}
      />
      <div className="flex flex-wrap items-center gap-1.5">
        <button type="submit" className="drawer-btn" disabled={sending || text.trim() === ''}>
          보내기
        </button>
        <button type="button" className="drawer-btn" onClick={() => onDraft(undefined)}>
          취소
        </button>
      </div>
      <span className="text-meta text-faint">
        받는 세션에는 다른 세션의 메시지로 보여요 — 권한 허락·결정 답으로는 쓰이지 않아요
      </span>
      {result && !result.ok ? (
        <span role="status" className="text-meta text-p1">
          {result.text}
        </span>
      ) : null}
    </form>
  );
}
