//! The project-baseline ADR link guard runs in each `cargo test --workspace`.
//!
//! The project-baseline skill owns the guard (§A Binding Rule Lives In A
//! Decision Record). The CI runner has no `~/.agents` folder. On CI, the
//! test passes when the script is missing. On a local computer, a missing
//! script is a failure.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn adr_link_guard_passes() {
    let home = std::env::var_os("HOME").expect("HOME is set");
    let script =
        PathBuf::from(home).join(".agents/skills/project-baseline/scripts/check_adr_links.py");
    if !script.is_file() {
        assert!(
            std::env::var_os("CI").is_some(),
            "project-baseline §A Binding Rule Lives In A Decision Record: {} is missing. \
             Install the shared skills in ~/.agents/skills.",
            script.display()
        );
        eprintln!(
            "CI: {} is missing. The ADR guard did not run.",
            script.display()
        );
        return;
    }

    let output = Command::new("python3")
        .arg(&script)
        .arg("--counts")
        .arg(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("python3 starts");
    assert!(
        output.status.success(),
        "project-baseline §A Binding Rule Lives In A Decision Record: the ADR link guard \
         failed. Correct each ADR that the output names.\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
