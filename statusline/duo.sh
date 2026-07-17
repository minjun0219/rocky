#!/bin/sh
# rocky-statusline-template: duo
# description: 2줄 — cwd+branch / model+ctx+세션 잔여율+리셋 타이머 (기본)
#
# rocky statusline 템플릿. 설치본은 ~/.config/rocky/statusline.sh —
# /rocky:statusline 이 복사하고, SessionStart 훅이 헤더의 template 마커를 보고
# 플러그인 업데이트를 자동 전파한다. 커스터마이징은 이 파일(레포)을 고친다. deps: jq, git.
#
#   line 1 = <cwd (~ abbreviated)>  ⎇ <branch>   (branch omitted outside a git repo)
#   line 2 = <model>  ctx <used>%  left <remaining>%  <gray>(<human>)
#            (segments space-joined, no "|"; the reset segment is medium gray and only
#            shown when rate_limits.<window>.resets_at is present and still in the future)
# left = 100 - rate_limits.five_hour.used_percentage (session limit remaining).
# Falls back to seven_day if five_hour is absent; label becomes "left7d" in that case.
# resets_at is read from the SAME window used for the limit (five_hour or seven_day),
# Unix epoch seconds; parenthesized text is "<remaining>, <HH:MM>" — remaining is
# human-adaptive (Nd Nh / Nh Nm / Nm) and HH:MM is the local reset clock time,
# e.g. (1h 30m, 18:00). The clock part is dropped if `date` can't format the epoch.

input=$(cat)
# 로캘 독립 숫자 처리 — 쉼표 소수점 로캘에서 printf '%.0f' 파싱 실패 방지
export LC_NUMERIC=C

# --- model display name ---
model=$(printf '%s\n' "$input" | jq -r '.model.display_name // empty')

# --- current directory: keep raw path for git, build abbreviated copy for display ---
raw_dir=$(printf '%s\n' "$input" | jq -r '.workspace.current_dir // empty')
dir="$raw_dir"
case "$dir" in
  "$HOME"/*) dir="~${dir#$HOME}" ;;
  "$HOME") dir="~" ;;
esac

# --- git branch (skip optional locks; omit segment if not a repo) ---
branch=""
if [ -n "$raw_dir" ]; then
  branch=$(git -C "$raw_dir" --no-optional-locks rev-parse --abbrev-ref HEAD 2>/dev/null)
fi

# --- context window usage (pre-calculated field; omit if null/empty) ---
ctx_used=$(printf '%s\n' "$input" | jq -r '.context_window.used_percentage // empty')

# --- rate limit remaining: prefer 5-hour session limit, fall back to 7-day weekly limit ---
# limit_resets_at is captured from the SAME window as limit_used (may be absent).
limit_used=$(printf '%s\n' "$input" | jq -r '.rate_limits.five_hour.used_percentage // empty')
limit_resets_at=$(printf '%s\n' "$input" | jq -r '.rate_limits.five_hour.resets_at // empty')
limit_label="left"
if [ -z "$limit_used" ]; then
  limit_used=$(printf '%s\n' "$input" | jq -r '.rate_limits.seven_day.used_percentage // empty')
  limit_resets_at=$(printf '%s\n' "$input" | jq -r '.rate_limits.seven_day.resets_at // empty')
  limit_label="left7d"
fi

# --- colors ---
c_reset=$(printf '\033[0m')
c_model=$(printf '\033[1;35m')   # magenta
c_dir=$(printf '\033[36m')       # cyan
c_branch_sym=$(printf '\033[1;34m') # blue
c_branch=$(printf '\033[32m')    # green
c_ctx=$(printf '\033[33m')       # yellow
c_limit=$(printf '\033[35m')     # purple
c_reset_time=$(printf '\033[38;5;245m') # medium gray

sep="  "

# --- line 1: cwd, branch appended to the right (omitted outside a git repo) ---
line1=""
if [ -n "$dir" ]; then
  line1="${c_dir}${dir}${c_reset}"
fi

if [ -n "$branch" ]; then
  [ -n "$line1" ] && line1="${line1}  "
  line1="${line1}${c_branch_sym}⎇${c_reset} ${c_branch}${branch}${c_reset}"
fi

# --- line 2: model  ctx  limit  resets-in (space-joined, no "|") ---
line2=""

if [ -n "$model" ]; then
  line2="${c_model}${model}${c_reset}"
fi

if [ -n "$ctx_used" ]; then
  ctx_rounded=$(printf '%.0f' "$ctx_used")
  [ -n "$line2" ] && line2="${line2}${sep}"
  line2="${line2}${c_ctx}ctx ${ctx_rounded}%${c_reset}"
fi

if [ -n "$limit_used" ]; then
  limit_remaining=$((100 - $(printf '%.0f' "$limit_used")))
  [ -n "$line2" ] && line2="${line2}${sep}"
  line2="${line2}${c_limit}${limit_label} ${limit_remaining}%${c_reset}"
fi

# --- resets-in: same window as the limit above; omit if absent or already passed ---
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

printf '%s\n%s\n' "$line1" "$line2"
