#!/bin/sh
# Rocky.app 을 만든다 — SwiftPM 으로 release 빌드 → mac/build/Rocky.app 에 실행 파일과 Info.plist → codesign.
# 서명은 ROCKY_MAC_SIGN_IDENTITY(키체인에 있는 인증서 이름, 자체 서명 인증서면 그 Common Name)로, 없으면 ad-hoc(-).
# 버전은 레포의 package.json 을 따른다 — 앱 버전을 따로 들지 않는다.
set -eu

here=$(cd "$(dirname "$0")/.." && pwd)
cd "$here"

version=$(sed -n 's/^ *"version": *"\([^"]*\)".*/\1/p' ../package.json | head -n 1)
if [ -z "$version" ]; then
  echo "bundle.sh: ../package.json 에서 version 을 못 읽었어요" >&2
  exit 1
fi

swift build -c release --product RockyMac
bin="$(swift build -c release --show-bin-path)/RockyMac"

app="$here/build/Rocky.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cp "$bin" "$app/Contents/MacOS/Rocky"
sed "s/__VERSION__/$version/g" Info.plist > "$app/Contents/Info.plist"

identity="${ROCKY_MAC_SIGN_IDENTITY:--}"
if ! codesign --force --sign "$identity" --timestamp=none "$app"; then
  echo "bundle.sh: '$identity' 로 서명하지 못했어요 — 'security find-identity -p codesigning' 으로 이름을 확인해 주세요" >&2
  exit 1
fi
codesign --verify --strict "$app"

echo "$app ($version, 서명: $identity)"
