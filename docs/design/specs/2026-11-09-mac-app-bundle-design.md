# 버전 폴더 하나로 배포 — 데몬·CLI·플러그인·맥 앱을 한 tarball 에

- 날짜: 2026-11-09, **2026-11-10 개정(A안)**
- 상태: A안으로 확정(오너, 2026-11-10 "tui는 제거하고 A안으로 가자"). 처음 안(앱이 정본, SMAppService)은 0번 실측에서 막혀 접었다 —
  아래 "처음 안과 실측" 에 기록으로 남긴다. 구현되면 규칙 정본은 [`docs/features/daemon-lifecycle.md`](../../features/daemon-lifecycle.md)와
  [`docs/features/mac.md`](../../features/mac.md)로 옮긴다
- 앞선 문서: [오케스트레이터 설계](./2026-10-29-rocky-orchestrator-design.md)의 "데몬을 어디에",
  [nightly 채널 설계](./2026-10-29-nightly-channel-design.md)

## 한 줄

**업데이트 단위는 릴리스 tarball 이 풀린 버전 폴더(`~/.local/share/rocky/<v버전|nightly>/`) 하나다.** 그 안에 `rocky`·`rockyd`·웹 UI·
플러그인 사본(폴더 마켓플레이스)·`Rocky.app`(선택)이 같은 커밋으로 들어 있고, `current` 링크 하나로 함께 바뀐다. 기능 표의 *바탕*(데몬 수명) 줄에 붙는다.

## 왜

- **오너 요청(2026-11-09)**: 데몬과 Claude Code 플러그인이 맥 앱과 함께 배포되는 구조. 앱을 정본으로 두려던 근거는 SMAppService(고정 경로·서명자 하나)였는데
  실측에서 자체 서명으로는 쓸 수 없었다(아래). 그러면 앱 안에 넣어 얻는 것이 없고, 앱 없는 기기(SSH·화면 없는 Mac Studio)까지 한 길로 가는 버전 폴더가 낫다.
- **지금 이미 반쯤 그렇다.** 릴리스 모드는 `plugin.json` 버전으로 tarball 을 고르고, nightly 모드는 `main-src/` 를 그 태그 커밋으로 맞춘다. 앱도 #528 부터
  tarball 에서 깐다. 남은 틈은 플러그인이 tarball 밖(GitHub 또는 `main-src/`)에서 온다는 것과 `--main` 이 앱을 빌드하지 않는다는 것.
