//! Service configuration and token loading.

use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::{env, fs};

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

use crate::{FallbackConfig, LiveValueDestination, WatchTarget};

/// Default service configuration path.
pub const DEFAULT_CONFIG_PATH: &str = "/etc/musicindex-live-publisher/config.toml";

const EVENT_ID_PLACEHOLDERS: &[&str] = &[
    "replace-with-provisioned-event-guid",
    "the-provisioned-event-guid",
    "<event_id>",
];

/// Runtime service configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct PublisherConfig {
    pub watch_dir: PathBuf,
    pub endpoint: String,
    pub targets: Vec<PublisherTarget>,
}

/// A configured publishing target with its loaded broadcaster token.
#[derive(Clone, PartialEq)]
pub struct PublisherTarget {
    pub name: String,
    pub event_id: String,
    pub token_file: PathBuf,
    pub token: String,
    pub fallback: FallbackConfig,
}

impl fmt::Debug for PublisherTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PublisherTarget")
            .field("name", &self.name)
            .field("event_id", &self.event_id)
            .field("token_file", &self.token_file)
            .field("token", &"<redacted>")
            .field("fallback", &self.fallback)
            .finish()
    }
}

impl PublisherTarget {
    /// Returns the watcher-facing target data.
    pub fn watch_target(&self) -> WatchTarget {
        WatchTarget {
            name: self.name.clone(),
            event_guid: self.event_id.clone(),
            fallback: self.fallback.clone(),
        }
    }
}

/// CLI overrides that take precedence over config fields.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConfigOverrides {
    pub watch_dir: Option<PathBuf>,
    pub endpoint: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    watch_dir: PathBuf,
    endpoint: String,
    #[serde(rename = "target")]
    targets: Vec<RawTarget>,
}

#[derive(Debug, Deserialize)]
struct RawTarget {
    name: String,
    event_id: String,
    token_file: PathBuf,
    fallback: Option<RawFallback>,
}

#[derive(Debug, Deserialize)]
struct RawFallback {
    title: String,
    image: Option<String>,
    destinations: Vec<LiveValueDestination>,
}

/// Loads, validates, and resolves token files for a service config.
///
/// # Errors
///
/// Returns an error when the config file cannot be read, TOML is invalid, a
/// validation rule fails, or a token file is missing or empty.
pub fn load_config(path: &Path, overrides: ConfigOverrides) -> Result<PublisherConfig> {
    let bytes = fs::read(path).with_context(|| format!("read config file {}", path.display()))?;
    load_config_bytes(&bytes, Some(path), overrides)
}

/// Loads config from bytes, resolving token files from disk.
///
/// This helper is public for integration tests and small embedding tools.
///
/// # Errors
///
/// Returns an error when TOML parsing, validation, or token loading fails.
pub fn load_config_bytes(
    bytes: &[u8],
    source_path: Option<&Path>,
    overrides: ConfigOverrides,
) -> Result<PublisherConfig> {
    let text = std::str::from_utf8(bytes).with_context(|| {
        source_path
            .map(|path| format!("read config TOML as UTF-8 {}", path.display()))
            .unwrap_or_else(|| "read config TOML as UTF-8".to_owned())
    })?;
    let raw: RawConfig = toml::from_str(text).with_context(|| {
        source_path
            .map(|path| format!("parse config TOML {}", path.display()))
            .unwrap_or_else(|| "parse config TOML".to_owned())
    })?;
    resolve_config(raw, overrides)
}

fn resolve_config(raw: RawConfig, overrides: ConfigOverrides) -> Result<PublisherConfig> {
    if raw.targets.is_empty() {
        return Err(anyhow!("config must define at least one target"));
    }

    let mut seen = HashSet::new();
    let targets = raw
        .targets
        .into_iter()
        .map(|target| resolve_target(target, &mut seen))
        .collect::<Result<Vec<_>>>()?;

    Ok(PublisherConfig {
        watch_dir: overrides.watch_dir.unwrap_or(raw.watch_dir),
        endpoint: overrides.endpoint.unwrap_or(raw.endpoint),
        targets,
    })
}

fn resolve_target(target: RawTarget, seen: &mut HashSet<String>) -> Result<PublisherTarget> {
    if target.name.trim().is_empty() {
        return Err(anyhow!("target name must not be empty"));
    }
    if !seen.insert(target.name.clone()) {
        return Err(anyhow!("duplicate target name {}", target.name));
    }
    validate_event_id(&target.name, &target.event_id)?;

    let fallback = target
        .fallback
        .ok_or_else(|| anyhow!("target {} must configure fallback", target.name))?;
    validate_fallback(&target.name, &fallback)?;

    let token_file = resolve_token_file_path(
        &target.token_file,
        env::var_os("CREDENTIALS_DIRECTORY").as_ref().map(Path::new),
        env::var_os("HOME").as_ref().map(Path::new),
    )?;
    let token = load_token_file(&token_file)?;

    Ok(PublisherTarget {
        name: target.name,
        event_id: target.event_id,
        token_file,
        token,
        fallback: FallbackConfig {
            title: fallback.title,
            image: fallback.image,
            destinations: fallback.destinations,
        },
    })
}

