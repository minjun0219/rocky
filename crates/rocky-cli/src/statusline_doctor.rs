//! `rocky statusline probe` · `doctor` — 한도 줄이 왜 안 나오는지, extra 줄이 왜 안 붙는지를 사람이 보는 명령. cc-usage `probe` ·
//! `doctor` 의 이식. statusline 은 실패를 조용히 삼키므로 "왜" 는 여기서만 보인다.
//!
//! 진단은 실제 동작과 **같은 함수**로 계산한다 — 계정·토큰 자리(`Slot`·`find_token`), extra 실행(`bounded::run_extra`), guard 의
//! 크레딧 판단(`limits::credits_enabled`). 따로 계산하면 둘이 어긋났을 때 화면이 실제 동작과 다른 말을 한다.

use std::path::Path;

use chrono::{DateTime, Local, SecondsFormat, Utc};
use rocky_core::claude_account;
use rocky_core::config::StatuslineConfig;
use rocky_core::limits::{credits_enabled, AllowFile, StateFile, UsageCache};
use rocky_core::statusline::extra::{describe, Vars};

use crate::statusline_cache::{read, state_file, usage_file, Slot};
use crate::statusline_refresh::{fetch_raw, find_token};

/// `probe` — usage API 원본 응답(필드 확인용). JSON 이면 들여 써서, 아니면 그대로.
pub fn probe(cfg: &StatuslineConfig, now: DateTime<Utc>) -> Result<String, String> {
    let slot =
        Slot::from_env(cfg.config_dir.as_deref()).ok_or("HOME 을 몰라 계정을 정할 수 없다")?;
    let found = find_token(cfg, &slot)?;
    let token = claude_account::check_token(found.token, now)?;
    let body = fetch_raw(&token.access_token, now).map_err(|f| f.message)?;
    Ok(match serde_json::from_slice::<serde_json::Value>(&body) {
        Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
        Err(_) => String::from_utf8_lossy(&body).into_owned(),
    })
}

/// `doctor [--session ID]` — 설정·계정·토큰·캐시·keychain 후보·extra 명령을 차례로 찍는다.
///
/// doctor 에는 Claude Code 세션이 없다 — `{{cwd}}` 는 지금 폴더로 채우고, `{{session_id}}` 는 `--session` 으로 받지 않으면 비워
/// 둔다(그 명령은 statusline 과 똑같이 건너뛴다고 나온다).
pub fn doctor(
    cfg: &StatuslineConfig,
    config_path: &Path,
    session: Option<&str>,
    now: DateTime<Utc>,
) -> Result<(), String> {
    if cfg!(target_os = "macos") {
        if let Some(services) = keychain_services() {
            println!("keychain 후보:  {}", services.join(", "));
        }
    }
    println!("config:        {}", config_path.display());
    println!("source:        {}", cfg.limits.source.as_str());
    let slot = Slot::from_env(cfg.config_dir.as_deref());
    let home = std::env::var("HOME").unwrap_or_default();
    let home = Path::new(&home);
    let (state, usage, account) = match &slot {
        Some(slot) => {
            println!("config_dir:    {}", slot.config_dir.display());
            let (account, bucket) = crate::statusline_guard::account_bucket(slot);
            match (&account, slot.account_paths.first()) {
                (Some(a), _) if !a.email.is_empty() => println!("계정:          {}", a.email),
                (Some(_), _) => println!("계정:          (이메일 없음 — API 키 인증 등)"),
                (None, Some(path)) => {
                    println!(
                        "계정:          (계정 파일을 못 읽었다 — {})",
                        path.display()
                    )
                }
                (None, None) => println!("계정:          (계정 파일 후보 없음)"),
            }
            let settings_dir = claude_account::config_dir(None, cfg.config_dir.as_deref(), home);
            match claude_account::keychain_service(
                cfg.keychain_service.as_deref(),
                &settings_dir,
                &slot.config_dir,
                home,
            ) {
                Some(service) => println!("keychain:      {service}"),
                // 빈 값은 설정 누락이 아니라 의도다 — 왜 건너뛰는지 화면에 적는다.
                None => {
                    println!("keychain:      (건너뜀 — 비기본 config_dir, creds 파일만 봅니다)")
                }
            }
            println!(
                "creds file:    {}",
                claude_account::credentials_file(
                    cfg.credentials_file.as_deref(),
                    &settings_dir,
                    &slot.config_dir,
                    home,
                )
                .display()
            );
            println!("cache dir:     {}", bucket.display());
            let state: StateFile = read(&state_file(&bucket));
            let usage: UsageCache = read(&usage_file(&bucket));
            (state, usage, account)
        }
        None => {
            println!("config_dir:    (HOME 을 몰라 계정·캐시를 정할 수 없다)");
            (StateFile::default(), UsageCache::default(), None)
        }
    };
    println!("guard:         {}", cfg.limits.guard);

    // guard 가 한도 소진에서 실제로 막을지는 크레딧이 켜져 있느냐에 달렸다 — guard 와 같은 함수로 계산한다.
    let hint = account.as_ref().and_then(|a| a.extra_usage_enabled);
    let observed = usage.usage.as_ref().is_some_and(|u| u.extra.is_some());
    let (state_text, from, blocks) = match credits_enabled(&usage, hint) {
        None => ("모름", "-", true), // 어느 쪽도 답을 내지 못했다
        Some(true) => (
            "켜짐",
            if observed {
                "usage.json"
            } else {
                ".claude.json"
            },
            true,
        ),
        Some(false) => (
            "꺼짐",
            if observed {
                "usage.json"
            } else {
                ".claude.json"
            },
            false,
        ),
    };
    // guard 가 꺼져 있으면 막고 안 막고를 말하지 않는다 — 어차피 아무것도 막지 않는다. 홈을 모르면 guard 는 캐시를 못 읽어
    // 막지 않는다(fail-open).
    let blocks = blocks && slot.is_some();
    match (cfg.limits.guard, blocks) {
        (false, _) => println!("크레딧:        {state_text} ({from})"),
        (true, true) => {
            println!("크레딧:        {state_text} ({from}) — 소진 시 guard 가 막습니다")
        }
        (true, false) => println!("크레딧:        {state_text} ({from}) — guard 가 막지 않습니다"),
    }
    println!(
        "alert:         임박 {:.0}% (0이면 소진만)",
        cfg.limits.alert()
    );

    match &slot {
        Some(slot) => match find_token(cfg, slot) {
            Ok(found) => {
                let expires = found.token.expires_at.map_or_else(
                    || "unknown".to_string(),
                    |at| {
                        at.with_timezone(&Local)
                            .to_rfc3339_opts(SecondsFormat::Secs, true)
                    },
                );
                match claude_account::check_token(found.token, now) {
                    Ok(_) => println!(
                        "token:         ok (source={}, expires={expires})",
                        found.source
                    ),
                    Err(e) => println!("token:         {e} (source={})", found.source),
                }
            }
            Err(e) => println!("token:         {e}"),
        },
        None => println!("token:         (HOME 을 몰라 찾지 않았다)"),
    }

    print_extras(cfg, session);

    let allow: AllowFile = crate::statusline_guard::allow_path()
        .map(|p| read(&p))
        .unwrap_or_default();
    let dump = serde_json::json!({ "state": state, "usage": usage, "allow": allow });
    println!(
        "{}",
        serde_json::to_string_pretty(&dump).unwrap_or_default()
    );
    Ok(())
}

