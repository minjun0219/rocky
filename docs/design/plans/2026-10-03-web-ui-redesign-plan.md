# 로키(rocky) 웹 UI 리디자인 상세 실행 계획 (A안: 미니멀 관제판)

> **날짜**: 2026-10-03  
> **상태**: 구현 완료 (Phase 1~5 전체 완료)  
> **목적**: Pretendard Variable 한글 폰트 적용, 12px 가독성 확보, 5개 탭 지원, 모바일·cmux·PC 전천후 레이아웃 단계별 이식

---

## 1. 개요 및 설계 원칙

본 계획은 인수인계된 핸드오프 문서(`HANDOFF-antigravity.md`)와 오너의 피드백을 충실히 반영합니다:
1. **한글 폰트**: **Pretendard Variable (프리텐다드 가변 폰트)**를 전면 적용하여 선명한 가독성과 미려한 타이포그래피 구현.
2. **가독성 개선 (12px 미만 전면 제거)**: 11px `micro` 텍스트를 제거하고 최소 12px 크기를 보장하여 곁눈 관제 시 시인성 대폭 향상.
3. **최신 main 정합성 (5개 탭 지원)**: #280으로 추가된 **작업로그**를 포함하여 **피드 · 할 일 · 노트 · 작업로그 · GitHub** 5개 탭 체계로 일관되게 지원.
4. **점진적 분할 커밋 (Phase별 diff 400줄 안쪽)**: 한 번에 거대한 변경을 가하지 않고, 각 단계별로 테스트와 게이트를 통과하며 안전하게 이식.

---

## 2. Pretendard Variable 적용 방안

- **오프라인/로컬 제약 준수**: 외부 CDN 네트워크 호출을 배제하고, 로컬 woff2 에셋 또는 로컬 폰트 face로 번들링.
- **`@font-face` 정의**:
  ```css
  @font-face {
    font-family: "Pretendard Variable";
    font-weight: 45 920;
    font-style: normal;
    font-display: swap;
    src: local("Pretendard Variable"),
         local("Pretendard-Variable"),
         local("Pretendard"),
         url("./fonts/PretendardVariable.woff2") format("woff2-variations");
  }
  ```
- **CSS 토큰 갱신 (`web/styles/tokens.css`)**:
  ```css
  --sans: "Pretendard Variable", Pretendard, -apple-system, BlinkMacSystemFont, "Segoe UI", "Apple SD Gothic Neo", sans-serif;
  ```
- **`web/DESIGN.md` 동기화**: Typography 섹션의 폰트 패밀리 기본값을 Pretendard Variable로 업데이트.

---

## 3. 세부 단계별(Phase) 실행 계획

### Phase 1: 최신 main 리베이스 & 폰트 및 스펙 정돈 (예상 diff ~150줄)
1. `git fetch origin && git rebase origin/main`으로 최신 커밋(`bd19dac`) 위에 브랜치 재정렬.
2. Pretendard Variable 폰트 선언 및 `web/styles/tokens.css`의 `--sans` 스택 갱신.
3. `web/styles/tokens.css`의 11px(`--text-micro`)을 12px(`--text-chip`)으로 상향 조정하여 12px 미만 제거.
4. `web/DESIGN.md` 정본 동기화:
   - 5개 탭 구조 명시
   - Pretendard Variable 명시
   - Known Gaps 표 갱신
5. 게이트 통과 검증: `bun run check`, `bun run typecheck`, `bun run test`.

### Phase 2: 초슬림 헤더 통합 (CompactHeader & SegmentTabs) (예상 diff ~300줄)
1. 상단 3단 적층(`TopBar` + `NowTable` + `ViewSwitch`)을 신규 `CompactHeader` 기반 42px 단일 바 구조로 재배치.
2. 5개 탭(**피드 · 할 일 · 노트 · 작업로그 · GitHub**) 지원:
   - `SegmentTabs`에 `worklog` 탭 추가 및 사용성 로그 연동.
   - 탭 옆 미읽음/알림 카운트 뱃지 연결.
3. 실시간 에이전트 관제 펄스 인디케이터(초록 점 및 실행 중 카운트)를 헤더 우측 슬롯에 통합.
4. 상단 불명확한 온도 띠 제거.
5. DOM 테스트 및 단위 테스트 갱신.

### Phase 3: 작업 목록 & 관제 카드 리뉴얼 (TaskRow, AttentionCard, LiveAgentCard) (예상 diff ~350줄)
1. `FeedPane.tsx`:
   - 알림 목록을 신규 `AttentionGroup` / `AttentionItem`으로 교체.
2. `NowTable.tsx` / `FeedPane.tsx`:
   - 실시간 진행 중 작업을 `LiveAgentCard`로 연동 (초록 펄스 점 + 경과 시간).
3. `TodoPane.tsx` & `TodoItem.tsx`:
   - 999px 알약형 칩(`.chip`)을 4px 마이크로 `Badge`로 교체.
   - `TaskRow` 컴포넌트 적용: 체크박스, 2단 타이포그래피, 마이크로 태그.
   - 완료된 작업(Done) 맨 아래 접힘(Collapsible) 처리 (Known Gap 해소).
4. DOM 테스트 및 단위 테스트 갱신.

### Phase 4: 스마트 적응형 상세 뷰 전환 (PushDetailContainer) (예상 diff ~300줄)
1. 기존 모달 오버레이 `DetailDrawer.tsx`를 `PushDetailContainer.tsx`로 전환:
   - 모바일 & cmux 좁은 독(`< 720px`): Push 슬라이드 화면 전환 및 ESC/뒤로 가기.
   - PC 데스크톱(`>= 720px`): 좌측 목록(420px) + 우측 상세가 나란히 열리는 2열 스플릿 뷰(Master-Detail).
2. `Timeline.tsx` 및 새로 추가된 `WorkRow`(작업 흐름)가 2열 뷰에서도 유려하게 렌더링되도록 확인.
3. 키보드 내비게이션(ESC 닫기) 검증.

### Phase 5: 최종 검증, 빌드 및 배포 준비 (예상 diff ~50줄)
1. 전체 게이트 검증:
   - `bun run check` (Biome)
   - `bun run typecheck` (TypeScript)
   - `bun run test` (단위 및 DOM 테스트 전원 통과)
   - `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`
2. `bun run build:ui` 빌드 산출물(`dist/`) 확인.
3. 테스트 데몬 구동 확인: `ROCKY_CONFIG=/tmp/rocky-ui.json cargo run -p rockyd`.
4. `bunx changeset`으로 사용자 표면 변경 기록 작성.
5. Draft PR(#288) 업데이트 및 리뷰 요청.

---

## 4. 검증 체크리스트

- [x] 360px (cmux 독 기준) 및 280px (최소 독)에서 가로 스크롤 없이 선명하게 보이는가?
- [x] 스마트폰 화면(100vw, 100dvh)에서 네이티브 앱처럼 상하단 여백이 최적화되었는가?
- [x] PC 데스크톱(>= 720px)에서 목록 클릭 시 우측에 즉시 2열로 열리는가?
- [x] 11px 글자가 완전히 사라지고 Pretendard Variable로 또렷하게 읽히는가?
- [x] 5개 탭(피드 · 할 일 · 노트 · 작업로그 · GitHub)이 정상적으로 오가는가?