fn validate_event_id(target_name: &str, event_id: &str) -> Result<()> {
    let event_id = event_id.trim();
    if event_id.is_empty() {
        return Err(anyhow!("target {target_name} event_id must not be empty"));
    }
    if EVENT_ID_PLACEHOLDERS.contains(&event_id)
        || event_id.contains("replace-with")
        || event_id.contains("provisioned")
    {
        return Err(anyhow!(
            "target {target_name} event_id is still an example placeholder; run provision and paste the returned event_id"
        ));
    }
    Ok(())
}

fn resolve_token_file_path(
    token_file: &Path,
    credentials_directory: Option<&Path>,
    home_directory: Option<&Path>,
) -> Result<PathBuf> {
    let Some(raw) = token_file.to_str() else {
        return Ok(token_file.to_path_buf());
    };

    if let Some(rest) = raw.strip_prefix("%d/") {
        let credentials_directory = credentials_directory
            .ok_or_else(|| anyhow!("token_file {raw:?} requires CREDENTIALS_DIRECTORY"))?;
        if rest.is_empty() {
            return Err(anyhow!("token_file {raw:?} must name a credential"));
        }
        return Ok(credentials_directory.join(rest));
    }

    if raw == "~" || raw.starts_with("~/") {
        let home_directory =
            home_directory.ok_or_else(|| anyhow!("token_file {raw:?} requires HOME"))?;
        let rest = raw.strip_prefix("~/").unwrap_or_default();
        return Ok(home_directory.join(rest));
    }

    Ok(token_file.to_path_buf())
}

fn validate_fallback(target_name: &str, fallback: &RawFallback) -> Result<()> {
    if fallback.destinations.is_empty() {
        return Err(anyhow!(
            "target {target_name} fallback destinations must not be empty"
        ));
    }

    for (index, destination) in fallback.destinations.iter().enumerate() {
        validate_required_destination_field(
            target_name,
            index,
            "name",
            destination.name.as_deref(),
        )?;
        validate_required_destination_field(
            target_name,
            index,
            "type",
            destination.kind.as_deref(),
        )?;
        validate_required_destination_field(
            target_name,
            index,
            "address",
            destination.address.as_deref(),
        )?;
        let split = validate_required_destination_field(
            target_name,
            index,
            "split",
            destination.split.as_deref(),
        )?;
        let parsed = split.parse::<f64>().with_context(|| {
            format!("target {target_name} fallback destination {index} split must be decimal")
        })?;
        if !parsed.is_finite() {
            return Err(anyhow!(
                "target {target_name} fallback destination {index} split must be finite"
            ));
        }
    }

    Ok(())
}

fn validate_required_destination_field<'a>(
    target_name: &str,
    index: usize,
    field: &str,
    value: Option<&'a str>,
) -> Result<&'a str> {
    let Some(value) = value else {
        return Err(anyhow!(
            "target {target_name} fallback destination {index} missing {field}"
        ));
    };
    if value.trim().is_empty() {
        return Err(anyhow!(
            "target {target_name} fallback destination {index} {field} must not be empty"
        ));
    }
    Ok(value)
}

fn load_token_file(path: &Path) -> Result<String> {
    warn_if_token_file_permissive(path);
    let token = fs::read_to_string(path)
        .with_context(|| format!("read token file {}", path.display()))?
        .trim_end_matches(char::is_whitespace)
        .to_owned();

    if token.is_empty() {
        return Err(anyhow!("token file {} is empty", path.display()));
    }

    Ok(token)
}

#[cfg(unix)]
fn warn_if_token_file_permissive(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    match fs::metadata(path) {
        Ok(metadata) => {
            let mode = metadata.permissions().mode() & 0o777;
            if mode & 0o077 != 0 {
                tracing::warn!(
                    path = %path.display(),
                    mode = format_args!("{mode:03o}"),
                    "token file permissions are more permissive than 0600"
                );
            }
        }
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                %error,
                "could not inspect token file permissions"
            );
        }
    }
}

#[cfg(not(unix))]
fn warn_if_token_file_permissive(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_token_path_expands_from_credentials_directory() -> Result<()> {
        let path = resolve_token_file_path(
            Path::new("%d/default.token"),
            Some(Path::new(
                "/run/credentials/musicindex-live-publisher.service",
            )),
            None,
        )?;

        assert_eq!(
            path,
            Path::new("/run/credentials/musicindex-live-publisher.service/default.token")
        );
        Ok(())
    }

    #[test]
    fn credential_token_path_requires_credentials_directory() {
        let error = resolve_token_file_path(Path::new("%d/default.token"), None, None);

        assert!(
            error.is_err_and(|error| {
                error.to_string().contains("requires CREDENTIALS_DIRECTORY")
            })
        );
    }

    #[test]
    fn home_token_path_expands_from_home_directory() -> Result<()> {
        let path = resolve_token_file_path(
            Path::new("~/.config/musicindex-live-publisher/tokens/default.token"),
            None,
            Some(Path::new("/home/tester")),
        )?;

        assert_eq!(
            path,
            Path::new("/home/tester/.config/musicindex-live-publisher/tokens/default.token")
        );
        Ok(())
    }

    #[test]
    fn home_token_path_requires_home_directory() {
        let error = resolve_token_file_path(Path::new("~/default.token"), None, None);

        assert!(error.is_err_and(|error| error.to_string().contains("requires HOME")));
    }
}
