//! Read instance paths and target settings shared by producer binaries.

use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Result, anyhow};
use serde::Deserialize;

/// The values that `--instance` takes from the instance configuration
/// (ADR 0021 §The Producer Reads Its Instance).
#[derive(Debug)]
pub struct InstanceSettings {
    /// `$XDG_RUNTIME_DIR/musicindex-live-publisher/NAME/nowplaying`.
    pub drop_dir: PathBuf,
    /// The configured target name.
    pub target: String,
    /// The configured display directory, when enabled.
    pub display_dir: Option<PathBuf>,
}

/// The part of the instance configuration that the producer reads. The
/// producer ignores each other key. It never writes the file.
#[derive(Debug, Deserialize)]
struct InstanceFile {
    #[serde(default, rename = "target")]
    targets: Vec<InstanceTarget>,
}

#[derive(Debug, Deserialize)]
struct InstanceTarget {
    name: String,
    display_dir: Option<PathBuf>,
}

impl InstanceSettings {
    /// Reads the instance configuration of `name`.
    ///
    /// # Errors
    ///
    /// Returns an error when `XDG_RUNTIME_DIR` is not set, when the file is
    /// missing or not valid TOML, when it has not exactly one target, when
    /// the target name is not valid, or when `display_dir` is not the
    /// display directory of the instance. Each error names ADR 0021.
    pub fn load(name: &str, config_home: &Path, runtime_dir: Option<&Path>) -> Result<Self> {
        let runtime_dir = runtime_dir.ok_or_else(|| {
            anyhow!("ADR 0021 §The Producer Reads Its Instance: --instance needs XDG_RUNTIME_DIR")
        })?;
        let config_file = config_home
            .join("musicindex-live-publisher")
            .join(name)
            .join("config.toml");
        let instance_run = runtime_dir.join("musicindex-live-publisher").join(name);
        let raw = fs::read_to_string(&config_file).map_err(|source| {
            anyhow!(
                "ADR 0021 §The Producer Reads Its Instance: cannot read instance configuration {}: {source}",
                config_file.display()
            )
        })?;
        let file: InstanceFile = toml::from_str(&raw).map_err(|source| {
            anyhow!(
                "ADR 0021 §The Producer Reads Its Instance: cannot parse instance configuration {}: {source}",
                config_file.display()
            )
        })?;
        let mut targets = file.targets;
        if targets.len() != 1 {
            return Err(anyhow!(
                "ADR 0021 §The Producer Reads Its Instance: the instance configuration {} must have exactly one target, but it has {}",
                config_file.display(),
                targets.len()
            ));
        }
        let target = targets.remove(0);
        validate_target_name(&target.name).map_err(|source| {
            anyhow!(
                "ADR 0021 §The Producer Reads Its Instance: target in {}: {source}",
                config_file.display()
            )
        })?;
        let expected_display = instance_run.join("display");
        if let Some(display_dir) = target.display_dir.as_deref()
            && !same_directory(display_dir, &expected_display)
        {
            return Err(anyhow!(
                "ADR 0021 §The Producer Reads Its Instance: display_dir {} in {} must be {}",
                display_dir.display(),
                config_file.display(),
                expected_display.display()
            ));
        }
        Ok(Self {
            drop_dir: instance_run.join("nowplaying"),
            target: target.name,
            // The check above allows only the expected directory.
            display_dir: target.display_dir.map(|_| expected_display),
        })
    }
}

/// Applies the target name rule of the publisher (`src/config.rs`): the name
/// is not empty and has no control character. The producer also refuses a
/// path separator and a dot name, because the name is part of a file name.
fn validate_target_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(anyhow!("target name must not be empty"));
    }
    if name.chars().any(char::is_control) {
        return Err(anyhow!("target name must not contain control characters"));
    }
    // The drop file is `<target>.json` in the drop directory. A path
    // separator or a dot name can put that file outside the drop directory.
    if name.contains(['/', '\\']) || name == "." || name == ".." {
        return Err(anyhow!(
            "target name must not contain a path separator or be a dot name"
        ));
    }
    Ok(())
}

fn normalize(path: &Path) -> PathBuf {
    if let Ok(resolved) = path.canonicalize() {
        return resolved;
    }
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut clean = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                clean.pop();
            }
            other => clean.push(other.as_os_str()),
        }
    }
    clean
}

/// Tests whether two paths name the same directory.
///
/// The function resolves paths that exist. It normalizes absolute paths otherwise.
pub fn same_directory(a: &Path, b: &Path) -> bool {
    normalize(a) == normalize(b)
}
