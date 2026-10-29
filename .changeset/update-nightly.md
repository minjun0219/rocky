---
"@minjun0219/rocky": minor
---

`rocky update --nightly [TAG]` 를 더했다. CI 가 매일 밤 낸 main 의 nightly 프리릴리스(`v<버전>-nightly.<YYYYMMDD>`)를 받아 이 기기를 nightly 모드로 바꾸고 올린다. 기기에서 cargo·bun 으로 빌드하지 않고 tarball 을 받아 체크섬을 확인한다. 그 뒤로 `rocky update` 는 최신 nightly 로 올리고, `rocky update --release` 로 릴리스에 돌아간다.
