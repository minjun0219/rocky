---
"@minjun0219/rocky": minor
---

statusline 진단 명령 둘을 더한다. `rocky statusline doctor [--session ID]` 는 읽는 설정·`source`·설정 폴더와 로그인된 계정·keychain 항목(macOS 면 후보 전부)·credentials 파일·캐시 폴더·guard·크레딧 상태·토큰을 찍고, `extraCommands` 를 statusline 과 같은 경로로 돌려 항목마다 결과(ok · 출력 없음 · 건너뜀 · 미설치 · 타임아웃 · 비정상 종료)를 보인다. `rocky statusline probe` 는 usage API 원본 응답을 보인다(필드 확인용).
