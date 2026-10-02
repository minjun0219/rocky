import type { LucideIcon } from 'lucide-react';
import type { ReactNode } from 'react';

export interface AttentionItemProps {
  /** 항목 제목 */
  title: string;
  /** 부가 메타 (예: 'web-ui · 32분 전', '피드백 대기') */
  meta: string;
  /** 좌측 상태 아이콘 */
  icon?: LucideIcon;
  /** 클릭 시 호출될 콜백 */
  onClick?: () => void;
  /** 우측 추가 액션 버튼 (숨기기 등) */
  action?: ReactNode;
}

/**
 * 개별 주목 필요 항목 행
 */
export function AttentionItem({ title, meta, icon: Icon, onClick, action }: AttentionItemProps) {
  return (
    <div className="flex items-start justify-between gap-2 border-b border-mine/15 py-2 last:border-b-0">
      <button
        type="button"
        className="flex min-w-0 flex-1 cursor-pointer items-start gap-2.5 border-0 bg-transparent p-0 text-left"
        onClick={onClick}
      >
        {Icon ? (
          <span className="mt-0.5 flex shrink-0 text-mine">
            <Icon size={15} strokeWidth={2.25} aria-hidden />
          </span>
        ) : null}
        <div className="min-w-0 flex-1">
          <p className="text-sm font-medium leading-snug text-text hover:text-mine">{title}</p>
          <p className="mt-0.5 font-mono text-chip text-mine">{meta}</p>
        </div>
      </button>
      {action}
    </div>
  );
}

export interface AttentionGroupProps {
  children: ReactNode;
  className?: string;
}

/**
 * '내 차례' 주목 필요 카드 그룹 래퍼
 */
export function AttentionGroup({ children, className = '' }: AttentionGroupProps) {
  return (
    <div className={`rounded-lg border border-mine/25 bg-mine-soft/60 px-3 py-1 ${className}`}>
      {children}
    </div>
  );
}
