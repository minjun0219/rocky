#!/usr/bin/env python3
"""수집함 어댑터 — Todoist. 미완료 작업을 rocky 수집함 규약(`{"items": [...]}`)으로 낸다.

동기화가 아니다: 읽기만 하고 Todoist 쪽은 절대 바꾸지 않는다. rocky 는 이 출력을 보여주고,
사용자가 고른 것을 보드에 링크(`https://app.todoist.com/app/task/<id>`)를 달아 올린다.

    rocky.json:
      { "todo": { "inbox": [ { "name": "todoist",
          "command": ["python3", "<repo>/bridges/todoist/inbox.py",
                      "--op", "op://Agent Vault/<item-uuid>/credential",
                      "--filter", "#Inbox"],
          "timeoutMs": 30000 } ] } }

토큰(1Password Agent Vault — 홈에 평문 토큰 파일을 두지 않는다):
  --op REF          `op read REF` 로 읽는다. 서비스 계정 토큰은 `~/.config/op/service-account-token`
                    (600) 에서 이 프로세스 안에서만 실어 준다. 값은 stdout·argv·에러에 절대 나가지 않는다.
  TODOIST_API_TOKEN 개발용 env 폴백. 홈의 평문 파일에 적지 마라 — `op run --env-file` 로 주입한다.

옵션:
  --filter QUERY    Todoist 필터 문법(`#Inbox`, `today | overdue`, `@next`). 없으면 활성 작업 전부.
  --limit N         최대 항목 수(기본 200).
  --from FILE       API 대신 저장된 응답(JSON, `{"results": [...]}` 또는 배열)을 읽는다 — 테스트용,
                    토큰 불필요.

stdlib 만 쓴다(의존 없음). 실패는 exit 1 + stderr 한 줄 — 데몬이 그 소스만 available:false 로 표시한다.
"""

from __future__ import annotations  # `str | None` 를 3.9(맥 시스템 python3)에서도

import argparse
import json
import os
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request

API = "https://api.todoist.com/api/v1"
TASK_URL = "https://app.todoist.com/app/task/{id}"
HTTP_TIMEOUT = 8
OP_TIMEOUT = 5
PAGE = 200
# 최악 합계: op read 5s + 페이지마다 8s. 데몬 기본 상한(10s)을 넘길 수 있으니 등록 예시는 timeoutMs 30000.


def fail(message: str):
    sys.stderr.write(f"todoist: {message}\n")
    sys.exit(1)


def read_token(op_ref: str | None) -> str:
    """`--op` 가 있으면 op read, 없으면 env. 값은 반환만 하고 어디에도 찍지 않는다."""
    if op_ref:
        env = dict(os.environ)
        if "OP_SERVICE_ACCOUNT_TOKEN" not in env:
            path = os.path.expanduser("~/.config/op/service-account-token")
            try:
                with open(path, encoding="utf-8") as f:
                    env["OP_SERVICE_ACCOUNT_TOKEN"] = f.read().strip()
            except OSError:
                fail(f"op 서비스 계정 토큰 파일이 없다: {path}")
        try:
            out = subprocess.run(
                ["op", "read", op_ref],
                env=env,
                capture_output=True,
                text=True,
                timeout=OP_TIMEOUT,
                check=False,
            )
        except FileNotFoundError:
            fail("op CLI 가 없다")
        except subprocess.TimeoutExpired:
            fail("op read 가 제때 끝나지 않았다")
        if out.returncode != 0:
            # op 의 stderr 는 값이 아니라 사유(참조 문법·권한)라 첫 줄만 그대로 전한다.
            reason = (out.stderr.strip().splitlines() or ["exit %d" % out.returncode])[0]
            fail(f"op read 실패: {reason}")
        token = out.stdout.strip()
    else:
        token = os.environ.get("TODOIST_API_TOKEN", "").strip()
    if not token:
        fail("토큰이 없다 — --op REF 또는 TODOIST_API_TOKEN")
    return token


def fetch(token: str, filter_query: str | None, limit: int) -> list:
    """활성 작업을 커서 페이지로 전부 모은다. `--filter` 가 있으면 /tasks/filter."""
    results: list = []
    cursor = None
    while len(results) < limit:
        params = {"limit": str(min(PAGE, limit - len(results)))}
        if cursor:
            params["cursor"] = cursor
        if filter_query:
            params["query"] = filter_query
            path = "/tasks/filter"
        else:
            path = "/tasks"
        url = f"{API}{path}?{urllib.parse.urlencode(params)}"
        req = urllib.request.Request(url, headers={"Authorization": f"Bearer {token}"})
        try:
            with urllib.request.urlopen(req, timeout=HTTP_TIMEOUT) as resp:
                page = json.load(resp)
        except urllib.error.HTTPError as e:
            # 본문에 토큰이 섞일 일은 없지만, 원문을 통째로 옮기지 않는다 — 상태 코드면 충분하다.
            fail(f"HTTP {e.code} ({path})")
        except urllib.error.URLError as e:
            fail(f"연결 실패: {e.reason}")
        except (TimeoutError, json.JSONDecodeError) as e:
            fail(f"응답 오류: {e.__class__.__name__}")
        results.extend(page.get("results", []))
        cursor = page.get("next_cursor")
        if not cursor:
            break
    return results[:limit]


def to_items(tasks: list) -> list:
    """Todoist task → 규약 item. 완료·삭제된 것은 뺀다(활성 엔드포인트라 보통 없다)."""
    items = []
    for task in tasks:
        if not isinstance(task, dict) or task.get("checked") or task.get("is_deleted"):
            continue
        task_id = str(task.get("id", "")).strip()
        title = str(task.get("content", "")).strip()
        if not task_id or not title:
            continue
        item = {"id": task_id, "title": title, "url": TASK_URL.format(id=task_id)}
        description = str(task.get("description") or "").strip()
        if description:
            item["note"] = description
        due = task.get("due") or {}
        due_date = str(due.get("date") or "")[:10] if isinstance(due, dict) else ""
        if len(due_date) == 10:
            item["due"] = due_date
        added = task.get("added_at")
        if isinstance(added, str) and added:
            item["createdAt"] = added
        items.append(item)
    return items


def main() -> None:
    parser = argparse.ArgumentParser(add_help=True)
    parser.add_argument("--op", dest="op_ref")
    parser.add_argument("--filter", dest="filter_query")
    parser.add_argument("--limit", type=int, default=200)
    parser.add_argument("--from", dest="from_file")
    args = parser.parse_args()
    if args.limit <= 0:
        fail("--limit 은 1 이상")

    if args.from_file:
        try:
            with open(args.from_file, encoding="utf-8") as f:
                data = json.load(f)
        except (OSError, json.JSONDecodeError) as e:
            fail(f"--from 읽기 실패: {e.__class__.__name__}")
        tasks = data.get("results", []) if isinstance(data, dict) else data
        tasks = tasks[: args.limit]
    else:
        token = read_token(args.op_ref)
        tasks = fetch(token, args.filter_query, args.limit)

    json.dump({"items": to_items(tasks)}, sys.stdout, ensure_ascii=False)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
