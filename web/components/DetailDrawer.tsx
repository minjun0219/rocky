import * as Dialog from '@radix-ui/react-dialog';
import { X } from 'lucide-react';
import { isEditableTarget } from '../lib';
import { useEffect, useState } from 'react';
import { api, useUiStore } from '../store';
import type { WorklogEntry } from '../types';
import { NoteDetail } from './drawer/NoteDetail';
import { CommentComposer, Timeline } from './drawer/Timeline';
import { TodoDetail } from './drawer/TodoDetail';

/**
 * 우측 상세 드로어 — todo/note 상세 + 상태 버튼 + 히스토리 타임라인.
 *
 * 셸은 Radix Dialog 다. 수제 셸에서 넘긴 것: Esc(편집 중 가드는 유지)·백드롭 탭·
 * 배경 스크롤 잠금, 그리고 수제로는 없던 **포커스 트랩과 복원** — 이게 전환의 이유다.
 * 시각은 기존 클래스(.drawer-backdrop/.drawer)를 그대로 쓴다.
 */
export function DetailDrawer() {
  const detail = useUiStore((s) => s.detail);
  const actor = useUiStore((s) => s.actor);
  // 이 할 일을 들고 일한 세션의 턴 기록 — 로그 색인(`todo:<ref>` 태그). 없거나 실패하면 그냥 없다.
  const todoRef = detail?.kind === 'todo' ? detail.todo?.ref : undefined;
  const [work, setWork] = useState<WorklogEntry[]>([]);
  useEffect(() => {
    setWork([]);
    if (!todoRef) {
      return;
    }
    let live = true;
    api<{ entries: WorklogEntry[] }>(
      `/api/logs/worklog?todo=${encodeURIComponent(todoRef)}&limit=30`,
      actor,
    )
      .then((body) => {
        if (live) {
          setWork(body.entries ?? []);
        }
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [todoRef, actor]);
  const closeDetail = useUiStore((s) => s.closeDetail);

  if (!detail) {
    return null;
  }
  return (
    <Dialog.Root
      open
      onOpenChange={(open) => {
        if (!open) {
          closeDetail();
        }
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="drawer-backdrop" />
        <Dialog.Content
          className="drawer"
          aria-describedby={undefined}
          onEscapeKeyDown={(e) => {
            // 입력 중인 Esc 는 그 입력의 취소지 드로어 닫기가 아니다 — 제목 편집뿐
            // 아니라 설명 편집기(CodeMirror — contenteditable)에서도 편집분이 날아가지 않게 걸러 낸다.
            if (isEditableTarget(e.target)) {
              e.preventDefault();
            }
          }}
        >
          <Dialog.Title className="sr-only">
            {detail.kind === 'todo' ? '할 일 상세' : '메모 상세'}
          </Dialog.Title>
          <Dialog.Close className="drawer-close" aria-label="상세 닫기">
            <X size={16} aria-hidden />
          </Dialog.Close>
          {detail.kind === 'todo' ? <TodoDetail /> : <NoteDetail />}
          {detail.kind === 'todo' && detail.todo && <CommentComposer todoId={detail.todo.id} />}
          <Timeline history={detail.history} comments={detail.comments} work={work} />
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
