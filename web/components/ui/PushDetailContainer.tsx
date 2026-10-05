import { ArrowLeft, X } from 'lucide-react';
import { type ReactNode, useEffect } from 'react';
import { IconButton } from './IconButton';

export interface PushDetailContainerProps {
  /** 상세 화면 열림 여부 */
  open: boolean;
  /** 상세 화면 닫기 콜백 */
  onClose: () => void;
  /** 상단 헤더 타이틀 또는 ref */
  headerTitle?: string;
  /** 본문 컨텐츠 */
  children: ReactNode;
  /** 데스크톱 2열 모드 여부 (화면 폭이 넓은 경우 true) */
  isDesktopSplit?: boolean;
  className?: string;
}

/**
 * 스마트 적응형 상세 화면 셸 —
 * 좁은 화면(< 720px)에서는 Push 슬라이드로 전환되고,
 * 데스크톱(>= 720px)에서는 우측 분할 열로 매끄럽게 고정됩니다.
 */
export function PushDetailContainer({
  open,
  onClose,
  headerTitle,
  children,
  isDesktopSplit = false,
  className = '',
}: PushDetailContainerProps) {
  useEffect(() => {
    if (!open) {
      return;
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onClose();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open, onClose]);

  // 데스크톱 2열 분할 모드
  if (isDesktopSplit) {
    if (!open) {
      return (
        <aside
          className={`hidden flex-1 flex-col items-center justify-center border-l border-line p-8 text-center text-sm text-faint md:flex ${className}`}
        >
          <p>할 일을 고르면 여기에 상세가 보여요</p>
        </aside>
      );
    }

    return (
      <aside
        className={`flex flex-1 flex-col border-l border-line bg-surface ${className}`}
        aria-label="할 일 상세"
      >
        <header className="flex h-11 items-center justify-between border-b border-line px-4">
          <span className="font-mono text-chip text-muted">{headerTitle}</span>
          <IconButton size="sm" aria-label="상세 닫기" onClick={onClose}>
            <X size={15} aria-hidden />
          </IconButton>
        </header>
        <div className="flex-1 overflow-y-auto p-5">{children}</div>
      </aside>
    );
  }

  // 모바일 & cmux 좁은 폭 (Push 슬라이드 오버레이)
  return (
    <section
      aria-label="할 일 상세"
      className={`fixed inset-0 z-30 flex flex-col bg-surface transition-transform duration-250 ease-out md:static ${
        open ? 'translate-x-0' : 'pointer-events-none translate-x-full'
      } ${className}`}
      aria-hidden={!open}
    >
      <header className="flex h-11 items-center justify-between border-b border-line px-3">
        <button
          type="button"
          className="inline-flex items-center gap-1.5 text-sm font-medium text-muted hover:text-text"
          onClick={onClose}
        >
          <ArrowLeft size={16} aria-hidden />
          <span>뒤로</span>
        </button>
        <span className="font-mono text-chip text-muted">{headerTitle}</span>
        <IconButton size="sm" aria-label="상세 닫기" onClick={onClose}>
          <X size={15} aria-hidden />
        </IconButton>
      </header>
      <div className="flex-1 overflow-y-auto p-4">{children}</div>
    </section>
  );
}
