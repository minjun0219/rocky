#!/bin/sh
# rocky-statusline-template: full
# description: 3줄 — duo + 세션 비용/변경 라인 수/경과 시간
#
# rocky statusline 템플릿. 설치본은 ~/.config/rocky/statusline.sh —
# /rocky:statusline 이 복사하고, SessionStart 훅이 헤더의 template 마커를 보고
# 플러그인 업데이트를 자동 전파한다. 커스터마이징은 이 파일(레포)을 고친다. deps: jq, git.
#
#   line 1 = <cwd (~ abbreviated)>  ⎇ <branch>   (branch omitted outside a git repo)
#   line 2 = <model>  ctx <used>%  left <remaining>%  <gray>(<human>)
#   line 3 = $<cost>  +<added>/-<removed>  <elapsed>   (each segment omitted when absent;
#            the whole line is omitted when every segment is empty)
# left = 100 - rate_limits.five_hour.used_percentage (falls back to seven_day → "left7d").
# resets_at is read from the SAME window used for the limit, Unix epoch seconds; the gray
# segment is "(<remaining>, <HH:MM>)" — remaining is human-adaptive (Nd Nh / Nh Nm / Nm)
# and HH:MM is the local reset clock time, e.g. (1h 30m, 18:00). The clock part is
# dropped if `date` can't format the epoch.

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
limit_resets_at=$(echo "$input" | jq -r '.rate_limits.five_hour.resets_at // empty')
limit_label="left"
if [ -z "$limit_used" ]; then
  limit_used=$(echo "$input" | jq -r '.rate_limits.seven_day.used_percentage // empty')
  limit_resets_at=$(echo "$input" | jq -r '.rate_limits.seven_day.resets_at // empty')
  limit_label="left7d"
fi

# --- session cost / churn / elapsed (line 3 sources; each may be absent) ---
cost_usd=$(echo "$input" | jq -r '.cost.total_cost_usd // empty')
lines_added=$(echo "$input" | jq -r '.cost.total_lines_added // empty')
lines_removed=$(echo "$input" | jq -r '.cost.total_lines_removed // empty')
duration_ms=$(echo "$input" | jq -r '.cost.total_duration_ms // empty')

c_reset=$(printf '\033[0m')
c_model=$(printf '\033[1;35m')   # magenta
c_dir=$(printf '\033[36m')       # cyan
c_branch_sym=$(printf '\033[1;34m') # blue
c_branch=$(printf '\033[32m')    # green
c_ctx=$(printf '\033[33m')       # yellow
c_limit=$(printf '\033[35m')     # purple
c_reset_time=$(printf '\033[38;5;245m') # medium gray
c_cost=$(printf '\033[33m')      # yellow
c_added=$(printf '\033[32m')     # green
c_removed=$(printf '\033[31m')   # red
c_elapsed=$(printf '\033[38;5;245m') # medium gray

sep="  "

# --- line 1: cwd, branch appended to the right ---
line1=""
[ -n "$dir" ] && line1="${c_dir}${dir}${c_reset}"
if [ -n "$branch" ]; then
  [ -n "$line1" ] && line1="${line1}  "
  line1="${line1}${c_branch_sym}⎇${c_reset} ${c_branch}${branch}${c_reset}"
fi

# --- line 2: model  ctx  limit  resets-in ---
line2=""
[ -n "$model" ] && line2="${c_model}${model}${c_reset}"

if [ -n "$ctx_used" ]; then
  ctx_rounded=$(printf '%.0f' "$ctx_used")
  [ -n "$line2" ] && line2="${line2}${sep}"
  line2="${line2}${c_ctx}ctx ${ctx_rounded}%${c_reset}"
fi

if [ -n "$limit_used" ]; then
  limit_remaining=$(awk -v u="$limit_used" 'BEGIN { printf "%.0f", 100 - u }')
  [ -n "$line2" ] && line2="${line2}${sep}"
  line2="${line2}${c_limit}${limit_label} ${limit_remaining}%${c_reset}"
fi

if [ -n "$limit_resets_at" ]; then
  case "$limit_resets_at" in
    ''|*[!0-9]*) : ;; # not a plain integer, skip
    *)
      now=$(date +%s)
      rem=$((limit_resets_at - now))
      if [ "$rem" -gt 0 ]; then
        if [ "$rem" -ge 86400 ]; then
          resets_text="$((rem / 86400))d $(((rem % 86400) / 3600))h"
        elif [ "$rem" -ge 3600 ]; then
          resets_text="$((rem / 3600))h $(((rem % 3600) / 60))m"
        else
          resets_text="$(((rem + 59) / 60))m"
        fi
        # 로컬 리셋 시각(HH:MM) — BSD date(-r) 우선, GNU date(-d @) fallback
        reset_clock=$(date -r "$limit_resets_at" +%H:%M 2>/dev/null || date -d "@$limit_resets_at" +%H:%M 2>/dev/null)
        [ -n "$reset_clock" ] && resets_text="${resets_text}, ${reset_clock}"
        [ -n "$line2" ] && line2="${line2} "
        line2="${line2}${c_reset_time}(${resets_text})${c_reset}"
      fi
      ;;
  esac
fi

# --- line 3: session cost  +added/-removed  elapsed ---
line3=""

if [ -n "$cost_usd" ]; then
  cost_text=$(awk -v c="$cost_usd" 'BEGIN { printf "$%.2f", c }')
  line3="${c_cost}${cost_text}${c_reset}"
fi

if [ -n "$lines_added" ] || [ -n "$lines_removed" ]; then
  [ -n "$line3" ] && line3="${line3}${sep}"
  line3="${line3}${c_added}+${lines_added:-0}${c_reset}/${c_removed}-${lines_removed:-0}${c_reset}"
fi

if [ -n "$duration_ms" ]; then
  case "$duration_ms" in
    ''|*[!0-9]*) : ;; # not a plain integer, skip
    *)
      total_s=$((duration_ms / 1000))
      if [ "$total_s" -ge 3600 ]; then
        elapsed_text="$((total_s / 3600))h $(((total_s % 3600) / 60))m"
      elif [ "$total_s" -ge 60 ]; then
        elapsed_text="$((total_s / 60))m"
      else
        elapsed_text="${total_s}s"
      fi
      [ -n "$line3" ] && line3="${line3}${sep}"
      line3="${line3}${c_elapsed}${elapsed_text}${c_reset}"
      ;;
  esac
fi

if [ -n "$line3" ]; then
  printf '%s\n%s\n%s\n' "$line1" "$line2" "$line3"
else
  printf '%s\n%s\n' "$line1" "$line2"
fi
