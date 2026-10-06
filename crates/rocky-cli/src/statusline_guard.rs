//! `rocky statusline guard` · `allow` — 한도가 소진돼 크레딧이 차감되기 시작하면 prompt 를 막는 `UserPromptSubmit` 훅과, 그것을
//! 잠시 끄는 명령. cc-usage `guard` · `allow` 의 이식. 판정은 `rocky_core::limits::guard`, 여기는 읽고 쓰기만 한다.
//!
//! guard 는 **fail-open** 이다 — 꺼져 있거나(`statusline.guard`), `source: none` 이거나, 홈·캐시를 모르면 막지 않는다. 설정
//! 파일의 `source` 만 본다: `ROCKY_STATUSLINE_SOURCE` 는 프로세스 트리로 물려 내려가서, 다른 호스트 때문에 셸에 걸어 둔 `none`
//! 이 같은 셸에서 띄운 Claude Code 의 guard 를 아무 표시 없이 끈다.

use std::path::PathBuf;

use chrono::{DateTime, Local, TimeDelta, Utc};
use rocky_core::claude_account;
use rocky_core::config::StatuslineConfig;
use rocky_core::limits::{
    account_cached, guard, parse_go_duration, select, AccountCache, AllowFile, Input, Source,
    StateFile, UsageCache,
};

use crate::statusline_cache::{read, state_file, usage_file, write, Slot};

/// 막을 때 사용자에게 보일 문구(stderr) — 훅이 exit 2 로 끝나면 Claude Code 가 prompt 를 버리고 이것을 보인다.
pub fn block_message(reason: &str) -> String {
    format!("[rocky] {reason}.\n계속하려면 터미널에서: rocky statusline allow 30m")
}

/// 이 prompt 를 막나 — 막으면 그 이유. 훅의 stdin(prompt JSON)은 읽어서 버린다.
///
/// 계정은 statusline 과 같은 규칙으로 정한다(세션 환경의 설정 폴더 → 계정 파일의 이메일 → 그 계정의 캐시). 계정 파일은 prompt
/// 당 한 번 읽는다 — statusline 과 달리 초당 두 번 도는 자리가 아니다. 못 읽으면 statusline 이 남긴 계정 캐시로 간다.
pub fn check(cfg: &StatuslineConfig, now: DateTime<Utc>) -> Option<String> {
    discard_stdin();
    let limits = &cfg.limits;
    if !limits.guard || limits.source == Source::None {
        return None;
    }
    let slot = Slot::from_env(cfg.config_dir.as_deref())?;
    let (account, bucket) = account_bucket(&slot);
    let state: StateFile = read(&state_file(&bucket));
    let usage: UsageCache = read(&usage_file(&bucket));
    let allow: AllowFile = allow_path().map(|p| read(&p)).unwrap_or_default();
    let lim = select(limits, &Input::default(), &state, &usage, now)?;
    let hint = account.and_then(|a| a.extra_usage_enabled);
    guard(limits, &lim, &usage, &allow, hint, now)
}

/// guard 가 볼 계정과 그 캐시 폴더 — 계정 파일을 직접 읽고, 못 읽으면 statusline 이 남긴 계정 캐시의 이메일로. doctor 도 이것을
/// 써서 guard 가 실제로 읽는 폴더를 보인다.
pub fn account_bucket(slot: &Slot) -> (Option<claude_account::AccountFile>, PathBuf) {
    let account = slot.read_account();
    let email = match &account {
        Some(a) => Some(a.email.clone()),
        None => {
            let cached: AccountCache = read(&slot.account_file());
            account_cached(&slot.account_source(), &cached).map(str::to_owned)
        }
    };
    let bucket = slot.bucket(email.as_deref());
    (account, bucket)
}

/// `allow [DURATION|off]` — 기본 30분. `off`·`0` 이면 guard 를 다시 켠다. 돌려주는 문구를 stdout 에 낸다.
pub fn allow(arg: Option<&str>, now: DateTime<Utc>) -> Result<String, String> {
    let arg = arg.unwrap_or("30m");
    let path = allow_path().ok_or("HOME 을 몰라 allow.json 자리를 정할 수 없다")?;
    let (file, message) = if arg == "off" || arg == "0" {
        (
            AllowFile::default(),
            "[rocky] guard 다시 활성화".to_string(),
        )
    } else {
        let until = parse_go_duration(arg)
            .filter(|d| *d > TimeDelta::zero())
            .and_then(|d| now.checked_add_signed(d))
            .ok_or_else(|| format!("invalid duration {arg:?} (예: 30m, 2h)"))?;
        let message = format!(
            "[rocky] {}까지 크레딧 사용 허용",
            until.with_timezone(&Local).format("%H:%M")
        );
        (
            AllowFile {
                allow_until: Some(until),
            },
            message,
        )
    };
    write(&path, &file).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(message)
}

/// `<cache>/rocky/statusline/allow.json` — 계정과 상관없이 하나.
pub fn allow_path() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var("HOME").ok().filter(|h| !h.is_empty())?);
    let root = claude_account::cache_root(std::env::var("XDG_CACHE_HOME").ok().as_deref(), &home);
    Some(claude_account::allow_file(&root))
}

/// 훅 입력을 버린다 — 터미널에서 그냥 부르면(stdin 이 TTY) 읽지 않는다. 4MB 까지만.
fn discard_stdin() {
    use std::io::{IsTerminal, Read};
    if std::io::stdin().is_terminal() {
        return;
    }
    let _ = std::io::copy(&mut std::io::stdin().take(4 << 20), &mut std::io::sink());
}
