//! `git status --porcelain=v2 --branch --untracked-files=no` 한 번의 출력 → 브랜치 세그먼트 재료.
//! cc-usage `internal/git` 의 이식. 실행(타임아웃 포함)은 CLI 몫이고 여기는 파싱만 한다.

/// 브랜치명·업스트림·ahead/behind·변경 수. 추가 git 호출 없이 porcelain=v2 한 번으로 다 나온다.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitStatus {
    /// `"(detached)"` 이면 `detached` 가 true.
    pub branch: String,
    pub detached: bool,
    /// detached 일 때 보여줄 커밋 — 첫 커밋 전이면 빈 값.
    pub oid: String,
    pub has_upstream: bool,
    pub ahead: u32,
    pub behind: u32,
    pub staged: u32,
    pub unstaged: u32,
    pub conflicted: u32,
}

impl GitStatus {
    pub fn parse(out: &str) -> GitStatus {
        let mut s = GitStatus::default();
        for line in out.split('\n').filter(|l| !l.is_empty()) {
            let Some(header) = line.strip_prefix("# ") else {
                s.count(line);
                continue;
            };
            let (key, val) = header.split_once(' ').unwrap_or((header, ""));
            match key {
                "branch.oid" if val != "(initial)" => s.oid = val.to_string(),
                "branch.head" => {
                    s.detached = val == "(detached)";
                    s.branch = val.to_string();
                }
                "branch.upstream" => s.has_upstream = !val.is_empty(),
                "branch.ab" => {
                    if let Some((a, b)) = val.split_once(' ') {
                        s.ahead = unsigned(a);
                        s.behind = unsigned(b);
                    }
                }
                _ => {}
            }
        }
        s
    }

    /// 변경 줄 하나 — `u` 는 충돌, `1`/`2` 는 `XY` 필드로 staged/unstaged 를 가른다.
    fn count(&mut self, line: &str) {
        let Some((kind, rest)) = line.split_once(' ') else {
            return;
        };
        if kind == "u" {
            self.conflicted += 1;
            return;
        }
        if kind != "1" && kind != "2" {
            return;
        }
        let xy = rest.split(' ').next().unwrap_or_default().as_bytes();
        if xy.len() != 2 {
            return;
        }
        if xy[0] != b'.' {
            self.staged += 1;
        }
        if xy[1] != b'.' {
            self.unstaged += 1;
        }
    }
}

/// `+1` / `-2` → 1 / 2. 읽을 수 없으면 0.
fn unsigned(v: &str) -> u32 {
    let v = v.strip_prefix('+').unwrap_or(v);
    v.strip_prefix('-').unwrap_or(v).parse().unwrap_or(0)
}
