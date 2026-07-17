#!/bin/sh
# rocky-statusline-template: mini
# description: 1줄 — cwd+branch+model+ctx+세션 잔여율, 가장 컴팩트
#
# rocky statusline 템플릿. 설치본은 ~/.config/rocky/statusline.sh —
# /rocky:statusline 이 복사하고, SessionStart 훅이 헤더의 template 마커를 보고
# 플러그인 업데이트를 자동 전파한다. 커스터마이징은 이 파일(레포)을 고친다. deps: jq, git.
#
#   <cwd (~ abbreviated)>  ⎇ <branch>  <model>  ctx <used>%  left <remaining>%
#   (branch omitted outside a git repo; left falls back to seven_day → label "left7d")

input=$(cat)

model=$(echo "$input" | jq -r '.model.display_name // empty')

raw_dir=$(echo "$input" | jq -r '.workspace.current_dir // empty')
dir="$raw_dir"
case "$dir" in
  "$HOME"/*) dir="~${dir#$HOME}" ;;
  "$HOME") dir="~" ;;
esac

branch=""
if [ -n "$raw_dir" ]; then
  branch=$(git -C "$raw_dir" --no-optional-locks rev-parse --abbrev-ref HEAD 2>/dev/null)
fi

ctx_used=$(echo "$input" | jq -r '.context_window.used_percentage // empty')

limit_used=$(echo "$input" | jq -r '.rate_limits.five_hour.used_percentage // empty')
limit_label="left"
if [ -z "$limit_used" ]; then
  limit_used=$(echo "$input" | jq -r '.rate_limits.seven_day.used_percentage // empty')
  limit_label="left7d"
fi

c_reset=$(printf '\033[0m')
c_model=$(printf '\033[1;35m')   # magenta
c_dir=$(printf '\033[36m')       # cyan
c_branch_sym=$(printf '\033[1;34m') # blue
c_branch=$(printf '\033[32m')    # green
c_ctx=$(printf '\033[33m')       # yellow
c_limit=$(printf '\033[35m')     # purple

sep="  "
line=""

append() {
  [ -n "$line" ] && line="${line}${sep}"
  line="${line}$1"
}

[ -n "$dir" ] && append "${c_dir}${dir}${c_reset}"
[ -n "$branch" ] && append "${c_branch_sym}⎇${c_reset} ${c_branch}${branch}${c_reset}"
[ -n "$model" ] && append "${c_model}${model}${c_reset}"

if [ -n "$ctx_used" ]; then
  ctx_rounded=$(printf '%.0f' "$ctx_used")
  append "${c_ctx}ctx ${ctx_rounded}%${c_reset}"
fi

if [ -n "$limit_used" ]; then
  limit_remaining=$(awk -v u="$limit_used" 'BEGIN { printf "%.0f", 100 - u }')
  append "${c_limit}${limit_label} ${limit_remaining}%${c_reset}"
fi

printf '%s\n' "$line"