/// extra 명령을 statusline 과 같은 경로로 돌려 무슨 일이 났는지 말한다.
fn print_extras(cfg: &StatuslineConfig, session: Option<&str>) {
    if cfg.extra_commands.is_empty() {
        println!(
            "extraCommands: 없음 (다른 도구 줄을 붙이는 법: docs/board.md \"statusline에 얹기\")"
        );
        return;
    }
    let cwd = std::env::current_dir()
        .map(|d| d.display().to_string())
        .unwrap_or_default();
    let session_id = session.unwrap_or_default();
    let shown = if session_id.is_empty() {
        "(비어 있음 — --session 으로 채운다)"
    } else {
        session_id
    };
    println!(
        "extraCommands: {}개  cwd={cwd}  session_id={shown}",
        cfg.extra_commands.len()
    );
    let vars = Vars {
        session_id,
        cwd: &cwd,
    };
    let results: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = cfg
            .extra_commands
            .iter()
            .map(|c| scope.spawn(move || crate::bounded::run_extra(c, vars)))
            .collect();
        handles.into_iter().map(|h| h.join().ok()).collect()
    });
    for (i, (command, result)) in cfg.extra_commands.iter().zip(results).enumerate() {
        println!("  [{}] {}", i + 1, command.command.join(" "));
        let Some((argv, probe)) = result else {
            println!("      (실행 스레드가 죽었다)");
            continue;
        };
        // 치환 결과가 원인일 때가 있다 — placeholder 가 있었으면 실제 argv 도 보인다.
        if let Some(argv) = argv.filter(|a| *a != command.command) {
            println!("      = {}", argv.join(" "));
        }
        println!("      {}", describe(&probe, command.timeout()));
    }
}

/// keychain 에서 Claude Code 토큰처럼 보이는 항목 이름들 — `security dump-keychain` 을 `-d` 없이 돌려 비밀은 찍지 않는다.
fn keychain_services() -> Option<Vec<String>> {
    let argv = ["/usr/bin/security", "dump-keychain"].map(String::from);
    let out = crate::bounded::run(&argv, std::time::Duration::from_secs(10))?;
    Some(keychain_services_from(&String::from_utf8_lossy(&out)))
}

/// `dump-keychain` 출력 → `"svce"<blob>="Claude Code…"` 의 이름들(처음 나온 순서, 중복 없이).
pub fn keychain_services_from(dump: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in dump.lines() {
        let Some(name) = line
            .trim()
            .strip_prefix(r#""svce"<blob>=""#)
            .map(|rest| rest.strip_suffix('"').unwrap_or(rest))
        else {
            continue;
        };
        if name.starts_with("Claude Code") && !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
    }
    out
}
