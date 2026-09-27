---
"@minjun0219/rocky": minor
---

웹 UI 복귀 — rocky-todo 시절의 React 보드 UI 를 `web/` 로 되살려 릴리스 tarball 에 `dist/` 로 동봉한다.
데몬이 실행 파일 옆 `dist/` 를 `http://127.0.0.1:8636/` 에 서빙한다(`ROCKY_TODO_UI_DIST` 로 override).
보드·섹션·상세 드로어(마크다운·댓글·타임라인)·핸드오프·이슈 생성·새 세션·메모 그대로. 이름만 rocky 로
(`/rocky:board` 복사, localStorage 키). 테일넷 없이 밖에서 닿는 길(Cloudflare Tunnel + Access)은 다음 조각.
