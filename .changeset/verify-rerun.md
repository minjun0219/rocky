---
"@minjun0219/rocky": minor
---

기본 브랜치 검증: 실패하면 그 자리에서 한 번 더 돌고 두 번 연속 실패일 때만 알린다. `rocky verify --rerun [보드]`(`POST /api/verify/rerun`, 로컬 전용)로 같은 커밋을 다시 돌려 환경 탓 거짓 실패를 푼다. 대상 디렉터리의 `runs.jsonl` 에 실행 이력(통과·실패·끊김)과 검증을 못 한 이유를 남긴다.
