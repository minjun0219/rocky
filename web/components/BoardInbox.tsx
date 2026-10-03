import { ArrowUpRight } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import { githubHideKey } from '../lib';
import { api, useUiStore } from '../store';
import { HideButton } from './HideButton';
import type { BoardInboxSource, InboxAdapter, InboxSourceResult } from '../types';

/**
 * 이 보드의 수집함 — 아직 안 올린 외부 항목 목록과, 보드마다 수집함을 등록하는 설정.
 *
 * 무엇을 실행할지는 `rocky.json` 의 `todo.inboxAdapters[]` 가, 무엇을 거를지는 여기서 정한다 — 폼은
 * 어댑터가 `--describe` 로 알려 준 칸만 그리고, 그 칸만 서버로 보낸다. 설정 쓰기는 로컬에서 연 화면만
 * 된다(서버가 403 으로 막는다). 수집함은 보드를 볼 때만 부르는 외부 호출이라 SSE 로 따라가지 않고,
 * 보드를 열 때와 "새로고침" 때만 읽는다.
 */
export function BoardInbox({ board }: { board: string }) {
  const actor = useUiStore((s) => s.actor);
  const [sources, setSources] = useState<InboxSourceResult[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [settings, setSettings] = useState(false);
  // 소스 이름 → 구독한 세션 수. 세션이 `rocky inbox subscribe` 로 구독하면 새 항목이 그 세션에 간다.
  const [subscribed, setSubscribed] = useState<Record<string, number>>({});

  const load = useCallback(
    async (refresh: boolean) => {
      setError(null);
      try {
        const query = `board=${encodeURIComponent(board)}${refresh ? '&refresh=true' : ''}`;
        const res = await api<{ sources: InboxSourceResult[] }>(`/api/inbox?${query}`, actor);
        setSources(res.sources);
        const subs = await api<{ source: string }[]>('/api/inbox/subscriptions', actor).catch(
          () => [],
        );
        // 이 보드에 보이는 소스의 구독만 센다 — 다른 보드 전용 소스의 구독은 여기 이야기가 아니다.
        const here = new Set(res.sources.map((s) => s.name));
        const counts: Record<string, number> = {};
        for (const s of subs.filter((s) => here.has(s.source))) {
          counts[s.source] = (counts[s.source] ?? 0) + 1;
        }
        setSubscribed(counts);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    },
    [board, actor],
  );

  useEffect(() => {
    void load(false);
  }, [load]);

  const hidden = useUiStore((s) => s.githubHidden);
  const hide = useUiStore((s) => s.hideGithub);
  const open = (sources ?? []).flatMap((s) =>
    s.available
      ? s.items
          .filter((i) => !i.promoted)
          .filter(
            (i) => !hidden.includes(githubHideKey({ kind: 'inbox', source: s.name, id: i.id })),
          )
          .map((i) => ({ source: s.name, item: i }))
      : [],
  );
  const failed = (sources ?? []).filter((s) => !s.available);

  return (
    <section className="mb-[26px]" aria-label="수집함">
      <div className="mb-1.5 flex items-baseline gap-2 border-b border-line pb-[5px]">
        <span className="font-mono text-chip text-muted">수집함 {open.length}</span>
        <div className="flex-1" />
        <button
          type="button"
          className="tap text-chip text-faint hover:text-text"
          onClick={() => void load(true)}
        >
          새로고침
        </button>
        <button
          type="button"
          className="tap text-chip text-faint hover:text-text"
          aria-expanded={settings}
          onClick={() => setSettings(!settings)}
        >
          설정
        </button>
      </div>
      {error && (
        <p className="m-0 text-sm text-dead" role="alert">
          수집함을 읽지 못했다 — {error}
        </p>
      )}
      {Object.keys(subscribed).length > 0 && (
        <p className="m-0 mb-1 text-meta text-faint">
          {Object.entries(subscribed)
            .map(([name, n]) => `${name} — 세션 ${n}곳이 구독 중(새 항목을 알림)`)
            .join(' · ')}
        </p>
      )}
      {failed.map((s) => (
        <p key={s.name} className="m-0 truncate text-meta text-dead">
          {s.name} — 실패: {(s.reason ?? '사유 없음').split('\n')[0]}
        </p>
      ))}
      <ul className="m-0 list-none p-0">
        {open.map(({ source, item }) => (
          <li key={`${source}:${item.id}`} className="flex min-h-10 items-center gap-2 py-1">
            <span className="shrink-0 font-mono text-chip text-faint">{source}</span>
            {item.url ? (
              <a
                className="min-w-0 flex-1 truncate text-sm text-link"
                href={item.url}
                target="_blank"
                rel="noreferrer"
              >
                {item.title} <ArrowUpRight size={11} aria-hidden className="inline align-[-1px]" />
              </a>
            ) : (
              <span className="min-w-0 flex-1 truncate text-sm">{item.title}</span>
            )}
            <HideButton
              label={`${item.title} 숨기기`}
              onClick={() => hide(githubHideKey({ kind: 'inbox', source, id: item.id }))}
            />
          </li>
        ))}
      </ul>
      {settings && <InboxSettings board={board} onChanged={() => void load(true)} />}
    </section>
  );
}

/** 보드 수집함 설정 — 등록된 것 목록(지우기) + 어댑터를 골라 칸을 채워 등록. */
export function InboxSettings({ board, onChanged }: { board: string; onChanged: () => void }) {
  const actor = useUiStore((s) => s.actor);
  const [registered, setRegistered] = useState<BoardInboxSource[]>([]);
  const [adapters, setAdapters] = useState<InboxAdapter[] | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [adapterName, setAdapterName] = useState('');
  const [name, setName] = useState('');
  const [values, setValues] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);

  const reload = useCallback(async () => {
    setRegistered(
      await api<BoardInboxSource[]>(`/api/inbox/sources?board=${encodeURIComponent(board)}`, actor),
    );
  }, [board, actor]);

  useEffect(() => {
    void reload().catch((e: unknown) => setNotice(e instanceof Error ? e.message : String(e)));
    api<InboxAdapter[]>('/api/inbox/adapters', actor)
      .then((list) => {
        setAdapters(list);
        setAdapterName(list.find((a) => !a.error)?.name ?? '');
      })
      // 노출된 화면(테일넷·터널)에서는 403 — 사유를 그대로 보여 주고 폼은 그리지 않는다.
      .catch((e: unknown) => {
        setAdapters([]);
        setNotice(e instanceof Error ? e.message : String(e));
      });
  }, [reload, actor]);

  const adapter = adapters?.find((a) => a.name === adapterName);

  const submit = async (): Promise<void> => {
    if (busy || !adapter) {
      return;
    }
    setBusy(true);
    setNotice(null);
    try {
      await api('/api/inbox/sources', actor, {
        method: 'POST',
        body: JSON.stringify({ board, name: name.trim(), adapter: adapter.name, params: values }),
      });
      setName('');
      setValues({});
      await reload();
      onChanged();
    } catch (e) {
      setNotice(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const remove = async (id: string): Promise<void> => {
    setNotice(null);
    try {
      await api(`/api/inbox/sources/${encodeURIComponent(id)}`, actor, { method: 'DELETE' });
      await reload();
      onChanged();
    } catch (e) {
      setNotice(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="mt-2 rounded-lg border border-line bg-surface px-3.5 py-3">
      <ul className="m-0 mb-2 list-none p-0">
        {registered.map((s) => (
          <li key={s.id} className="flex items-center gap-2 py-1 text-meta">
            <span className="font-mono">{s.name}</span>
            <span className="min-w-0 flex-1 truncate text-muted">
              {s.adapterMissing ? `${s.adapter} (설정에 없는 어댑터)` : s.adapter} ·{' '}
              {s.params.map((p) => p.value).join(' · ')}
            </span>
            <button type="button" className="drawer-btn" onClick={() => void remove(s.id)}>
              지우기
            </button>
          </li>
        ))}
        {registered.length === 0 && (
          <li className="text-meta text-faint">이 보드에 등록한 수집함이 없다.</li>
        )}
      </ul>
      {adapters && adapters.length === 0 && !notice && (
        <p className="m-0 text-meta text-faint">
          rocky.json 의 todo.inboxAdapters[] 에 어댑터를 등록하면 여기서 보드마다 조건을 채운다.
        </p>
      )}
      {adapters && adapters.length > 0 && (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void submit();
          }}
        >
          <Field label="어댑터">
            <select
              className="board-edit-input min-w-0 flex-1 rounded-md border border-line bg-bg px-[9px] py-[5px] text-sm text-text"
              value={adapterName}
              onChange={(e) => {
                setAdapterName(e.target.value);
                setValues({});
              }}
            >
              {adapters.map((a) => (
                <option key={a.name} value={a.name} disabled={Boolean(a.error)}>
                  {a.title ?? a.name}
                  {a.error ? ` — ${a.error}` : ''}
                </option>
              ))}
            </select>
          </Field>
          <Field label="이름">
            <input
              className="board-edit-input min-w-0 flex-1 rounded-md border border-line bg-bg px-[9px] py-[5px] text-sm text-text placeholder:text-faint"
              value={name}
              placeholder="소문자·숫자·- (예: gh-bugs)"
              onChange={(e) => setName(e.target.value)}
            />
          </Field>
          {adapter?.params?.map((p) => (
            <Field key={p.flag} label={p.label}>
              <input
                className="board-edit-input min-w-0 flex-1 rounded-md border border-line bg-bg px-[9px] py-[5px] text-sm text-text placeholder:text-faint"
                value={values[p.flag] ?? ''}
                placeholder={p.placeholder}
                required={p.required}
                onChange={(e) => setValues({ ...values, [p.flag]: e.target.value })}
              />
            </Field>
          ))}
          <div className="drawer-actions">
            <button type="submit" className="drawer-btn" disabled={busy || !adapter}>
              {busy ? '등록 중…' : '등록'}
            </button>
          </div>
        </form>
      )}
      {notice && (
        <div className="board-add-error" role="alert">
          {notice}
        </div>
      )}
    </div>
  );
}

/** 칸 한 줄 — `children` 은 늘 input·select 하나다(라벨이 감싸서 연결된다). */
function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    // biome-ignore lint/a11y/noLabelWithoutControl: children 이 늘 input·select 라 감싸는 것으로 연결된다
    <label className="mb-1.5 flex items-center gap-2.5">
      <span className="w-16 shrink-0 text-chip text-faint">{label}</span>
      {children}
    </label>
  );
}
