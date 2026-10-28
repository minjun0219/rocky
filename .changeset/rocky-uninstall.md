---
"@minjun0219/rocky": minor
---

`rocky uninstall` 을 더했다. 플러그인·마켓플레이스, launchd job, 떠 있는 데몬, `~/.local/bin/rocky` 링크, `~/.local/share/rocky` 를 걷어내고, 설정·보드·워크로그는 `--purge` 를 붙일 때만 지운다. 실행 전에 목록을 보여 주고 확인을 받는다(`--yes` · `--dry-run`).
