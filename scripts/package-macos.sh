#!/bin/sh
# 빌드된 CLI·데몬·웹 UI·플러그인 사본·맥 앱 을 릴리스 tarball 하나로 묶는다 — `out/rocky-<TAG>-aarch64-apple-darwin.tar.gz` + `SHA256SUMS`.
# 플러그인 부트스트랩(plugin/bin/rocky)과 `rocky update --nightly` 가 이 레이아웃 그대로 푼다(필수는 rocky·rockyd 둘 —
# dist/ 는 데몬이 옆에서 찾는다). nightly.yml 이 쓴다. release.yml 의 assets job 은 같은 단계를
# 안에 적어 둔다 — 그 job 의 수동 실행은 옛 태그를 체크아웃해서 이 파일이 없을 수 있다.
#
#   scripts/package-macos.sh <TAG>    target/release/{rocky,rockyd}, dist/ 가 있어야 한다(mac/build/Rocky.app 은 있으면)
set -eu

TAG="${1:-}"
[ -n "$TAG" ] || { echo "TAG 가 비어 있다"; exit 1; }
TARGET="$(rustc -vV | sed -n 's/^host: //p')"
if [ "$TARGET" != "aarch64-apple-darwin" ]; then
  echo "러너 타깃이 aarch64-apple-darwin 이 아니다: $TARGET — 문서와 산출물 이름이 어긋난다"
  exit 1
fi
[ -f dist/index.html ] || { echo "dist/index.html 이 없다 — 웹 UI 빌드가 안 됐다"; exit 1; }
mkdir -p out
rm -rf target/release/dist
cp -R dist target/release/dist
# 맥 앱 — `mac/scripts/bundle.sh` 가 만든 것이 있고 서명 검증을 통과하면 넣는다(빌드가 중간에 깨진 앱은 뺀다)(`rocky update` 가 /Applications 로 옮긴다). ditto 는 서명을 그대로 옮긴다.
app=""
rm -rf target/release/Rocky.app
if [ -d mac/build/Rocky.app ] && codesign --verify --strict mac/build/Rocky.app; then ditto mac/build/Rocky.app target/release/Rocky.app; app=Rocky.app; fi
# 플러그인 사본 — 폴더 마켓플레이스 모양 그대로(marketplace.json 의 source 가 "./plugin"). 버전 폴더가 배포 단위라 플러그인도
# 바이너리와 같은 커밋으로 함께 받는다(설계 2026-11-09-mac-app-bundle-design.md, A안).
rm -rf target/release/marketplace
mkdir -p target/release/marketplace/.claude-plugin
cp .claude-plugin/marketplace.json target/release/marketplace/.claude-plugin/
cp -R plugin target/release/marketplace/plugin
# Antigravity 번들 — 스킬은 ../../plugin/skills 링크라 marketplace/ 안에서 그대로 풀린다(`agy plugin install` 이 복사하며 푼다).
cp -R antigravity target/release/marketplace/antigravity
# AppleDouble(`._*`) 항목이 tar 에 섞이지 않게 COPYFILE_DISABLE.
COPYFILE_DISABLE=1 tar -czf "out/rocky-${TAG}-${TARGET}.tar.gz" \
  -C target/release rocky rockyd dist marketplace $app
(cd out && shasum -a 256 -- *.tar.gz > SHA256SUMS && shasum -c SHA256SUMS)
ls -la out
