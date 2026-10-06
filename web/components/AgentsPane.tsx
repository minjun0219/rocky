import { Circle, CircleAlert, CircleCheck, CircleDot, type LucideIcon } from 'lucide-react';
import { useEffect, useState } from 'react';
import {
  type AgentPhase,
  type AgentRow,
  agentSections,
  agentSince,
  isSafeShortId,
} from '../agents';
import { copyRefWithFeedback, formatAge } from '../lib';
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
  const background = session.kind === 'background';
  const asleep = background && session.pid === undefined;
  // 세션을 움직이는 일이라 로컬 화면에서만(세션 띄우기와 같은 경계 — 서버도 거절한다). 끝난 세션과 pid 없이 잠든
  // background 세션은 받은편지함 소켓을 들을 프로세스가 없다.
  const local = useUiStore((s) => s.spawnAllowed);
  const canMessage = local && row.phase !== 'done' && !asleep;
  // 짧은 id 는 CLI 출력에서 온 값이라 형식을 본 뒤에만 명령으로 쓴다.
  const shortId = background && session.id && isSafeShortId(session.id) ? session.id : undefined;
  // 멈추기는 살아 있는 background 세션만 — 최종 판정은 데몬(`stop_target`). 잠든 세션의 stop 은 재 보지 못했다.
  const stop = useStopAction(session.sessionId, canMessage && shortId !== undefined);
  const attach = shortId ? <AttachCopy id={shortId} /> : null;
  const extras =
    stop.button || attach ? (
      <>
        {stop.button}
        {attach}
      </>
    ) : null;
  const job = session.job;
  const summary = row.phase === 'blocked' ? (job?.needs ?? job?.detail) : job?.detail;
  const meta = [
    row.place,
    background ? '백그라운드' : null,
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
          <MessageToggle sessionId={session.sessionId} draft={props.draft} onDraft={props.onDraft}>
            {extras}
          </MessageToggle>
        ) : extras ? (
          <div className={ACTIONS_COLUMN}>{extras}</div>
        ) : null}
      </div>
      {stop.below}
    </li>
  );
}

/** 행 오른쪽 버튼 — 메시지 · 멈추기 · attach. 좁은 패널에서는 위아래로 쌓아 글자 칸을 지키고, 넓으면 한 줄로. */
const ACTIONS_COLUMN =
  'flex shrink-0 flex-col items-end gap-1 px-3 py-2.5 sm:flex-row sm:items-center';

/**
 * 멈추기 — 하던 턴이 끊기므로 같은 자리 아래 한 줄로 한 번 더 묻는다(원격 제어의 재시작과 같은 모양, 모달 없음). 대화·
 * 워크트리는 남아 `claude attach` 로 잇는다. 멈추면 행이 목록에서 빠진다.
 */
function useStopAction(sessionId: string, enabled: boolean) {
  const stopSession = useUiStore((s) => s.stopSession);
  const [confirming, setConfirming] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!enabled) {
    return { button: null, below: null };
  }
  const run = async () => {
    setConfirming(false);
    setStopping(true);
    setError(null);
    try {
      await stopSession(sessionId);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setStopping(false);
    }
  };
  const button = stopping ? (
    <span className="text-meta text-muted">멈추는 중…</span>
  ) : (
    <button
      type="button"
      className="drawer-btn"
      aria-expanded={confirming}
      onClick={() => {
        // 지난 실패 문구는 다시 물을 때 지운다 — 취소해도 되살아나지 않게.
        setError(null);
        setConfirming(!confirming);
      }}
    >
      멈추기
    </button>
  );
  const below = confirming ? (
    <div className="flex flex-wrap items-center gap-2 px-3.5 pb-2.5 pl-[42px]">
      <span className="text-chip text-muted">
        하던 일이 끊겨요 — 대화는 남아 claude attach 로 이어요
      </span>
      <button
        type="button"
        className="min-h-8 rounded-md bg-mine-soft px-2.5 text-chip font-semibold text-mine"
        onClick={() => void run()}
      >
        지금 멈추기
      </button>
      <button
        type="button"
        className="tap text-chip text-faint hover:text-text"
        onClick={() => setConfirming(false)}
      >
        취소
      </button>
    </div>
  ) : error ? (
    <p role="status" className="mb-0 mt-0 px-3.5 pb-2.5 pl-[42px] text-chip text-mine">
      {error}
    </p>
  ) : null;
  return { button, below };
}

/**
 * `claude attach <짧은 id>` 를 복사한다 — 출력·답장은 터미널 몫이라 화면은 명령만 건넨다. 복사는 아무것도 움직이지 않아
 * 노출된 화면에도 둔다. pid 없이 잠든 세션(메시지로 못 깨운다)에 답하는 길이 이것이다.
 */
function AttachCopy({ id }: { id: string }) {
  const [copied, setCopied] = useState(false);
  const command = `claude attach ${id}`;
  return (
    <button
      type="button"
      className="drawer-btn"
      title={`${command} 복사`}
      aria-label={copied ? '복사됨' : `${command} 복사`}
      onClick={() => {
        logUsage('web:session-attach-copy');
        void copyRefWithFeedback(command, setCopied);
      }}
    >
      {copied ? '복사됨' : 'attach'}
    </button>
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
  children,
}: {
  sessionId: string;
  draft?: string;
  onDraft: (text: string | undefined) => void;
  /** 입력칸이 닫혀 있을 때 메시지 버튼 밑에 놓을 다른 버튼(멈추기 · attach). */
  children?: React.ReactNode;
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
      <div className={ACTIONS_COLUMN}>
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
        {children}
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