- **성공 기준**: (1) 버전 폴더 하나 = 데몬·CLI·플러그인·앱이 같은 커밋. (2) 업데이트 때 plist 를 다시 쓰지 않는다(#504, `current/rockyd` 고정).
  (3) 회사 맥(쓰게 되면)은 Swift·cargo 없이 nightly 를 받는다.

## A안 결정

| # | 질문 | 결정 |
|---|---|---|
| A1 | 단위 | 버전 폴더. `current` → `v<버전>` · `nightly` 링크가 곧 모드(지금 그대로) |
| A2 | tarball | 맨 위에 실제 파일 `rocky`·`rockyd`·`dist/`, 그리고 `marketplace/`(레포의 `.claude-plugin/marketplace.json` + `plugin/` 사본), 있으면 `Rocky.app`(바이너리를 품지 않는 화면 앱) |
| A3 | 데몬 상주 | 지금처럼 rocky 가 쓰는 `~/Library/LaunchAgents` plist, 경로는 `current/rockyd` 고정 — 재시작은 `kickstart -k` 만(#504). SMAppService 는 쓰지 않는다 |
| A4 | 플러그인 | 폴더 마켓플레이스 `rocky-marketplace` 가 `current/marketplace` 를 가리킨다. 버전이 바뀌면 `--main`·`--nightly` 와 같은 길(`main_build::plugin_step`)로 빼고 다시 깐다. 처음 바꿀 때 플러그인 옵션이 지워지므로 한 번 확인(`--yes`) |
| A5 | 맥 앱 | **선택 부품, 필요할 때 CLI 로만 띄운다**(오너 결정 2026-11-10 "메뉴막대와 아바타는 옵션으로 두자 cli로 필요할때만 실행"). 데몬·CLI·플러그인·웹 UI 는 앱 없이 돈다. `rocky update` 는 앱을 `/Applications` 에 깔지도, 내리고 띄우지도 않는다 — 앱은 버전 폴더(`current/Rocky.app`)에 머문다. `rocky app` 이 그 앱을 (키체인에 `rocky` 인증서가 있으면 다시 서명해) `open` 으로 띄우고, `rocky app --quit` 이 내린다. 앱은 로그인 항목에 스스로 오르지 않는다(메뉴에서 켤 수는 있다). 앱 빌드가 깨지면 앱 없이 낸다. 이미 `/Applications` 에 깔린 앱과 로그인 항목은 손대지 않고 `rocky doctor` 가 알려만 준다 |
| A6 | TUI | **제거**(오너 결정 2026-11-10). 별도 PR |
| A7 | `--main` | 개발·핫픽스 길로만. 회사 맥은 쓰게 되면 nightly(2026-11-09 결정 1번) |

## 나눠 낼 PR(A안)

1. **tarball 에 플러그인 사본**(#548) — `package-macos.sh`·`release.yml` 이 `marketplace/` 를 같이 묶는다. 기기 동작은 아직 그대로.
2. **TUI 제거** — `tui/`, `rocky tui`, `build:tui`, tarball 의 `tui.js`, 문서.
3. **폴더 마켓플레이스를 `current/marketplace` 로(main·nightly)** — `--main`·`--nightly` 가 새 버전 폴더를 건 뒤 마켓플레이스를
   거기에 걸고 플러그인을 다시 깐다. main 빌드도 `main/marketplace` 사본을 만들고, nightly 의 `main-src/` clone·reset 은 없어진다.
   Antigravity 번들도 사본(`marketplace/antigravity`)에서. 옛 `main-src` 마켓플레이스는 다음 업데이트에서 한 번 확인받고 옮긴다.
3c. **릴리스 모드도**(뒤로) — 릴리스 모드는 GitHub 마켓플레이스가 입구이고 부트스트랩이 플러그인 버전으로 tarball 을 고른다. 폴더로
   옮기려면 `rocky update` 가 최신 릴리스 tarball 을 플러그인 없이 직접 받아야 한다(nightly 의 `install_nightly` 와 같은 길).
3b. **맥 앱을 CLI 로** — `rocky update` 의 앱 깔기·다시 띄우기를 빼고 `rocky app [--quit]` 을 더한다. 앱의 첫 실행 로그인 항목 등록(`LoginItem.swift`)을 끈다.
4. **nightly 자동 받기**(nightly 설계 8번) — Mac Studio 의 `--main` 자동 배포를 옮긴다.

## 처음 안과 실측(기록)

처음 안은 앱 번들(`Rocky.app`)을 정본으로 두고 데몬을 `SMAppService.agent` 로 등록하는 것이었다(아래 결정 표 0-10). 0번 실측(2026-11-09, Mac Studio):

- ad-hoc 과 자체 서명 인증서 `rocky` 모두, 등록·`kickstart -k` 는 되지만 **새 빌드로 rename 해 바꿔 끼우면 `Launch Constraint Violation`** 으로 뜨지 못했다.
- `unregister` 는 BTM 항목을 지우지 않고 disabled 로 남기고 다음 `register` 가 그 항목(옛 빌드의 실행 제약)을 다시 써서, 자체 서명에서는 다시 등록해도 살아나지 않았다.
  자체 서명엔 팀 ID 가 없어 빌드마다 다른 서명으로 취급되는 것으로 보인다(추론). Developer ID 에서는 재지 않았다.
- 재등록마다 "백그라운드 실행" 알림이 뜨고 한도를 넘기면 더 뜨지 않는다. 앱만 지우면 job 이 남는다(`bootout` 필요).
- 실측이 남긴 disabled 항목(smtest-agent)은 Mac Studio 의 로그인 항목에 남아 있다 — 지우려면 모든 로그인 항목을 초기화하는 `sfltool resetbtm` 뿐이라 두었다.

### 처음 안의 결정 표(접음)

| # | 질문 | 결정 | 버린 것과 이유 |
|---|---|---|---|
| 0 | 먼저 확인할 것 | **맥에서 `SMAppService.agent` 실측**(아래 "실측" 절). 결과가 나쁘면 3번만 "지금처럼 rocky 가 plist 를 쓰되 경로만 앱 안"으로 물린다 | 실측 없이 구현: 자체 서명 앱에서의 동작과 EDR 반응은 문서로 확인되지 않는다 |
| 1 | 회사 맥(`--main`)은 | **nightly 로 옮긴다**(오너 결정). `--main` 은 개발·핫픽스 길로만 남고, 그 모드는 앱 모드가 아니다(지금처럼 `main/` 의 바이너리, 앱은 손으로) | `--main` 이 앱까지 빌드: 기기마다 Xcode·Swift 가 필요하고, 방금 컴파일한 바이너리가 도는 EDR 모양이 그대로 남는다 |
| 2 | 번들 안에 무엇을 | `Contents/MacOS/Rocky`(앱), `Contents/Helpers/{rocky, rockyd}`, `Contents/Resources/{dist/, tui.js, marketplace/}`, `Contents/Library/LaunchAgents/com.rocky.daemon.plist`. `Info.plist` 에 커밋 해시(`RockyCommit`)를 굽는다 | `Contents/MacOS/` 에 나란히: 대소문자를 가리지 않는 볼륨(macOS 기본)에서 CLI `rocky` 가 앱 실행 파일 `Rocky` 를 덮어쓴다(#548 리뷰) |
| 3 | 데몬 상주 | **`SMAppService.agent(plistName:)`** 로 앱이 등록한다(오너 결정). 번들의 plist 는 `BundleProgram`(`Contents/Helpers/rockyd`)을 가리켜 경로가 바뀌지 않는다. 라벨은 지금의 `com.rocky.daemon` 그대로 — `kickstart -k gui/<uid>/com.rocky.daemon` 와 `launchctl print` 를 쓰는 코드가 그대로 돈다. 앱 모드에서 rocky(CLI·훅)는 **plist 를 쓰지도 bootout/bootstrap 하지도 않는다** — 재시작은 `kickstart -k` 뿐, job 이 없으면 앱을 `open -g` 로 띄워 등록을 맡긴다 | rocky 가 `~/Library/LaunchAgents` 에 계속 쓰기: EDR 이 본 모양 그대로. 앱이 데몬을 자식으로 띄우기: 앱이 꺼지면 데몬도 꺼지고, 훅·SSH 만 쓰는 상황에서 앱이 떠 있어야 한다 |
| 4 | 플러그인 | **앱 안 `Resources/marketplace/` 를 폴더 마켓플레이스 `rocky-marketplace` 로 건다**(오너 결정). 내용은 레포의 `.claude-plugin/marketplace.json` + `plugin/` 사본. 앱이 바뀌면 `--main`·`--nightly` 와 같은 길(`main_build::plugin_step`)로 빼고 다시 깐다 — 폴더 마켓플레이스도 설치 때 캐시로 복사하고, 버전이 같으면 `plugin update` 가 새 커밋을 안 가져온다(daemon-lifecycle 함정). 처음 바꿀 때 플러그인 옵션이 지워지므로 한 번 확인을 받는다(`--yes`) | GitHub 마켓플레이스 유지: 플러그인 캐시가 main HEAD 를 따라가 앱의 바이너리와 커밋이 어긋난다(nightly 설계 6번과 같은 이유) |
| 5 | 기기 모드 | **`current` → 깔린 앱의 `Contents/Helpers`(절대 경로)** 가 앱 모드다. 채널(릴리스 · nightly)은 `$XDG_DATA_HOME/rocky/channel` 한 줄(`release`·`nightly`)에 둔다. 부트스트랩은 앱 모드면 플러그인 버전과 상관없이 `current/rocky` 를 실행한다(main·nightly 모드와 같은 규칙). `~/.local/bin/rocky` → `current/rocky` 는 그대로 따라온다 | 채널마다 `current` 이름을 따로(`app-nightly` …): 링크 하나가 자리와 채널을 같이 뜻해 판정이 늘어난다. 앱 `defaults` 에 채널: Rust 가 읽기 어렵고 판정이 Swift 로 넘어간다 |
| 6 | 배포물 | 릴리스·nightly tarball 의 **정본은 `Rocky.app`** 이다. 옛 CLI 가 새 tarball 을 풀 수 있게 맨 위 `rocky`·`rockyd`·`dist`·`tui.js` 는 앱 안을 가리키는 심볼릭 링크로 두 릴리스 동안 남긴다. 앱 없는 기기(SSH 만 쓰는 곳, 앱을 지운 곳)는 지금처럼 부트스트랩이 `v<버전>/` 에 풀고 그 안의 앱 바이너리를 실행한다 — 앱을 `/Applications` 에 깔지 않아도 돈다 | 바이너리를 앱 밖에 한 벌 더: tarball 이 두 배가 되고 둘 중 무엇이 돌지 판정이 생긴다 |
| 7 | 누가 업데이트하나 | **판단과 실행은 Rust CLI(`rocky update`)가 한다.** 앱 메뉴의 "업데이트"는 앱 안의 `rocky update --yes` 를 띄우고 진행을 보여 줄 뿐이다. 순서: 받기·체크섬 → `.Rocky.app.new` 에 풀기 → (인증서가 있으면) 앱째 다시 서명·검증 → 바꿔 끼우기(지금의 `mac_app.rs`) → 플러그인 다시 깔기 → `kickstart -k` → 앱 다시 띄우기. 바꿔 끼우는 동안 도는 데몬은 옛 inode 를 쥐고 있어 끊기지 않는다 | Sparkle: 새 Swift 의존성이고 판정이 Swift 로 간다(mac.md "판정을 Swift 로 새로 만들지 않는다"). 앱이 직접 받기: 같은 이유 |
| 8 | 자동으로 받기 | **nightly 설계 8번을 이 구조로 구현한다**: 앱 모드 + nightly 채널 + `verify.targets[].deploy` 인 기기는 배포 창(`deployWindow`) 안에서 새 nightly 가 있으면 `rocky update --nightly <tag>` 를 띄운다. Mac Studio 의 `--main` 자동 배포도 이것으로 옮긴다 | 앱 안에 따로 주기 검사: 데몬의 배포 규칙(앞으로만, 같은 태그 한 번, 잠금 기다림)을 두 곳에 두게 된다 |
| 9 | 서명 | 앱 **안쪽부터** 서명한다(`rocky`·`rockyd` → 앱, `--deep` 은 쓰지 않는다). CI 는 ad-hoc, 기기는 키체인에 `rocky` 인증서가 있으면 다시 서명(지금 규칙 그대로). 공증·Developer ID 는 하지 않는다 — curl 로 받아 격리 속성이 없고 지금도 지운다. 회사 EDR 이 자체 서명 인증서를 믿을지는 모른다; 요구가 오면 Developer ID 는 그때 오너가 정한다 | Developer ID 지금: 연회비, 오너가 아직 필요로 하지 않았다(rocky-company-mac-edr) |
| 10 | 문서 규칙 | `AGENTS.md` 구성의 맥 앱 줄과 `mac.md` 의 "데몬의 클라이언트 — 데몬은 고치지 않는다"를 **"데몬의 클라이언트이자 그 배포 단위 — 데몬을 등록·업데이트하지만 데몬 판정은 Rust"** 로 고친다. 언어 경계는 그대로(데몬·CLI 는 Rust, 앱은 Swift) | — |

### 실측(0번) — 구현 전에 맥에서

Mac Studio 의 Remote Control 세션에서 버리는 앱(`dev.minjun.rocky.smtest`, 라벨 `com.rocky.smtest`, `sleep` 만 하는 에이전트)으로 확인한다.

1. 자체 서명(`rocky`)·ad-hoc 앱에서 `SMAppService.agent(plistName:).register()` 가 되는가, `status` 가 `.enabled` 인가 `.requiresApproval` 인가.
2. 등록 뒤 `launchctl print gui/<uid>/com.rocky.smtest` 와 `kickstart -k` 가 지금처럼 도는가(rocky 의 재시작 코드가 기대는 두 명령).
3. 같은 경로에 **다시 서명한 새 빌드**를 rename 으로 바꿔 끼워도 등록이 남는가, 백그라운드 항목 알림이 다시 뜨는가(nightly 는 매일 해시가 바뀐다).
4. `unregister()` 와 앱 삭제 뒤 job 이 사라지는가(`rocky uninstall` 이 기댈 길).

회사 맥 EDR 반응은 실측할 수 없다 — 바뀐 모양(rocky 가 plist 를 쓰지 않음, 서명자 고정)만 보안팀에 알린다.

### 흐름

```
CI(release · nightly)
 └─ cargo release + build:ui + build:tui + plugin 사본 → bundle.sh 가 Rocky.app 에 다 넣고 ad-hoc 서명 → tarball(Rocky.app + 옛 이름 링크)

기기: rocky update [--nightly]            ← 앱 메뉴 "업데이트" 도 이것을 띄운다
 └─ 받기·체크섬 → .Rocky.app.new → rocky 인증서로 다시 서명 → /Applications/Rocky.app 바꿔 끼우기
     → current → /Applications/Rocky.app/Contents/Helpers, channel 기록
     → 마켓플레이스 rocky-marketplace = Rocky.app/Contents/Resources/marketplace, 플러그인 빼고 다시 깔기
     → launchctl kickstart -k gui/<uid>/com.rocky.daemon(job 이 없으면 앱을 띄워 SMAppService 등록)
     → 앱 다시 띄우기

앱이 뜰 때
 └─ SMAppService.agent("com.rocky.daemon.plist") 상태 확인 → 미등록이면 register()(처음 한 번 백그라운드 항목 알림)
```

### 옮겨 가기

- **릴리스·nightly 기기**: 새 버전의 `rocky update` 가 처음 앱 모드로 갈 때 옛 `~/Library/LaunchAgents/com.rocky.daemon.plist` 를
  bootout 하고 지운 뒤(한 번뿐인 plist 일), 앱을 띄워 SMAppService 로 다시 등록한다. 포트가 락이라 두 데몬이 동시에 DB 를 열지는 않는다(daemon-lifecycle "포트 먼저").
- **회사 맥(main 모드)**: `rocky update --nightly` 한 번(1번 결정). 키체인의 `rocky` 인증서로 다시 서명된다.
- **Mac Studio(main 모드 + 자동 배포)**: 8번 PR 이 머지된 뒤 `rocky update --nightly`. 화면이 없어도 앱은 깔리고 뜬다(rocky-owner-mac).
- **`--main` 개발 루프**는 그대로다. 앱 모드에서 `--main` 을 부르면 main 모드로 나가고(앱은 그대로 두고 SMAppService 등록을 해제), `--nightly`·`--release` 로 돌아온다.

### 나눠 낼 PR

1. **번들 레이아웃** — `bundle.sh` 가 바이너리·`dist/`·`tui.js`·마켓플레이스 사본을 넣고 안쪽부터 서명, `Info.plist` 커밋, `rockyd` 가
   `Resources/dist` 도 찾기, `package-macos.sh` 의 tarball(앱 + 옛 이름 링크). 기기 동작은 아직 안 바뀐다.
2. **앱 모드(CLI·부트스트랩)** — `current` → 앱, `channel` 파일, `rocky update` 의 앱 경로, 앱 안 마켓플레이스로 플러그인 다시 깔기, `doctor`·`config show` 표시.
3. **SMAppService** — 0번 실측 뒤. 앱의 등록, 번들 plist, 앱 모드에서 rocky 의 launchd 쓰기 끄기, 옛 plist 옮기기, `uninstall`.
4. **앱 메뉴 "업데이트"** — `rocky update --yes` 를 띄우고 진행·실패를 보여 준다.
5. **nightly 자동 받기**(8번) — 데몬의 배포를 nightly 채널로, 문서 정본 이관(daemon-lifecycle · mac · verify · AGENTS.md).

### 열린 질문

- 0번 3항(바꿔 끼운 뒤 등록 유지)이 실패하면: 업데이트 때마다 `unregister`→`register` 하고 알림을 받아들일지, 3번을 "rocky 가 쓰는 plist, 경로만 앱 안"으로 물릴지 — 실측 결과와 함께 오너에게 묻는다.
- 앱 모드 기기에서 GUI 로그인 없이(SSH 만) 데몬을 띄워야 할 때 — SMAppService 에이전트는 GUI 세션 도메인이다. 지금도 LaunchAgent 라 같은 제약이지만 실측에서 같이 본다.
