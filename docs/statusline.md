# rocky statusline 템플릿

`/rocky:statusline` 으로 설치하는 번들 statusline 템플릿 3종의 표시 내용 정리.
설치·전파 메커니즘(안정 경로 `~/.config/rocky/statusline.sh`, 헤더 마커 기반 훅 sync)은
[`FEATURES.md`](../FEATURES.md) 의 statusline 절과 `/rocky:statusline` 커맨드 문서 참고.
deps: `jq`, `git` (full 은 `awk` 추가 — POSIX 표준이라 별도 설치 불요).

모든 값은 Claude Code 가 statusLine 스크립트 stdin 으로 주는 JSON 에서 읽는다.
세그먼트는 소스 값이 없으면 그냥 생략된다 — 빈 칸이나 placeholder 를 남기지 않는다.

## `duo` (2줄, 기본)

```
~/dev/workspaces/rocky  ⎇ main
Opus  ctx 42%  left 63% (1h 30m, 18:00)
```

| 세그먼트 | 소스 | 비고 |
| --- | --- | --- |
| cwd | `workspace.current_dir` | `~` 축약, cyan |
| `⎇ branch` | `git rev-parse --abbrev-ref HEAD` | 비 git 디렉터리면 생략, green |
| model | `model.display_name` | magenta |
| `ctx N%` | `context_window.used_percentage` | 반올림, yellow |
| `left N%` | `100 − rate_limits.five_hour.used_percentage` | five_hour 없으면 seven_day → 라벨 `left7d`, purple |
| `(남은시간, HH:MM)` | 같은 window 의 `resets_at` (epoch) | 미래일 때만, medium gray |

## `mini` (1줄, 컴팩트)

```
~/dev/workspaces/rocky  ⎇ main  Opus  ctx 42%  left 63%
```

duo 의 두 줄을 한 줄로 합친 구성 — 리셋 타이머는 없다. 세그먼트/소스는 duo 와 동일.

## `full` (3줄)

```
~/dev/workspaces/rocky  ⎇ main*  ↑2 ↓1
Opus  ctx 72%  left 15% (1h 30m, 18:00)
$4.20 ($2.1/h)  +120/-40  2h 0m
```

duo 위에 git 상태 · 임계값 경고색 · 세션 비용 라인이 얹힌다.

| 세그먼트 | 소스 | 비고 |
| --- | --- | --- |
| `*` (dirty) | `git status --porcelain` 비어있지 않음 | untracked 포함, 브랜치명 뒤, yellow |
| `↑a ↓b` | `git rev-list --left-right --count @{u}...HEAD` | 0인 쪽 생략, upstream 없으면 통째 생략, gray |
| `$N.NN` | `cost.total_cost_usd` | yellow |
| `($N.N/h)` | cost ÷ 경과시간 | **경과 5분 미만이면 생략** (초반 왜곡 방지), gray |
| `+A/-R` | `cost.total_lines_added` / `total_lines_removed` | green / red |
| 경과 | `cost.total_duration_ms` | `Nh Nm` / `Nm` / `Ns`, gray |

### 임계값 경고색 (line 2)

평상시(안전 구간)는 둔한 회색으로 내려 경고만 도드라진다.

| 값 | 안전 (dim gray) | 경고 (yellow) | 위험 (bold red) |
| --- | --- | --- | --- |
| `ctx` | < 70% | 70–89% | ≥ 90% |
| `left` | > 30% | 11–30% | ≤ 10% |

## 커스터마이징

번들 템플릿(`statusline/<name>.sh`)을 고치면 `SessionStart` 훅이 다음 세션에 설치본으로
전파한다. 설치본(`~/.config/rocky/statusline.sh`)을 직접 고치면 sync 때 덮이므로 주의.
헤더의 `# rocky-statusline-template: <name>` 마커는 sync 대상 결정에 쓰인다 — 제거 금지.
