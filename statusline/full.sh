#!/bin/sh
# rocky-statusline-template: full
# description: 3줄 — duo + git 상태(dirty/↑↓) + 임계값 경고색 + 세션 비용(+시간당)/변경 라인 수/경과
#
# rocky statusline 템플릿. 설치본은 ~/.config/rocky/statusline.sh —
# /rocky:statusline 이 복사하고, SessionStart 훅이 헤더의 template 마커를 보고
# 플러그인 업데이트를 자동 전파한다. 커스터마이징은 이 파일(레포)을 고친다. deps: jq, git, awk.
#
#   line 1 = <cwd (~ abbreviated)>  ⎇ <branch><*>  ↑<ahead> ↓<behind>
#            (branch omitted outside a git repo; * = dirty worktree (untracked 포함);
#             ↑/↓ omitted when 0 or no upstream)
#   line 2 = <model>  ctx <used>%  left <remaining>%  <gray>(<human>)
#            ctx/left 는 임계값 색 — ctx <70% dim / 70%+ yellow / 90%+ red,
#            left >30% dim / 30%- yellow / 10%- red
#   line 3 = $<cost> <gray>($<rate>/h)  +<added>/-<removed>  <elapsed>
#            (rate 는 경과 5분 이상일 때만; each segment omitted when absent;
#             the whole line is omitted when every segment is empty)
# left = 100 - rate_limits.five_hour.used_percentage (falls back to seven_day → "left7d").
# resets_at is read from the SAME window used for the limit, Unix epoch seconds; the gray
# segment is "(<remaining>, <HH:MM>)" — remaining is human-adaptive (Nd Nh / Nh Nm / Nm)
# and HH:MM is the local reset clock time, e.g. (1h 30m, 18:00). The clock part is
# dropped if `date` can't format the epoch.

input=$(cat)
# 로캘 독립 숫자 처리 — 쉼표 소수점 로캘에서 printf '%.0f' 파싱 실패 방지
export LC_NUMERIC=C

model=$(printf '%s\n' "$input" | jq -r '.model.display_name // empty')

raw_dir=$(printf '%s\n' "$input" | jq -r '.workspace.current_dir // empty')
dir="$raw_dir"
case "$dir" in
  "$HOME"/*) dir="~${dir#$HOME}" ;;
  "$HOME") dir="~" ;;
esac

branch=""
dirty=""
ahead="" behind=""
if [ -n "$raw_dir" ]; then
  branch=$(git -C "$raw_dir" --no-optional-locks rev-parse --abbrev-ref HEAD 2>/dev/null)
  if [ -n "$branch" ]; then
    [ -n "$(git -C "$raw_dir" --no-optional-locks status --porcelain 2>/dev/null | head -1)" ] && dirty="*"
    # @{u}...HEAD --left-right --count → "<behind><TAB><ahead>" (upstream 없으면 실패 → 생략)
    ab=$(git -C "$raw_dir" --no-optional-locks rev-list --left-right --count '@{u}...HEAD' 2>/dev/null)
    if [ -n "$ab" ]; then
      # read 로 탭 구분 파싱 — set 으로 positional params 를 오염시키지 않는다
      read -r behind ahead <<EOF
$ab
EOF
      [ "$behind" = "0" ] && behind=""
      [ "$ahead" = "0" ] && ahead=""
    fi
  fi
fi

ctx_used=$(printf '%s\n' "$input" | jq -r '.context_window.used_percentage // empty')

limit_used=$(printf '%s\n' "$input" | jq -r '.rate_limits.five_hour.used_percentage // empty')
limit_resets_at=$(printf '%s\n' "$input" | jq -r '.rate_limits.five_hour.resets_at // empty')
limit_label="left"
if [ -z "$limit_used" ]; then
  limit_used=$(printf '%s\n' "$input" | jq -r '.rate_limits.seven_day.used_percentage // empty')
  limit_resets_at=$(printf '%s\n' "$input" | jq -r '.rate_limits.seven_day.resets_at // empty')
  limit_label="left7d"
fi

# --- session cost / churn / elapsed (line 3 sources; each may be absent) ---
cost_usd=$(printf '%s\n' "$input" | jq -r '.cost.total_cost_usd // empty')
lines_added=$(printf '%s\n' "$input" | jq -r '.cost.total_lines_added // empty')
lines_removed=$(printf '%s\n' "$input" | jq -r '.cost.total_lines_removed // empty')
duration_ms=$(printf '%s\n' "$input" | jq -r '.cost.total_duration_ms // empty')

c_reset=$(printf '\033[0m')
c_model=$(printf '\033[1;35m')   # magenta
c_dir=$(printf '\033[36m')       # cyan
c_branch_sym=$(printf '\033[1;34m') # blue
c_branch=$(printf '\033[32m')    # green
c_dirty=$(printf '\033[33m')     # yellow
c_dim=$(printf '\033[38;5;245m') # medium gray (safe zone)
c_warn=$(printf '\033[33m')      # yellow (warning zone)
c_danger=$(printf '\033[1;31m')  # bold red (danger zone)
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
  [ -n "$dirty" ] && line1="${line1}${c_dirty}*${c_reset}"
  if [ -n "$ahead" ] || [ -n "$behind" ]; then
    ab_text=""
    [ -n "$ahead" ] && ab_text="↑${ahead}"
    if [ -n "$behind" ]; then
      [ -n "$ab_text" ] && ab_text="${ab_text} "
      ab_text="${ab_text}↓${behind}"
    fi
    line1="${line1}  ${c_dim}${ab_text}${c_reset}"
  fi
fi

# --- line 2: model  ctx  limit  resets-in ---
line2=""
[ -n "$model" ] && line2="${c_model}${model}${c_reset}"

if [ -n "$ctx_used" ]; then
  ctx_rounded=$(printf '%.0f' "$ctx_used")
  ctx_color="$c_dim"
  [ "$ctx_rounded" -ge 70 ] && ctx_color="$c_warn"
  [ "$ctx_rounded" -ge 90 ] && ctx_color="$c_danger"
  [ -n "$line2" ] && line2="${line2}${sep}"
  line2="${line2}${ctx_color}ctx ${ctx_rounded}%${c_reset}"
fi

if [ -n "$limit_used" ]; then
  limit_remaining=$((100 - $(printf '%.0f' "$limit_used")))
  limit_color="$c_dim"
  [ "$limit_remaining" -le 30 ] && limit_color="$c_warn"
  [ "$limit_remaining" -le 10 ] && limit_color="$c_danger"
  [ -n "$line2" ] && line2="${line2}${sep}"
  line2="${line2}${limit_color}${limit_label} ${limit_remaining}%${c_reset}"
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
  cost_text=$(printf '$%.2f' "$cost_usd")
  line3="${c_cost}${cost_text}${c_reset}"
  case "$duration_ms" in
    ''|*[!0-9]*) : ;; # not a plain integer, skip
    *)
      # 시간당 비용 — 경과 5분 미만이면 초반 왜곡이 커서 생략
      if [ "$duration_ms" -ge 300000 ]; then
        rate=$(awk -v c="$cost_usd" -v ms="$duration_ms" 'BEGIN { printf "%.1f", c / (ms / 3600000) }')
        line3="${line3} ${c_elapsed}(\$${rate}/h)${c_reset}"
      fi
      ;;
  esac
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
