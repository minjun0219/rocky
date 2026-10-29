#!/bin/sh
# 빌드된 CLI·데몬·웹 UI·TUI 를 릴리스 tarball 하나로 묶는다 — `out/rocky-<TAG>-aarch64-apple-darwin.tar.gz` + `SHA256SUMS`.
# 플러그인 부트스트랩(plugin/bin/rocky)과 `rocky update --nightly` 가 이 레이아웃 그대로 푼다(필수는 rocky·rockyd 둘 —
# dist/ 는 데몬이, tui.js 는 `rocky tui` 가 옆에서 찾는다). nightly.yml 이 쓴다. release.yml 의 assets job 은 같은 단계를
# 안에 적어 둔다 — 그 job 의 수동 실행은 옛 태그를 체크아웃해서 이 파일이 없을 수 있다.
#
#   scripts/package-macos.sh <TAG>    target/release/{rocky,rockyd}, dist/, tui/dist/tui.js 가 있어야 한다
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
tui=""
if [ -f tui/dist/tui.js ]; then cp tui/dist/tui.js target/release/tui.js; tui=tui.js; fi
# AppleDouble(`._*`) 항목이 tar 에 섞이지 않게 COPYFILE_DISABLE.
COPYFILE_DISABLE=1 tar -czf "out/rocky-${TAG}-${TARGET}.tar.gz" \
  -C target/release rocky rockyd dist $tui
(cd out && shasum -a 256 -- *.tar.gz > SHA256SUMS && shasum -c SHA256SUMS)
ls -la out
