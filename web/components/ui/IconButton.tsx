import type { ButtonHTMLAttributes, ReactNode } from 'react';

export interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  /** 버튼 내부 아이콘 */
  children: ReactNode;
  /** 스크린리더 및 접근성 라벨 (필수) */
  'aria-label': string;
  /** 버튼 크기 (sm: 28px, md: 32px) */
  size?: 'sm' | 'md';
}

/**
 * 아이콘 전용 버튼 — 일관된 터치 타깃 및 부드러운 호버 피드백.
 */
export function IconButton({ children, size = 'md', className = '', ...props }: IconButtonProps) {
  const sizeClass = size === 'sm' ? 'size-7' : 'size-8';

  return (
    <button
      type="button"
      className={`inline-flex items-center justify-center rounded-md border-0 bg-transparent text-muted transition-colors duration-150 hover:bg-surface-2 hover:text-text focus-visible:outline-2 focus-visible:outline-warm disabled:cursor-not-allowed disabled:opacity-40 ${sizeClass} ${className}`}
      {...props}
    >
      {children}
    </button>
  );
}
