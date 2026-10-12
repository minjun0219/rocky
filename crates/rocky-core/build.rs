//! 빌드한 커밋의 짧은 해시를 `ROCKY_GIT_COMMIT` 으로 굽는다 — 버전 옆 `(abc1234)` 표시용
//! (`rocky_core::version::GIT_COMMIT`).
//!
//! **release 빌드에만 굽는다.** rocky-core 는 모든 크레이트가 기대는 바닥이라, 해시가 바뀌면 워크스페이스
//! 전체가 다시 컴파일된다. dev 빌드(테스트·clippy·pre-push·`rocky verify`)까지 구우면 Rust 를 안 고친 커밋
//! 뒤에도 매번 통째로 빌드한다. 설치되는 바이너리(릴리스 tarball·main 빌드)는 모두 release 라 표시에는 지장이 없다.
//! git 이 없거나 레포 밖이거나 커밋이 바뀌는 자리를 못 찾으면 굽지 않는다 — 낡은 해시는 없는 것보다 나쁘다.

use std::path::Path;
use std::process::Command;

/// `git <args>` 의 stdout 한 줄. 실패하면 None — 빌드를 막지 않는다. 바깥 git(훅·래퍼)의 `GIT_*` 가 새어
/// 들어오면 다른 레포의 HEAD 를 읽으므로 지우고 부른다.
fn git(dir: &str, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let line = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!line.is_empty()).then_some(line)
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("PROFILE").as_deref() != Ok("release") {
        return;
    }
    let Ok(dir) = std::env::var("CARGO_MANIFEST_DIR") else {
        return;
    };

    // 커밋이 바뀌면 다시 굽는다. 파일을 하나라도 지정하면 cargo 는 "패키지 파일이 바뀌면" 기본 규칙을 끄므로,
    // HEAD 가 움직이는 자리를 모두 건다: 분리된 HEAD 는 HEAD 파일, 브랜치 커밋은 refs/heads 아래 파일
    // (디렉터리를 주면 cargo 가 안을 훑는다), 압축된 ref 는 packed-refs. 워크트리면 HEAD 는 워크트리의
    // git 디렉터리에, refs 는 공용 디렉터리에 있다. `--path-format` 은 git 2.31+ — 없으면 굽지 않는다.
    let abs = ["rev-parse", "--path-format=absolute"];
    let (Some(git_dir), Some(common)) = (
        git(&dir, &[&abs[..], &["--git-dir"]].concat()),
        git(&dir, &[&abs[..], &["--git-common-dir"]].concat()),
    ) else {
        return;
    };
    let Some(commit) = git(&dir, &["rev-parse", "--short=7", "HEAD"]) else {
        return;
    };
    println!("cargo:rerun-if-changed={git_dir}/HEAD");
    for watched in ["refs/heads", "packed-refs"] {
        let path = Path::new(&common).join(watched);
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    println!("cargo:rustc-env=ROCKY_GIT_COMMIT={commit}");
}
