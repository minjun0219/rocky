//! `extraCommands` — 다른 도구의 statusline 줄을 rocky 줄 아래에 그대로 덧붙인다. cc-usage `internal/extra` 의 이식.
//! 여기는 순수한 부분(placeholder 치환·건너뛰기·출력 줄 정리)이고, 실행(병렬·마감·그룹 kill)은 CLI 몫이다.
//!
//! rocky 는 그 명령들이 무엇인지 모른다 — 명령·placeholder·마감만 알고, 어떤 도구를 부를지는 설정에만 있다.

use std::time::Duration;

/// 설정에 마감이 없거나 0 이하일 때 — statusline 은 매 렌더 도니 짧게 둔다.
pub const DEFAULT_EXTRA_TIMEOUT_MS: u64 = 300;

/// 덧붙일 명령 하나 — `rocky.json` 의 `statusline.extraCommands[]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtraCommand {
    /// argv 배열(셸을 거치지 않는다). 원소마다 `{{cwd}}` · `{{session_id}}` 를 치환한다.
    pub command: Vec<String>,
    pub timeout_ms: Option<u64>,
}

impl ExtraCommand {
    pub fn timeout(&self) -> Duration {
        Duration::from_millis(
            self.timeout_ms
                .filter(|t| *t > 0)
                .unwrap_or(DEFAULT_EXTRA_TIMEOUT_MS),
        )
    }
}

/// placeholder 에 넣을 값.
#[derive(Debug, Clone, Copy)]
pub struct Vars<'a> {
    pub session_id: &'a str,
    pub cwd: &'a str,
}

/// 치환한 argv. command 가 비었거나, 쓰인 placeholder 의 값이 비었으면 `None`(건너뜀) — 빈 자리를 채워 부르면
/// 무의미한 조회가 된다. 치환은 이름 순서(`{{cwd}}` → `{{session_id}}`)로 차례로 한다.
pub fn expand(argv: &[String], vars: Vars) -> Option<Vec<String>> {
    expand_checked(argv, vars).ok()
}

/// `expand` 와 같고, 건너뛸 때 이유를 준다 — `Err(None)` 은 command 가 비었다, `Err(Some(키))` 는 그 placeholder 가 비었다.
/// doctor 가 "왜 안 붙는가" 를 말하는 데 쓴다.
pub fn expand_checked(argv: &[String], vars: Vars) -> Result<Vec<String>, Option<&'static str>> {
    if argv.is_empty() {
        return Err(None);
    }
    let pairs = [("{{cwd}}", vars.cwd), ("{{session_id}}", vars.session_id)];
    argv.iter()
        .map(|arg| {
            let mut arg = arg.clone();
            for (key, value) in pairs {
                if !arg.contains(key) {
                    continue;
                }
                if value.is_empty() {
                    return Err(Some(key));
                }
                arg = arg.replace(key, value);
            }
            Ok(arg)
        })
        .collect()
}

/// 명령 하나를 statusline 과 같은 경로로 돌린 결과 — statusline 은 줄만 쓰고, 나머지는 doctor 가 "왜 안 붙는가" 를 말하는 데 쓴다.
#[derive(Debug, Clone, PartialEq)]
pub enum Probe {
    /// 붙을 줄(`output_lines`).
    Ok {
        lines: Vec<Vec<u8>>,
        elapsed: Duration,
    },
    /// 0 으로 끝났지만 붙일 줄이 없다.
    Empty {
        elapsed: Duration,
    },
    /// 돌리지 않았다 — 비어 있던 placeholder(`None` 이면 command 자체가 비었다).
    Skipped {
        missing: Option<&'static str>,
    },
    NotFound(String),
    Timeout,
    /// 0 이 아닌 코드로 끝났거나, 끝난 뒤에도 자식이 파이프를 붙잡았다. `stderr` 는 첫 줄.
    Failed {
        error: String,
        stderr: String,
    },
}

impl Probe {
    /// statusline 에 붙을 줄 — 성공이 아니면 없다.
    pub fn into_lines(self) -> Vec<Vec<u8>> {
        match self {
            Probe::Ok { lines, .. } => lines,
            _ => Vec::new(),
        }
    }
}

/// doctor 가 찍는 한 줄 판정(cc-usage `extra.Describe`).
pub fn describe(p: &Probe, timeout: Duration) -> String {
    let ms = |d: &Duration| format!("{}ms", d.as_millis());
    match p {
        Probe::Ok { lines, elapsed } => {
            let more = if lines.len() > 1 {
                format!(" (외 {}줄)", lines.len() - 1)
            } else {
                String::new()
            };
            let first = lines
                .first()
                .map(|l| String::from_utf8_lossy(l))
                .unwrap_or_default();
            format!("ok {} → {first}{more}", ms(elapsed))
        }
        Probe::Empty { elapsed } => {
            format!(
                "출력 없음 — exit 0 이지만 stdout 이 비었다 ({})",
                ms(elapsed)
            )
        }
        Probe::Skipped { missing: None } => "건너뜀 — command 가 비어 있다".to_string(),
        Probe::Skipped { missing: Some(key) } => format!("건너뜀 — {key} 가 비어 있다"),
        Probe::NotFound(e) => format!("미설치 — {e}"),
        Probe::Timeout => format!(
            "타임아웃 — {} 를 넘겼다 (timeoutMs 로 늘릴 수 있다)",
            ms(&timeout)
        ),
        Probe::Failed { error, stderr } if stderr.is_empty() => format!("비정상 종료 — {error}"),
        Probe::Failed { error, stderr } => format!("비정상 종료 — {error}: {stderr}"),
    }
}

/// 명령의 stdout → 붙일 줄. 끝의 줄바꿈을 떼고 공백뿐인 줄은 뺀다. 나머지는 **바이트 그대로**다 — ANSI 는 물론
/// UTF-8 이 아닌 출력도 cc-usage 처럼 손대지 않고 붙인다.
pub fn output_lines(stdout: &[u8]) -> Vec<Vec<u8>> {
    let mut end = stdout.len();
    while end > 0 && stdout[end - 1] == b'\n' {
        end -= 1;
    }
    stdout[..end]
        .split(|b| *b == b'\n')
        // 공백 판정만 글자로 한다 — 잘못된 바이트는 공백이 아니다(Go 의 TrimSpace 와 같다).
        .filter(|line| !String::from_utf8_lossy(line).trim().is_empty())
        .map(<[u8]>::to_vec)
        .collect()
}
