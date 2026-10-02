export interface TabItem<T extends string = string> {
  id: T;
  label: string;
  count?: number;
  hasDot?: boolean;
}

export interface SegmentTabsProps<T extends string = string> {
  /** 탭 목록 */
  tabs: TabItem<T>[];
  /** 현재 활성화된 탭 ID */
  activeId: T;
  /** 탭 변경 콜백 */
  onChange: (id: T) => void;
  /** 접근성 라벨 */
  'aria-label'?: string;
  className?: string;
}

/**
 * 초슬림 세그먼트 탭 컴포넌트 — 42px 헤더 안에 깔끔하게 들어가는 네비게이션 탭.
 */
export function SegmentTabs<T extends string = string>({
  tabs,
  activeId,
  onChange,
  'aria-label': ariaLabel = '보기 선택',
  className = '',
}: SegmentTabsProps<T>) {
  return (
    <nav
      className={`inline-flex items-center rounded-lg border border-line/70 bg-surface-2/60 p-0.5 text-xs ${className}`}
      aria-label={ariaLabel}
    >
      {tabs.map((tab) => {
        const isActive = tab.id === activeId;
        return (
          <button
            key={tab.id}
            type="button"
            role="tab"
            aria-selected={isActive}
            className={`inline-flex min-h-7 items-center gap-1.5 rounded-md px-2.5 py-1 text-xs transition-all duration-150 ${
              isActive
                ? 'bg-surface font-semibold text-text shadow-sm'
                : 'text-muted hover:text-text'
            }`}
            onClick={() => onChange(tab.id)}
          >
            <span>{tab.label}</span>
            {tab.hasDot ? (
              <span className="size-1.5 rounded-full bg-mine" title="새 내용 있음" />
            ) : null}
            {typeof tab.count === 'number' && tab.count > 0 ? (
              <span className="rounded bg-mine-soft px-1 font-mono text-[11px] font-semibold tabular-nums text-mine">
                {tab.count}
              </span>
            ) : null}
          </button>
        );
      })}
    </nav>
  );
}
