# 웹 UI 리디자인 사양 (A안: 미니멀 관제판)

> **현행 여부(2026-10-07)**: 구현됐다(계획 `docs/design/plans/2026-10-03-web-ui-redesign-plan.md`). 지금 규칙의 정본은 `web/DESIGN.md`.

> **상태**: Draft  
> **날짜**: 2026-10-02  
> **목적**: 좁은 cmux 사이드 독(280~420px), 모바일, PC 데스크톱 전반에서 사용성과 시각적 완성도를 극대화하는 UI/UX 개편

---

## 1. 개요 및 배경

기존 웹 UI는 세로 3단 누적 헤더(`TopBar` + `NowTable` + `ViewSwitch`)로 인해 화면 상단 100px 이상이 가려져 정작 중요한 본문 뷰포트가 협소했습니다. 또한 999px 알약형 태그, 불필요한 테두리, 모달 드로어의 부자연스러운 전환 등으로 인해 사용성이 투박했습니다.

본 리디자인은 Linear 및 Things 스타일의 정갈한 미니멀리즘(A안)을 바탕으로 하며, 기존 작업 중인 코드와의 충돌을 피하기 위해 `web/components/ui/`에 신규 UI 킷을 구축하고 머지 후 점진적으로 연결합니다.

---

## 2. 반응형 레이아웃 아키텍처

| 환경 | 화면 폭 | 핵심 레이아웃 사양 |
|---|---|---|
| **모바일** | ~360px | • 100% 풀스크린 핏 (iOS Safe-area 대응)<br>• 1열 목록 + 작업 터치 시 부드러운 Push 슬라이드 전환 |
| **cmux 사이드 독** | 360 ~ 480px | • 42px 초슬림 일체형 헤더로 세로 공간 확보<br>• 1열 콤팩트 카드 관제판 (내 차례 → 진행 중 → 목록) |
| **PC 데스크톱** | 720px 이상 | • 2열 스플릿 뷰 (Master-Detail)<br>• 좌측 목록(420px) + 우측 실시간 작업 상세 패널 나란히 배치<br>• 연속 키보드 탐색 및 ESC 닫기 지원 |

---

## 3. 구축된 신규 UI 킷 (`web/components/ui/`)

- `CompactHeader.tsx`: 42px 초슬림 일체형 헤더 (보드 선택 + 세그먼트 탭 + 에이전트 인디케이터 + 메뉴)
- `SegmentTabs.tsx`: 피드 / 할 일 / 노트 전환 세그먼트 컨트롤
- `TaskRow.tsx`: Linear 감성의 작업 행 (체크박스, 4px 마이크로 뱃지, 경과 시간, PC 선택 하이라이트)
- `LiveAgentCard.tsx`: 진행 중인 에이전트 실시간 관제 스트립 (초록 펄스 점, 세션명, 경과 시간)
- `AttentionCard.tsx`: '내 차례' 주목 필요 카드 및 그룹
- `PushDetailContainer.tsx`: 스마트 적응형 상세 컨테이너 (좁은 폭 Push ↔ PC 2열 스플릿)
- `Badge.tsx` & `IconButton.tsx`: 4px 모서리 마이크로 뱃지 및 표준 터치 타깃 버튼
- `ui.test.tsx`: UI 킷 8개 단위 테스트 (전체 통과)

---

## 4. 단계별 적용 계획

1. **Phase 1 (본 PR)**: UI 킷 및 디자인 스펙 구축, 프로토타입 검증
2. **Phase 2 (머지 후)**: 기존 `TopBar`, `ViewSwitch`, `NowTable`을 `CompactHeader` 기반으로 통합
3. **Phase 3**: `TodoItem` 및 `TodoPane`을 `TaskRow` 및 2열 스플릿 뷰로 전환
4. **Phase 4**: 회귀 테스트 및 최종 정리
