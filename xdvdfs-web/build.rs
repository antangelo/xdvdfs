use std::process::Command;

fn main() {
    let rev_parse = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .unwrap();
    let hash = String::from_utf8(rev_parse.stdout).unwrap();
    println!("cargo:rustc-env=GIT_SHA={hash}");
}
