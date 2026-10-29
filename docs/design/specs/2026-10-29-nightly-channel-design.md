# nightly 채널 설계 — main 을 매일 밤 프리릴리스로

- 날짜: 2026-10-29
- 상태: CI(#517)·CLI 구현됨, 데몬(8번) 남음. 9번 `--main` 유지는 오너 결정. 구현되면 규칙 정본은 [`docs/features/daemon-lifecycle.md`](../../features/daemon-lifecycle.md)와
  [`docs/features/verify.md`](../../features/verify.md)로 옮긴다

## 한 줄

**CI 가 매일 밤 main 을 빌드해 GitHub 프리릴리스(`v<version>-nightly.<YYYYMMDD>`)로 올리고, 기기는 `rocky update --nightly` 로
그 tarball 을 받는다.** 기기에서 빌드하던 `--main` 의 자리를 받는 쪽으로 바꾼다. 기능 표의 *바탕*(데몬 수명의 업데이트) 줄에 붙는다.

## 왜

- **오너 요청(2026-10-29)**: "`--main` 대신 `--nightly` 로, 프리릴리스로 올리는 것까지." 어제의 배포 창(#513)은 기기 빌드를 밤으로
  미루기만 했다.
- **기기 빌드가 무겁다.** `update --main` 은 기기마다 `main-src/` 에서 cargo release 빌드 + `bun run build:ui` 를 돈다(수 분, 검증
  트리까지 수 GB). launchd PATH 에 cargo·bun 이 없으면 배포가 실패한다(verify.md 함정).
- **회사 맥 EDR(2026-10-19)** 이 "방금 빌드한 서명 없는 바이너리의 재시작"을 악성으로 봤다. 받는 바이너리도 서명은 없지만, 같은
  nightly 를 모든 기기가 같은 해시로 받는다 — 해시 예외를 하루 단위로라도 걸 수 있다. 기기에서 컴파일러가 바이너리를 쓰는 흐름은 사라진다.
- **성공 기준**: main 에 머지된 커밋이 다음 날 아침 사람 손 없이 Mac 에 깔려 있다(`/api/health` 의 `version` 이 그 nightly). 기기에
  cargo·bun 이 없어도 된다.

## 결정 표

| # | 질문 | 제안 | 버린 것과 이유 |
|---|---|---|---|
| 1 | 누가 빌드하나 | **CI 의 새 워크플로 `nightly.yml`.** 매일 01:30 KST(16:30 UTC) cron + 수동 실행. `release.yml` 의 `assets` job 과 같은 macos-15 빌드·패키지 단계를 쓴다(패키지 단계는 `scripts/package-macos.sh` 로 뽑았다. `release.yml` 은 안에 그대로 둔다 — 그 수동 실행은 이 스크립트가 없는 옛 태그를 체크아웃한다) | 기기 빌드 유지: 위 "왜". push 마다 프리릴리스: 하루 수십 개가 생기고 macOS 러너 시간이 든다 |
| 2 | 무엇을 올리나 | **main HEAD 가 지난 nightly 와 다르고, 그 커밋의 `ci.yml` 이 초록일 때만.** 빨강이면 그날은 건너뛰고(어제 nightly 가 그대로 최신) 워크플로 요약에 이유를 남긴다 | 무조건 빌드: 깨진 main 이 모든 기기에 깔린다. 기기 검증(`verify`)을 게이트로: 기기마다 결과가 달라질 수 있고 CI 가 이미 같은 게이트다 |
| 3 | 이름·버전 | 태그 **`v<package.json 버전>-nightly.<YYYYMMDD>`**, GitHub **prerelease**(`latest` 는 정식 릴리스에 머문다). CI 가 빌드 전에 Cargo 버전을 그 문자열로 바꿔 `CARGO_PKG_VERSION`·`/api/health` 가 nightly 를 말하게 한다(커밋하지 않는다). 산출물은 정식과 같은 `rocky-<tag>-aarch64-apple-darwin.tar.gz` + `SHA256SUMS` | 하나를 덮어쓰는 `nightly` 태그: 받은 기기가 어느 날 것인지 버전으로 구분되지 않는다. 같은 날 두 번이면 `.<YYYYMMDD>.2` |
| 4 | 몇 개 남기나 | **최근 7개.** 그보다 오랜 nightly 프리릴리스와 태그는 워크플로가 지운다(정식·`-next` 프리릴리스는 건드리지 않는다) | 다 남기기: 릴리스 목록이 nightly 로 덮인다 |
| 5 | 기기 모드 | **`current` → `nightly`** 가 nightly 모드다(main 모드와 같은 링크 규칙). 부트스트랩은 플러그인 버전과 상관없이 `nightly/rocky` 를 실행한다 | 버전 폴더(`v0.54.0-nightly…`)로 받기: 부트스트랩이 플러그인 버전으로 폴더를 고르므로, 플러그인과 바이너리 버전이 어긋나는 nightly 에서는 매 세션 다른 것을 받는다 |
| 6 | 플러그인 파일은 | **`main-src/` clone 을 그 nightly 의 커밋으로 맞추고**(`--main` 처럼 `reset --hard`) 폴더 마켓플레이스로 다시 깐다(지금 `--main` 의 마켓플레이스 처리 그대로, 빌드만 없다). 훅·커맨드·스킬이 바이너리와 같은 커밋이 된다 | GitHub 마켓플레이스 그대로: 플러그인 캐시는 main HEAD 를 가져와 바이너리보다 앞선 훅이 없는 명령을 부를 수 있다 |
| 7 | `rocky update --nightly [TAG]` | 최신 nightly(태그 생략 시) 를 `git ls-remote --tags` 로 찾는다(GitHub API 예산을 쓰지 않는다) → tarball·체크섬 받기 → `nightly/` 교체 → 플러그인 → `daemon restart`. 잠금은 `main-sync.lock` 을 같이 쓴다. 이미 그 nightly 면 아무것도 하지 않는다. 이 모드에서 맨 `rocky update` 는 최신 nightly 로 올린다 | API 의 releases 목록: 60회/시간 비인증 한도와 `gh` 예산 |
| 8 | 자동으로 받기 | **검증의 `deploy` 를 다시 쓴다.** nightly 모드 기기에서 `verify.targets[].deploy: true` 인 rocky main 대상은 "통과 커밋을 빌드" 대신 "새 nightly 가 있으면 `rocky update --nightly <tag>`" 를 띄운다. `deployWindow`(#513)는 그 시간대로 그대로 쓴다 — 01:30 에 만든 nightly 를 02:00-04:00 에 받는다. 같은 태그는 한 번만, 앞으로만, 잠금 기다림, 90분 상한은 지금 배포 규칙 그대로 | 새 설정 블록: 오너 Mac 에 이미 `deploy` + `deployWindow` 가 있다. 같은 뜻을 두 번 적게 된다 |
| 9 | `--main` 은 | **남긴다**(오너 결정 2026-10-29). 특정 커밋을 바로 올리는 개발·핫픽스 길이다. `--main` 으로 올린 기기는 main 모드가 되고 `--nightly` 로 돌아온다. 한 달 사용 횟수(`rocky usage`)를 보고 걷을지 정한다 | 바로 제거: nightly 는 하루 늦어 급한 수정을 올릴 길이 없어진다 |
| 10 | 배포 창(#513)은 | **유지.** 8번에서 받는 시간대로 쓴다. main 모드 배포에서도 지금처럼 그대로 돈다 | 되돌리기: 8번에 같은 것이 다시 필요하다 |

## 흐름

```
CI (01:30 KST)
 └─ main HEAD ≠ 지난 nightly && ci.yml 초록?
     ├─ 아니오 → 건너뜀(요약에 이유)
     └─ 예 → 버전 v0.54.0-nightly.20261029 로 빌드 → tarball + SHA256SUMS → gh release create --prerelease → 7개 넘으면 오랜 것 삭제

기기 데몬 (검증 잡의 바퀴마다, 배포 창 02:00-04:00 안에서만)
 └─ nightly 모드 && deploy 켜짐 && ls-remote 의 최신 nightly 태그 ≠ 깔린 것 && 앞으로 가는가
     └─ rocky update --nightly <tag> (떨어진 프로세스 그룹, 지금 배포와 같은 기록·배너)
```

## 나눠 낼 PR

1. **CI** — `nightly.yml` + 빌드·패키지 스크립트 분리(`release.yml` 의 assets 도 그걸 쓰게) + 보존 정리. 이것만 머지돼도 프리릴리스가
   쌓이기 시작한다.
2. **CLI** — `rocky update --nightly [TAG]`, 부트스트랩의 `current → nightly`, `rocky doctor`·`config show` 의 모드 표시.
3. **데몬** — 8번(nightly 모드의 `deploy`), verify.md·daemon-lifecycle.md 갱신.

## 열린 질문

- cron 시각(01:30 KST)과 배포 창(02:00-04:00)이 맞는가. CI 가 30분 넘게 걸리면 그날 창 안에서 못 받을 수 있다 — 빌드는 지금
  assets job 기준 10-15분.
- 7개 보존이 충분한가.
