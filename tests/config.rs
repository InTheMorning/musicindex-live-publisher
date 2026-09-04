use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use musicindex_live_publisher::{
    ConfigOverrides, DropEvent, DropEventKind, DropWatcher, load_config,
};
use serde_json::{Value, json};
use tempfile::TempDir;

fn write_token(dir: &Path, name: &str, token: &str) -> Result<std::path::PathBuf> {
    let path = dir.join(name);
    fs::write(&path, token)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(path)
}

fn config_text(watch_dir: &Path, default_token: &Path, second_token: Option<&Path>) -> String {
    let second = second_token
        .map(|path| {
            format!(
                r#"
[[target]]
name = "aux"
event_id = "event-aux"
token_file = "{}"

  [target.fallback]
  title = "Aux Station"
  destinations = [
    {{ name = "Aux", type = "node", address = "03aux", split = "100" }},
  ]
"#,
                path.display()
            )
        })
        .unwrap_or_default();

    format!(
        r#"
watch_dir = "{}"
endpoint = "https://api.example.test"

[[target]]
name = "default"
event_id = "event-default"
token_file = "{}"

  [target.fallback]
  title = "Default Station"
  destinations = [
    {{ name = "Station", type = "node", address = "03station", split = "100", customKey = "k", customValue = "v", fee = false }},
  ]
{second}
"#,
        watch_dir.display(),
        default_token.display()
    )
}

fn write_config(dir: &Path, text: &str) -> Result<std::path::PathBuf> {
    let path = dir.join("config.toml");
    fs::write(&path, text)?;
    Ok(path)
}

fn dropfile(target: &str, title: &str) -> String {
    json!({
        "schema": "musicindex.nowplaying/1",
        "target": target,
        "artist": "Alice",
        "title": title,
        "duration_secs": 187.326,
        "image": null,
        "feed_guid": null,
        "track_guid": null,
        "value_routes": [{
            "recipient_name": "Alice",
            "route_type": "node",
            "address": "03alice",
            "split": 90.0,
            "fee": false,
            "custom_key": null,
            "custom_value": null
        }],
        "value_routes_source": "musicindex-api"
    })
    .to_string()
}

fn one_payload(payloads: Vec<musicindex_live_publisher::LiveValuePayload>) -> Result<Value> {
    let payload = payloads
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("expected one payload"))?;
    Ok(serde_json::to_value(payload)?)
}

#[test]
fn config_loads_single_target_and_trims_token() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let token = write_token(temp.path(), "default.token", "secret-token\n\t")?;
    let config_path = write_config(temp.path(), &config_text(&watch_dir, &token, None))?;

    let config = load_config(&config_path, ConfigOverrides::default())?;

    assert_eq!(config.watch_dir, watch_dir);
    assert_eq!(config.endpoint, "https://api.example.test");
    assert_eq!(config.targets.len(), 1);
    assert_eq!(config.targets[0].name, "default");
    assert_eq!(config.targets[0].token, "secret-token");
    assert_eq!(
        config.targets[0].fallback.destinations[0].split.as_deref(),
        Some("100")
    );
    Ok(())
}

#[test]
fn config_cli_overrides_watch_dir_and_endpoint() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let override_watch_dir = temp.path().join("override");
    let token = write_token(temp.path(), "default.token", "secret-token")?;
    let config_path = write_config(temp.path(), &config_text(&watch_dir, &token, None))?;

    let config = load_config(
        &config_path,
        ConfigOverrides {
            watch_dir: Some(override_watch_dir.clone()),
            endpoint: Some("https://override.example.test".to_owned()),
        },
    )?;

    assert_eq!(config.watch_dir, override_watch_dir);
    assert_eq!(config.endpoint, "https://override.example.test");
    Ok(())
}

#[test]
fn config_loads_two_targets_and_routes_dropfile_to_matching_target() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let default_token = write_token(temp.path(), "default.token", "default-secret")?;
    let aux_token = write_token(temp.path(), "aux.token", "aux-secret")?;
    let config_path = write_config(
        temp.path(),
        &config_text(&watch_dir, &default_token, Some(&aux_token)),
    )?;
    fs::create_dir(&watch_dir)?;
    let drop_path = watch_dir.join("aux.json");
    fs::write(&drop_path, dropfile("aux", "Aux Track"))?;

    let config = load_config(&config_path, ConfigOverrides::default())?;
    let targets = config
        .targets
        .iter()
        .map(|target| target.watch_target())
        .collect();
    let mut watcher = DropWatcher::new_targets(targets, Duration::from_millis(75));

    let payload = one_payload(watcher.process_event(
        DropEvent {
            kind: DropEventKind::Upsert,
            path: drop_path,
        },
        Instant::now(),
    )?)?;

    assert_eq!(payload["title"], "Aux Track");
    assert_eq!(payload["eventGuid"], "event-aux");
    Ok(())
}

