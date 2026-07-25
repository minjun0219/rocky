---
"@minjun0219/rocky": minor
---

rocky-todo(공유 보드 데몬)를 별도 레포/플러그인 `minjun0219/rocky-todo` 로 분리했다. rocky 본체에서 todo 코드·데몬·웹 UI·CLI·`notify-todo` 훅·`todo` 스킬·`docs/rocky-todo.md` 를 제거하고 react/react-dom/zustand 의존을 걷어냈다. `rocky.json` 의 `todo` 키는 관용한다(rocky 는 무시, rocky-todo 데몬이 소비 — 공유 파일이라 거부하지 않음). rocky 마켓플레이스가 rocky-todo 를 github source 2번째 entry 로 서빙하므로 `claude plugin install rocky-todo@rocky-marketplace` 로 설치할 수 있다.
