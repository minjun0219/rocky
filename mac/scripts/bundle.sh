#!/bin/sh
# Rocky.app 을 만든다 — SwiftPM 으로 release 빌드 → mac/build/Rocky.app 에 실행 파일과 Info.plist → codesign.
# 서명은 ROCKY_MAC_SIGN_IDENTITY(키체인에 있는 인증서 이름, 자체 서명 인증서면 그 Common Name)로, 없으면 ad-hoc(-).
# 버전은 레포의 package.json 을 따른다 — 앱 버전을 따로 들지 않는다.
#
#   mac/scripts/bundle.sh             앱만(개발·CI 의 빌드 확인)
#   mac/scripts/bundle.sh --payload   rocky·rockyd·웹 UI·TUI·플러그인도 앱 안에 — 릴리스·nightly 가 쓴다. 미리
#                                     `cargo build --release -p rocky-cli -p rockyd` 와 `bun run build:ui`(·`build:tui`)가 돼 있어야 한다.
#                                     설계: docs/design/specs/2026-11-09-mac-app-bundle-design.md
set -eu

payload=0
case "${1:-}" in
  --payload) payload=1 ;;
  "") ;;
  *) echo "bundle.sh: 모르는 인자 '$1' — --payload 만 받아요" >&2; exit 1 ;;
esac

here=$(cd "$(dirname "$0")/.." && pwd)
cd "$here"

version=$(sed -n 's/^ *"version": *"\([^"]*\)".*/\1/p' ../package.json | head -n 1)
if [ -z "$version" ]; then
  echo "bundle.sh: ../package.json 에서 version 을 못 읽었어요" >&2
  exit 1
fi

commit=$(git -C "$here/.." rev-parse --short HEAD 2>/dev/null || echo unknown)

swift build -c release --product RockyMac
bin="$(swift build -c release --show-bin-path)/RockyMac"

app="$here/build/Rocky.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cp "$bin" "$app/Contents/MacOS/Rocky"
sed -e "s/__VERSION__/$version/g" -e "s/__COMMIT__/$commit/g" Info.plist > "$app/Contents/Info.plist"
# 아바타 조각 그림 — SwiftPM 리소스 번들 대신 앱 리소스로(앱은 Bundle.main 에서 먼저 찾는다).
mkdir -p "$app/Contents/Resources"
cp -R Sources/RockyMac/Resources/rocky "$app/Contents/Resources/rocky"
# 앱 아이콘 — 아바타를 그대로 그린 것(Finder · 보드 창을 연 동안의 Dock).
cp AppIcon.icns "$app/Contents/Resources/AppIcon.icns"

# 함께 싣는 것 — 바이너리는 Contents/MacOS, 나머지는 Contents/Resources. 하나라도 빠지면 반쪽 앱을 내지 않고 멈춘다
# (워크플로는 이 단계가 깨지면 앱 없이 옛 레이아웃으로 묶는다).
if [ "$payload" = 1 ]; then
  repo="$here/.."
  for f in "$repo/target/release/rocky" "$repo/target/release/rockyd" "$repo/dist/index.html" \
    "$repo/.claude-plugin/marketplace.json" "$repo/plugin/.claude-plugin/plugin.json"; do
    [ -e "$f" ] || { echo "bundle.sh: --payload 인데 $f 가 없어요 — 먼저 빌드해 주세요" >&2; exit 1; }
  done
  cp "$repo/target/release/rocky" "$repo/target/release/rockyd" "$app/Contents/MacOS/"
  cp -R "$repo/dist" "$app/Contents/Resources/dist"
  if [ -f "$repo/tui/dist/tui.js" ]; then cp "$repo/tui/dist/tui.js" "$app/Contents/Resources/tui.js"; fi
  # 플러그인 — 폴더 마켓플레이스 모양 그대로(marketplace.json 의 source 가 "./plugin").
  mkdir -p "$app/Contents/Resources/marketplace/.claude-plugin"
  cp "$repo/.claude-plugin/marketplace.json" "$app/Contents/Resources/marketplace/.claude-plugin/"
  cp -R "$repo/plugin" "$app/Contents/Resources/marketplace/plugin"
fi

# 서명은 안쪽부터 — 함께 실은 실행 파일을 먼저, 번들을 마지막에(`--deep` 은 쓰지 않는다). 순서는 rocky_core::mac_app::sign_order 와 같다.
identity="${ROCKY_MAC_SIGN_IDENTITY:--}"
for target in "$app/Contents/MacOS/rocky" "$app/Contents/MacOS/rockyd" "$app"; do
  [ -e "$target" ] || continue
  if ! codesign --force --sign "$identity" --timestamp=none "$target"; then
    echo "bundle.sh: '$identity' 로 $target 을 서명하지 못했어요 — 'security find-identity -p codesigning' 으로 이름을 확인해 주세요" >&2
    exit 1
  fi
done
codesign --verify --strict "$app"

echo "$app ($version · $commit, 서명: $identity, 함께 실음: $([ "$payload" = 1 ] && echo 예 || echo 아니오))"
