//! Guards for the packaged systemd units (ADR 0021 §The Unit Names).

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

fn has_line(text: &str, wanted: &str) -> bool {
    text.lines().any(|l| l.trim() == wanted)
}

#[test]
fn producer_template_has_required_lines() {
    let unit = read("systemd/mixxx-now-playing@.service");
    for wanted in [
        "ExecStart=/usr/bin/mixxx-now-playing --format json --instance %i",
        "Wants=musicindex-live-publisher@%i.service",
        "After=musicindex-live-publisher@%i.service",
        "RestartPreventExitStatus=3",
        "RuntimeDirectory=musicindex-live-publisher/%i/nowplaying musicindex-live-publisher/%i/display mixxx-now-playing",
    ] {
        assert!(
            has_line(&unit, wanted),
            "ADR 0021 §The Unit Names: systemd/mixxx-now-playing@.service must contain `{wanted}`."
        );
    }
}

#[test]
fn old_producer_unit_is_gone() {
    assert!(
        !root().join("systemd/mixxx-now-playing.service").exists(),
        "ADR 0021 §The Unit Names: systemd/mixxx-now-playing.service must not exist. Use the template unit."
    );
}

#[test]
fn publisher_template_reads_config_from_xdg_config_specifier() {
    let unit = read("systemd/musicindex-live-publisher@.service");
    assert!(
        unit.contains("--config %E/musicindex-live-publisher/%i/config.toml"),
        "ADR 0021 §The Unit Names: the publisher template must read --config from %E/musicindex-live-publisher/%i/config.toml."
    );
}

#[test]
fn units_hold_no_fixed_home_or_run_user_path() {
    let dir = root().join("systemd");
    for entry in fs::read_dir(&dir).expect("read systemd/") {
        let path = entry.expect("dir entry").path();
        if !path.is_file() {
            continue;
        }
        let text = fs::read_to_string(&path).expect("read unit");
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            !text.contains("/home/"),
            "{name}: a unit must not hold a home path. Use %h, %E or %S."
        );
        // A comment may name the pattern `/run/user/<uid>`. A digit must not follow.
        let fixed_run_user = text.match_indices("/run/user").any(|(at, m)| {
            let rest = text[at + m.len()..].trim_start_matches('/');
            rest.starts_with(|c: char| c.is_ascii_digit())
        });
        assert!(
            !fixed_run_user,
            "{name}: a unit must not hold a /run/user path with a user number. Use %t."
        );
    }
}

#[test]
fn packages_name_the_template_unit_only() {
    for rel in ["Cargo.toml", "packaging/arch/PKGBUILD"] {
        let text = read(rel);
        assert!(
            text.contains("systemd/mixxx-now-playing@.service"),
            "ADR 0021 §The Unit Names: {rel} must name systemd/mixxx-now-playing@.service."
        );
        assert!(
            !text.contains("systemd/mixxx-now-playing.service"),
            "ADR 0021 §The Unit Names: {rel} must not name systemd/mixxx-now-playing.service."
        );
    }
    assert!(Path::new(&root()).join("systemd").is_dir());
}
