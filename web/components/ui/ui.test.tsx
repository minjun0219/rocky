import { describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen } from '@testing-library/react';
import {
  AttentionGroup,
  AttentionItem,
  Badge,
  CompactHeader,
  IconButton,
  LiveAgentCard,
  PushDetailContainer,
  SegmentTabs,
  TaskRow,
} from './index';

describe('로키 웹 리디자인 UI 킷 컴포넌트 테스트', () => {
  it('Badge가 텍스트와 톤 스타일을 올바르게 렌더링한다', () => {
    const { rerender } = render(<Badge tone="mine">내 차례</Badge>);
    expect(screen.getByText('내 차례')).toBeDefined();

    rerender(<Badge tone="run">돌고 있음</Badge>);
    expect(screen.getByText('돌고 있음')).toBeDefined();
  });

  it('IconButton이 클릭 시 핸들러를 호출하고 접근성 라벨을 제공한다', () => {
    const onClick = mock();
    render(
      <IconButton aria-label="메뉴 열기" onClick={onClick}>
        <span>아이콘</span>
      </IconButton>,
    );

    const btn = screen.getByLabelText('메뉴 열기');
    expect(btn).toBeDefined();

    fireEvent.click(btn);
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it('SegmentTabs가 탭 목록과 개수를 표시하고 탭 변경을 처리한다', () => {
    const onChange = mock();
    const tabs = [
      { id: 'feed', label: '피드', count: 3 },
      { id: 'tasks', label: '할 일' },
    ];

    render(<SegmentTabs tabs={tabs} activeId="feed" onChange={onChange} />);

    expect(screen.getByText('피드')).toBeDefined();
    expect(screen.getByText('3')).toBeDefined();
    expect(screen.getByText('할 일')).toBeDefined();

    fireEvent.click(screen.getByText('할 일'));
    expect(onChange).toHaveBeenCalledWith('tasks');
  });

  it('LiveAgentCard가 제목과 경과 시간을 렌더링하고 클릭 시 콜백을 실행한다', () => {
    const onClick = mock();
    render(
      <LiveAgentCard title="웹 UI 리팩토링" actor="CLAUDE" elapsed="03:45" onClick={onClick} />,
    );

    expect(screen.getByText('웹 UI 리팩토링')).toBeDefined();
    expect(screen.getByText('CLAUDE')).toBeDefined();
    expect(screen.getByText('03:45')).toBeDefined();

    fireEvent.click(screen.getByText('웹 UI 리팩토링'));
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it('AttentionCard가 주목 항목과 메타 정보를 렌더링한다', () => {
    const onClick = mock();
    render(
      <AttentionGroup>
        <AttentionItem title="PR #42 머지 충돌" meta="30분 경과" onClick={onClick} />
      </AttentionGroup>,
    );

    expect(screen.getByText('PR #42 머지 충돌')).toBeDefined();
    expect(screen.getByText('30분 경과')).toBeDefined();

    fireEvent.click(screen.getByText('PR #42 머지 충돌'));
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it('TaskRow가 작업 완료 여부와 클릭 및 체크박스 토글을 올바르게 처리한다', () => {
    const onToggleDone = mock();
    const onClick = mock();

    const { rerender } = render(
      <TaskRow
        title="디자인 시스템 구축"
        refNumber="rocky-101"
        done={false}
        onToggleDone={onToggleDone}
        onClick={onClick}
      />,
    );

    expect(screen.getByText('디자인 시스템 구축')).toBeDefined();
    expect(screen.getByText('rocky-101')).toBeDefined();

    // 체크박스 클릭
    const checkbox = screen.getByRole('checkbox');
    fireEvent.click(checkbox);
    expect(onToggleDone).toHaveBeenCalledTimes(1);

    // 행 클릭
    fireEvent.click(screen.getByText('디자인 시스템 구축'));
    expect(onClick).toHaveBeenCalledTimes(1);

    // 완료 상태 렌더링 확인
    rerender(
      <TaskRow
        title="디자인 시스템 구축"
        done={true}
        onToggleDone={onToggleDone}
        onClick={onClick}
      />,
    );
    expect((screen.getByRole('checkbox') as HTMLInputElement).checked).toBe(true);
  });

  it('PushDetailContainer가 닫기 버튼과 ESC 키에 반응한다', () => {
    const onClose = mock();
    render(
      <PushDetailContainer open={true} onClose={onClose} headerTitle="상세 보기">
        <div>상세 본문 내용</div>
      </PushDetailContainer>,
    );

    expect(screen.getByText('상세 보기')).toBeDefined();
    expect(screen.getByText('상세 본문 내용')).toBeDefined();

    // 닫기 버튼 클릭
    fireEvent.click(screen.getByLabelText('상세 닫기'));
    expect(onClose).toHaveBeenCalledTimes(1);

    // ESC 키 입력
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it('CompactHeader가 슬롯 요소들을 정상 배치한다', () => {
    render(
      <CompactHeader
        leftSlot={<div>보드이름</div>}
        centerSlot={<div>세그먼트탭</div>}
        rightSlot={<div>메뉴버튼</div>}
      />,
    );

    expect(screen.getByText('보드이름')).toBeDefined();
    expect(screen.getByText('세그먼트탭')).toBeDefined();
    expect(screen.getByText('메뉴버튼')).toBeDefined();
  });
});