#[test]
fn config_target_without_fallback_is_error() -> Result<()> {
    let temp = TempDir::new()?;
    let token = write_token(temp.path(), "default.token", "secret-token")?;
    let path = write_config(
        temp.path(),
        &format!(
            r#"
watch_dir = "{}"
endpoint = "https://api.example.test"

[[target]]
name = "default"
event_id = "event-default"
token_file = "{}"
"#,
            temp.path().join("watch").display(),
            token.display()
        ),
    )?;

    let error = load_config(&path, ConfigOverrides::default()).err();

    assert!(error.is_some_and(|error| {
        error
            .to_string()
            .contains("target default must configure fallback")
    }));
    Ok(())
}

#[test]
fn config_empty_fallback_destinations_is_error() -> Result<()> {
    let temp = TempDir::new()?;
    let token = write_token(temp.path(), "default.token", "secret-token")?;
    let path = write_config(
        temp.path(),
        &format!(
            r#"
watch_dir = "{}"
endpoint = "https://api.example.test"

[[target]]
name = "default"
event_id = "event-default"
token_file = "{}"

  [target.fallback]
  title = "Station"
  destinations = []
"#,
            temp.path().join("watch").display(),
            token.display()
        ),
    )?;

    let error = load_config(&path, ConfigOverrides::default()).err();

    assert!(error.is_some_and(|error| {
        error
            .to_string()
            .contains("target default fallback destinations must not be empty")
    }));
    Ok(())
}

#[test]
fn config_missing_token_file_is_error_naming_path() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let token = temp.path().join("missing.token");
    let config_path = write_config(temp.path(), &config_text(&watch_dir, &token, None))?;

    let error = load_config(&config_path, ConfigOverrides::default()).err();

    assert!(error.is_some_and(|error| error.to_string().contains(&token.display().to_string())));
    Ok(())
}

#[test]
fn config_empty_token_file_is_error() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let token = write_token(temp.path(), "default.token", "\n\t")?;
    let config_path = write_config(temp.path(), &config_text(&watch_dir, &token, None))?;

    let error = load_config(&config_path, ConfigOverrides::default()).err();

    assert!(error.is_some_and(|error| {
        error
            .to_string()
            .contains(&format!("token file {} is empty", token.display()))
    }));
    Ok(())
}

#[test]
fn config_placeholder_event_id_is_error() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let token = write_token(temp.path(), "default.token", "secret-token")?;
    let text = config_text(&watch_dir, &token, None).replace(
        "event_id = \"event-default\"",
        "event_id = \"replace-with-provisioned-event-guid\"",
    );
    let config_path = write_config(temp.path(), &text)?;

    let error = load_config(&config_path, ConfigOverrides::default()).err();

    assert!(error.is_some_and(|error| {
        error
            .to_string()
            .contains("target default event_id is still an example placeholder")
    }));
    Ok(())
}

#[cfg(unix)]
#[test]
fn config_permissive_token_file_still_loads() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let token = write_token(temp.path(), "default.token", "secret-token")?;
    fs::set_permissions(&token, fs::Permissions::from_mode(0o644))?;
    let config_path = write_config(temp.path(), &config_text(&watch_dir, &token, None))?;

    let config = load_config(&config_path, ConfigOverrides::default())?;

    assert_eq!(config.targets[0].token, "secret-token");
    Ok(())
}

#[test]
fn config_duplicate_target_names_are_error() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let default_token = write_token(temp.path(), "default.token", "default-secret")?;
    let aux_token = write_token(temp.path(), "aux.token", "aux-secret")?;
    let mut text = config_text(&watch_dir, &default_token, Some(&aux_token));
    text = text.replace("name = \"aux\"", "name = \"default\"");
    let config_path = write_config(temp.path(), &text)?;

    let error = load_config(&config_path, ConfigOverrides::default()).err();

    assert!(error.is_some_and(|error| error.to_string().contains("duplicate target name default")));
    Ok(())
}

#[test]
fn config_unknown_dropfile_target_is_skipped() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let token = write_token(temp.path(), "default.token", "secret-token")?;
    let config_path = write_config(temp.path(), &config_text(&watch_dir, &token, None))?;
    fs::create_dir(&watch_dir)?;
    let drop_path = watch_dir.join("unknown.json");
    fs::write(&drop_path, dropfile("unknown", "Unknown Track"))?;

    let config = load_config(&config_path, ConfigOverrides::default())?;
    let targets = config
        .targets
        .iter()
        .map(|target| target.watch_target())
        .collect();
    let mut watcher = DropWatcher::new_targets(targets, Duration::from_millis(75));

    let payloads = watcher.process_event(
        DropEvent {
            kind: DropEventKind::Upsert,
            path: drop_path,
        },
        Instant::now(),
    )?;

    assert!(payloads.is_empty());
    Ok(())
}
