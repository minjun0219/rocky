---
"@minjun0219/rocky": minor
---

`rocky rc stop` 이 대상 밖 서버(rocky.json 목록에 없는 폴더의 rc 서버)도 닫는다 — 폴더 이름이나 pid 로 고르고(`POST /api/rc/strays/:ref/stop`, 로컬 전용), 지금 그 폴더의 rc 서버일 때만 pid 로 내린다. 웹 원격 제어 탭의 "대상 밖" 줄에도 닫기를 둔다(한 번 더 묻는다). 설정 대상은 닫지 않는다.
