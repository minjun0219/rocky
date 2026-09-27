#!/bin/sh
# 수집함 어댑터 — 파일. 인자로 받은 JSON 파일을 그대로 낸다.
#
# 규약(docs/board.md "수집함 어댑터")의 최소 구현이자 테스트·수동 확인용이다. 실제 앱 어댑터는
# 이 모양으로 외부 API 를 읽어 `{ "items": [...] }` 를 stdout 에 쓰면 된다.
#
#   rocky.json:  { "todo": { "inbox": [ { "name": "file", "command": ["sh", "<repo>/bridges/file/inbox.sh", "~/inbox.json"] } ] } }
#
# 파일이 없으면 exit 1 + stderr 한 줄 — 데몬은 그 소스만 available:false 로 표시한다.
set -eu
path="${1:-}"
if [ -z "$path" ]; then
  echo "usage: inbox.sh <items.json>" >&2
  exit 2
fi
case "$path" in
  "~/"*) path="$HOME/${path#\~/}" ;;
esac
if [ ! -f "$path" ]; then
  echo "no such file: $path" >&2
  exit 1
fi
cat "$path"
