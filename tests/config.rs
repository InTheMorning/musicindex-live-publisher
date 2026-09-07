use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use musicindex_live_publisher::{
    ConfigEditError, ConfigOverrides, DropEvent, DropEventKind, DropWatcher, TargetConfigEdit,
    add_target_to_config, list_config_targets, load_config, remove_target_from_config,
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

fn publisher_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_musicindex-live-publisher"))
}

#[derive(Debug)]
struct ProvisionStubServer {
    endpoint: String,
    received: mpsc::Receiver<String>,
}

impl ProvisionStubServer {
    fn start(status: u16, body: Value) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let endpoint = format!("http://{}", listener.local_addr()?);
        let (sender, received) = mpsc::channel();
        thread::spawn(move || {
            let Ok((stream, _peer)) = listener.accept() else {
                return;
            };
            let _ignored = handle_provision_connection(stream, status, &body.to_string())
                .and_then(|path| sender.send(path).map_err(Into::into));
        });

        Ok(Self { endpoint, received })
    }

    fn wait_for_request(&self) -> Result<String> {
        self.received
            .recv_timeout(Duration::from_secs(2))
            .map_err(Into::into)
    }
}

fn handle_provision_connection(stream: TcpStream, status: u16, body: &str) -> Result<String> {
    let mut reader = BufReader::new(stream);
    let mut first_line = String::new();
    reader.read_line(&mut first_line)?;
    let path = first_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow!("request line missing path"))?
        .to_owned();

    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
    }

    let reason = if status == 200 { "OK" } else { "Error" };
    let raw = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    reader.get_mut().write_all(raw.as_bytes())?;
    Ok(path)
}

fn config_text_without_targets(watch_dir: &Path) -> String {
    format!(
        r#"
# operator comment
watch_dir = "{}"
endpoint = "https://api.example.test"
"#,
        watch_dir.display()
    )
}

fn target_edit(name: &str, event_id: &str, token_file: &Path) -> TargetConfigEdit {
    TargetConfigEdit {
        name: name.to_owned(),
        event_id: event_id.to_owned(),
        token_file: token_file.to_path_buf(),
        stream_delay_secs: None,
    }
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

fn provision_response_body() -> Value {
    json!({
        "event_id": "created-event",
        "broadcaster_token": "created-secret",
        "metadata_url": "/v1/liveitems/created-event/metadata",
        "remote_value_url": "/v1/liveitems/created-event/remoteValue",
        "events_url": "/v1/liveitems/created-event/events",
        "socket_io_url": "/event?event_id=created-event"
    })
}

#[test]
fn provision_json_command_outputs_shape_without_token_content() -> Result<()> {
    let server = ProvisionStubServer::start(200, provision_response_body())?;
    let temp = TempDir::new()?;
    let token_file = temp.path().join("default.token");

    let output = publisher_command()
        .arg("provision")
        .arg("--endpoint")
        .arg(&server.endpoint)
        .arg("--target")
        .arg("default")
        .arg("--token-file")
        .arg(&token_file)
        .arg("--json")
        .output()?;

    assert!(
        output.status.success(),
        "provision --json should exit successfully"
    );
    assert_eq!(
        server.wait_for_request()?,
        "/v1/liveitems",
        "provision should call the live item create route"
    );
    let stdout = String::from_utf8(output.stdout)?;
    let value: Value = serde_json::from_str(&stdout)?;

    assert_eq!(value["event_id"], "created-event");
    assert_eq!(value["token_file"], token_file.display().to_string());
    assert_eq!(value["target"], "default");
    assert_eq!(
        value["metadata_url"],
        "/v1/liveitems/created-event/metadata"
    );
    assert_eq!(
        value["remote_value_url"],
        "/v1/liveitems/created-event/remoteValue"
    );
    assert_eq!(value["events_url"], "/v1/liveitems/created-event/events");
    assert_eq!(value["socket_io_url"], "/event?event_id=created-event");
    assert!(value.get("broadcaster_token").is_none());
    assert!(!stdout.contains("created-secret"));
    assert_eq!(fs::read_to_string(token_file)?, "created-secret\n");
    Ok(())
}

#[test]
fn provision_without_json_keeps_prose_output() -> Result<()> {
    let server = ProvisionStubServer::start(200, provision_response_body())?;
    let temp = TempDir::new()?;
    let token_file = temp.path().join("default.token");

    let output = publisher_command()
        .arg("provision")
        .arg("--endpoint")
        .arg(&server.endpoint)
        .arg("--target")
        .arg("default")
        .arg("--token-file")
        .arg(&token_file)
        .output()?;

    assert!(
        output.status.success(),
        "provision without --json should exit successfully"
    );
    let stdout = String::from_utf8(output.stdout)?;
    let expected = format!(
        "Live item provisioned.\n\
The broadcaster token was returned exactly once and cannot be recovered by the relay.\n\
Token written to {}\n\n\
[[target]]\n\
name = \"default\"\n\
event_id = \"created-event\"\n\
token_file = \"{}\"\n",
        token_file.display(),
        token_file.display()
    );

    assert_eq!(stdout, expected);
    assert!(!stdout.contains("created-secret"));
    Ok(())
}

#[test]
fn provision_json_failure_outputs_error_object() -> Result<()> {
    let server = ProvisionStubServer::start(500, json!({"error": "boom"}))?;
    let temp = TempDir::new()?;
    let token_file = temp.path().join("default.token");

    let output = publisher_command()
        .arg("provision")
        .arg("--endpoint")
        .arg(&server.endpoint)
        .arg("--token-file")
        .arg(&token_file)
        .arg("--json")
        .output()?;

    assert_eq!(
        output.status.code(),
        Some(1),
        "provision --json should keep the failure exit code"
    );
    let stdout = String::from_utf8(output.stdout)?;
    let value: Value = serde_json::from_str(&stdout)?;

    assert!(
        value["error"]
            .as_str()
            .is_some_and(|error| { error.contains("POST /v1/liveitems failed with HTTP 500") })
    );
    assert!(!stdout.contains("created-secret"));
    Ok(())
}

#[test]
fn config_show_json_outputs_redacted_shape() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let token = write_token(temp.path(), "default.token", "secret-token")?;
    let config_path = write_config(temp.path(), &config_text(&watch_dir, &token, None))?;

    let output = publisher_command()
        .arg("config")
        .arg("show")
        .arg("--config")
        .arg(&config_path)
        .arg("--json")
        .output()?;

    assert!(
        output.status.success(),
        "config show --json should exit successfully"
    );
    let stdout = String::from_utf8(output.stdout)?;
    let value: Value = serde_json::from_str(&stdout)?;

    assert_eq!(value["watch_dir"], watch_dir.display().to_string());
    assert_eq!(value["endpoint"], "https://api.example.test");
    assert_eq!(value["targets"][0]["name"], "default");
    assert_eq!(value["targets"][0]["event_id"], "event-default");
    assert_eq!(
        value["targets"][0]["token_file"],
        token.display().to_string()
    );
    assert_eq!(value["targets"][0]["stream_delay_secs"], 0.0);
    assert_eq!(value["targets"][0]["fallback_configured"], true);
    assert!(value["targets"][0].get("fallback").is_none());
    assert!(!stdout.contains("secret-token"));
    assert!(!stdout.contains("03station"));
    Ok(())
}

#[test]
fn config_show_json_failure_outputs_error_object() -> Result<()> {
    let temp = TempDir::new()?;
    let missing = temp.path().join("missing.toml");

    let output = publisher_command()
        .arg("config")
        .arg("show")
        .arg("--config")
        .arg(&missing)
        .arg("--json")
        .output()?;

    assert_eq!(
        output.status.code(),
        Some(1),
        "config show --json should keep the failure exit code"
    );
    let stdout = String::from_utf8(output.stdout)?;
    let value: Value = serde_json::from_str(&stdout)?;

    assert!(value["error"].as_str().is_some_and(|error| {
        error.contains("read config file") && error.contains("missing.toml")
    }));
    Ok(())
}

#[test]
fn version_command_prints_package_version() -> Result<()> {
    let output = publisher_command().arg("--version").output()?;

    assert!(
        output.status.success(),
        "--version should exit successfully"
    );
    assert_eq!(
        String::from_utf8(output.stdout)?,
        format!("{}\n", env!("CARGO_PKG_VERSION"))
    );
    Ok(())
}

#[test]
fn target_add_appends_to_config_without_targets() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let token = write_token(temp.path(), "default.token", "secret-token")?;
    let config_path = write_config(temp.path(), &config_text_without_targets(&watch_dir))?;

    add_target_to_config(
        &config_path,
        &target_edit("default", "event-default", &token),
        false,
    )?;

    let text = fs::read_to_string(&config_path)?;
    let config = load_config(&config_path, ConfigOverrides::default())?;

    assert!(text.contains("# operator comment"));
    assert_eq!(config.targets.len(), 1);
    assert_eq!(config.targets[0].name, "default");
    assert_eq!(config.targets[0].event_id, "event-default");
    Ok(())
}

#[test]
fn target_add_appends_second_target_without_rewriting_existing_config() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let default_token = write_token(temp.path(), "default.token", "default-secret")?;
    let aux_token = write_token(temp.path(), "aux.token", "aux-secret")?;
    let config_path = write_config(temp.path(), &config_text(&watch_dir, &default_token, None))?;

    add_target_to_config(
        &config_path,
        &target_edit("aux", "event-aux", &aux_token),
        false,
    )?;

    let text = fs::read_to_string(&config_path)?;
    let config = load_config(&config_path, ConfigOverrides::default())?;

    assert_eq!(config.targets.len(), 2);
    assert!(text.contains("[target.fallback]"));
    assert!(text.contains("title = \"Default Station\""));
    assert!(text.contains("name = \"aux\""));
    Ok(())
}

#[test]
fn target_add_duplicate_without_replace_is_distinct_error() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let token = write_token(temp.path(), "default.token", "secret-token")?;
    let config_path = write_config(temp.path(), &config_text(&watch_dir, &token, None))?;

    let error = add_target_to_config(
        &config_path,
        &target_edit("default", "event-new", &token),
        false,
    )
    .err()
    .ok_or_else(|| anyhow!("expected duplicate target error"))?;

    assert!(matches!(
        error.downcast_ref::<ConfigEditError>(),
        Some(ConfigEditError::TargetExists(name)) if name == "default"
    ));
    Ok(())
}

#[test]
fn target_add_duplicate_with_replace_replaces_stanza() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let old_token = write_token(temp.path(), "default.token", "old-secret")?;
    let new_token = write_token(temp.path(), "new.token", "new-secret")?;
    let config_path = write_config(temp.path(), &config_text(&watch_dir, &old_token, None))?;
    let mut edit = target_edit("default", "event-new", &new_token);
    edit.stream_delay_secs = Some(12.5);

    add_target_to_config(&config_path, &edit, true)?;

    let config = load_config(&config_path, ConfigOverrides::default())?;

    assert_eq!(config.targets.len(), 1);
    assert_eq!(config.targets[0].event_id, "event-new");
    assert_eq!(config.targets[0].token_file, new_token);
    assert_eq!(
        config.targets[0].stream_delay,
        Duration::from_millis(12_500)
    );
    Ok(())
}

#[test]
fn target_remove_deletes_one_stanza() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let default_token = write_token(temp.path(), "default.token", "default-secret")?;
    let aux_token = write_token(temp.path(), "aux.token", "aux-secret")?;
    let config_path = write_config(
        temp.path(),
        &config_text(&watch_dir, &default_token, Some(&aux_token)),
    )?;

    remove_target_from_config(&config_path, "aux")?;

    let config = load_config(&config_path, ConfigOverrides::default())?;

    assert_eq!(config.targets.len(), 1);
    assert_eq!(config.targets[0].name, "default");
    Ok(())
}

#[test]
fn target_remove_missing_target_is_distinct_error() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let token = write_token(temp.path(), "default.token", "secret-token")?;
    let config_path = write_config(temp.path(), &config_text(&watch_dir, &token, None))?;

    let error = remove_target_from_config(&config_path, "missing")
        .err()
        .ok_or_else(|| anyhow!("expected missing target error"))?;

    assert!(matches!(
        error.downcast_ref::<ConfigEditError>(),
        Some(ConfigEditError::TargetNotFound(name)) if name == "missing"
    ));
    Ok(())
}

#[test]
fn target_commands_preserve_comments() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let default_token = write_token(temp.path(), "default.token", "default-secret")?;
    let aux_token = write_token(temp.path(), "aux.token", "aux-secret")?;
    let text = format!(
        r#"
# top-level comment
watch_dir = "{}"
endpoint = "https://api.example.test"

# default target comment
[[target]]
name = "default"
event_id = "event-default"
token_file = "{}"

# trailing comment
"#,
        watch_dir.display(),
        default_token.display()
    );
    let config_path = write_config(temp.path(), &text)?;

    add_target_to_config(
        &config_path,
        &target_edit("aux", "event-aux", &aux_token),
        false,
    )?;
    remove_target_from_config(&config_path, "aux")?;

    let edited = fs::read_to_string(&config_path)?;

    assert!(edited.contains("# top-level comment"));
    assert!(edited.contains("# default target comment"));
    assert!(edited.contains("# trailing comment"));
    Ok(())
}

#[test]
fn target_list_reads_redacted_summaries_without_token_content() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let token = write_token(temp.path(), "default.token", "secret-token")?;
    let config_path = write_config(temp.path(), &config_text(&watch_dir, &token, None))?;

    let targets = list_config_targets(&config_path)?;

    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].name, "default");
    assert_eq!(targets[0].event_id, "event-default");
    assert_eq!(targets[0].token_file, token);
    assert_eq!(targets[0].stream_delay_secs, 0.0);
    assert!(!format!("{targets:?}").contains("secret-token"));
    Ok(())
}

#[test]
fn target_add_rejects_event_id_control_characters() -> Result<()> {
    let temp = TempDir::new()?;
    let token = write_token(temp.path(), "default.token", "secret-token")?;

    let error = add_target_to_config(
        &write_config(temp.path(), "")?,
        &target_edit("default", "event\nid", &token),
        false,
    )
    .err()
    .ok_or_else(|| anyhow!("expected control character error"))?;

    assert!(
        error
            .to_string()
            .contains("target event_id must not contain control characters")
    );
    Ok(())
}

#[test]
fn target_add_rejects_unreadable_token_file() -> Result<()> {
    let temp = TempDir::new()?;
    let missing = temp.path().join("missing.token");

    let error = add_target_to_config(
        &write_config(temp.path(), "")?,
        &target_edit("default", "event-default", &missing),
        false,
    )
    .err()
    .ok_or_else(|| anyhow!("expected missing token file error"))?;

    assert!(format!("{error:#}").contains("inspect token file"));
    Ok(())
}

#[test]
fn target_list_json_command_outputs_no_token_content() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let token = write_token(temp.path(), "default.token", "secret-token")?;
    let config_path = write_config(temp.path(), &config_text(&watch_dir, &token, None))?;

    let output = publisher_command()
        .args([
            "target",
            "list",
            "--config",
            &config_path.display().to_string(),
            "--json",
        ])
        .output()?;

    assert!(
        output.status.success(),
        "target list --json should exit successfully"
    );
    let stdout = String::from_utf8(output.stdout)?;
    let value: Value = serde_json::from_str(&stdout)?;

    assert_eq!(value["targets"][0]["name"], "default");
    assert_eq!(value["targets"][0]["event_id"], "event-default");
    assert!(!stdout.contains("secret-token"));
    Ok(())
}

#[test]
fn target_add_duplicate_exits_with_distinct_code() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let token = write_token(temp.path(), "default.token", "secret-token")?;
    let config_path = write_config(temp.path(), &config_text(&watch_dir, &token, None))?;

    let output = publisher_command()
        .args([
            "target",
            "add",
            "--config",
            &config_path.display().to_string(),
            "--name",
            "default",
            "--event-id",
            "event-new",
            "--token-file",
            &token.display().to_string(),
        ])
        .output()?;

    assert_eq!(
        output.status.code(),
        Some(2),
        "duplicate target should use the target-exists exit code"
    );
    Ok(())
}

#[test]
fn target_remove_missing_exits_with_distinct_code() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let token = write_token(temp.path(), "default.token", "secret-token")?;
    let config_path = write_config(temp.path(), &config_text(&watch_dir, &token, None))?;

    let output = publisher_command()
        .args([
            "target",
            "remove",
            "--config",
            &config_path.display().to_string(),
            "--name",
            "missing",
        ])
        .output()?;

    assert_eq!(
        output.status.code(),
        Some(3),
        "missing target should use the target-not-found exit code"
    );
    Ok(())
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
        config.targets[0].fallback.value.destinations[0]
            .split
            .as_deref(),
        Some("100")
    );
    assert_eq!(config.targets[0].fallback.value.model.kind, "lightning");
    assert_eq!(config.targets[0].fallback.value.model.method, "keysend");
    assert_eq!(
        config.targets[0].fallback.value.destinations[0]
            .kind
            .as_deref(),
        Some("node")
    );
    Ok(())
}

#[test]
fn config_loads_nested_fallback_value_block() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let token = write_token(temp.path(), "default.token", "secret-token")?;
    let config_path = write_config(
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
  title = "Default Station"

    [target.fallback.value.model]
    type = "lightning"
    method = "keysend"
    suggested = "0.0000100000"

    [[target.fallback.value.destinations]]
    name = "Sharpie"
    type = "node"
    address = "03a524eb4f2e9f07b4cbb904f265ed21af7e46cebd8dc92eb9682dfd0d54f6349f"
    split = "10"
    customKey = "696969"
    customValue = "5"
    fee = false
"#,
            watch_dir.display(),
            token.display()
        ),
    )?;

    let config = load_config(&config_path, ConfigOverrides::default())?;
    let model = &config.targets[0].fallback.value.model;
    let destination = &config.targets[0].fallback.value.destinations[0];

    assert_eq!(config.targets[0].fallback.title, "Default Station");
    assert_eq!(model.kind, "lightning");
    assert_eq!(model.method, "keysend");
    assert_eq!(model.suggested.as_deref(), Some("0.0000100000"));
    assert_eq!(destination.kind.as_deref(), Some("node"));
    assert_eq!(
        destination.address.as_deref(),
        Some("03a524eb4f2e9f07b4cbb904f265ed21af7e46cebd8dc92eb9682dfd0d54f6349f")
    );
    assert_eq!(destination.split.as_deref(), Some("10"));
    assert_eq!(destination.custom_key.as_deref(), Some("696969"));
    assert_eq!(destination.custom_value.as_deref(), Some("5"));
    assert_eq!(destination.fee, Some(false));
    Ok(())
}

#[test]
fn config_accepts_configured_fallback_value_model() -> Result<()> {
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

    [target.fallback.value.model]
    type = "legacy-node"
    method = "custom-method"

    [[target.fallback.value.destinations]]
    name = "Station"
    type = "node"
    address = "03station"
    split = "100"
"#,
            temp.path().join("watch").display(),
            token.display()
        ),
    )?;

    let config = load_config(&path, ConfigOverrides::default())?;

    assert_eq!(config.targets[0].fallback.value.model.kind, "legacy-node");
    assert_eq!(
        config.targets[0].fallback.value.model.method,
        "custom-method"
    );
    Ok(())
}

#[test]
fn config_accepts_lnaddress_fallback_destination() -> Result<()> {
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

    [target.fallback.value.model]
    type = "lightning"
    method = "lnaddress"

    [[target.fallback.value.destinations]]
    name = "Station"
    type = "lnaddress"
    address = "station@example.com"
    split = "100"
"#,
            temp.path().join("watch").display(),
            token.display()
        ),
    )?;

    let config = load_config(&path, ConfigOverrides::default())?;
    let destination = &config.targets[0].fallback.value.destinations[0];

    assert_eq!(config.targets[0].fallback.value.model.method, "lnaddress");
    assert_eq!(destination.kind.as_deref(), Some("lnaddress"));
    assert_eq!(destination.address.as_deref(), Some("station@example.com"));
    Ok(())
}

#[test]
fn config_rejects_empty_fallback_value_model_fields() -> Result<()> {
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

    [target.fallback.value.model]
    type = ""
    method = "keysend"

    [[target.fallback.value.destinations]]
    name = "Station"
    type = "node"
    address = "03station"
    split = "100"
"#,
            temp.path().join("watch").display(),
            token.display()
        ),
    )?;

    let error = load_config(&path, ConfigOverrides::default()).err();

    assert!(error.is_some_and(|error| {
        error
            .to_string()
            .contains("fallback value model type must not be empty")
    }));
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
fn config_target_without_fallback_uses_dead_fallback_route() -> Result<()> {
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

    let config = load_config(&path, ConfigOverrides::default())?;
    let fallback = &config.targets[0].fallback;
    let destination = &fallback.value.destinations[0];

    assert_eq!(fallback.title, "No V4V track playing");
    assert_eq!(fallback.value.model.kind, "lightning");
    assert_eq!(fallback.value.model.method, "lnaddress");
    assert_eq!(destination.kind.as_deref(), Some("lnaddress"));
    assert_eq!(destination.name.as_deref(), Some("No V4V payment route"));
    assert_eq!(
        destination.address.as_deref(),
        Some("no-v4v-track@example.invalid")
    );
    assert_eq!(destination.split.as_deref(), Some("100"));
    Ok(())
}

#[test]
fn config_empty_fallback_destinations_uses_dead_fallback_route() -> Result<()> {
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

    let config = load_config(&path, ConfigOverrides::default())?;
    let fallback = &config.targets[0].fallback;
    let destination = &fallback.value.destinations[0];

    assert_eq!(fallback.title, "Station");
    assert_eq!(fallback.value.model.method, "lnaddress");
    assert_eq!(destination.kind.as_deref(), Some("lnaddress"));
    assert_eq!(
        destination.address.as_deref(),
        Some("no-v4v-track@example.invalid")
    );
    Ok(())
}

#[test]
fn config_rejects_placeholder_fallback_destination_address() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let token = write_token(temp.path(), "default.token", "secret-token")?;
    let text = config_text(&watch_dir, &token, None).replace(
        "address = \"03station\"",
        "address = \"YOUR_LIGHTNING_NODE_PUBKEY\"",
    );
    let config_path = write_config(temp.path(), &text)?;

    let error = load_config(&config_path, ConfigOverrides::default()).err();

    assert!(error.is_some_and(|error| {
        error
            .to_string()
            .contains("fallback destination 0 address is still an example placeholder")
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

/// Inserts a `stream_delay_secs` line into the default target block.
fn config_text_with_delay(watch_dir: &Path, token: &Path, literal: &str) -> String {
    config_text(watch_dir, token, None).replace(
        "event_id = \"event-default\"",
        &format!("event_id = \"event-default\"\nstream_delay_secs = {literal}"),
    )
}

fn delay_error(literal: &str) -> Result<String> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let token = write_token(temp.path(), "default.token", "secret-token")?;
    let config_path = write_config(
        temp.path(),
        &config_text_with_delay(&watch_dir, &token, literal),
    )?;

    let error = load_config(&config_path, ConfigOverrides::default())
        .err()
        .ok_or_else(|| anyhow!("expected stream_delay_secs {literal} to be rejected"))?;
    Ok(format!("{error:#}"))
}

#[test]
fn config_stream_delay_defaults_to_zero() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let token = write_token(temp.path(), "default.token", "secret-token")?;
    let config_path = write_config(temp.path(), &config_text(&watch_dir, &token, None))?;

    let config = load_config(&config_path, ConfigOverrides::default())?;

    assert_eq!(config.targets[0].stream_delay, Duration::ZERO);
    Ok(())
}

#[test]
fn config_stream_delay_resolves_fractional_seconds() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let token = write_token(temp.path(), "default.token", "secret-token")?;
    let config_path = write_config(
        temp.path(),
        &config_text_with_delay(&watch_dir, &token, "12.5"),
    )?;

    let config = load_config(&config_path, ConfigOverrides::default())?;

    assert_eq!(
        config.targets[0].stream_delay,
        Duration::from_millis(12_500)
    );
    Ok(())
}

#[test]
fn config_negative_stream_delay_is_error_naming_target() -> Result<()> {
    assert!(
        delay_error("-1.0")?.contains("target default stream_delay_secs must not be negative"),
        "error should name the target and the field"
    );
    Ok(())
}

#[test]
fn config_non_finite_stream_delay_is_error() -> Result<()> {
    assert!(
        delay_error("nan")?.contains("target default stream_delay_secs must be a finite number")
    );
    assert!(
        delay_error("inf")?.contains("target default stream_delay_secs must be a finite number")
    );
    Ok(())
}

#[test]
fn config_stream_delay_above_the_ceiling_is_error() -> Result<()> {
    // A millisecond value typed into a seconds field is the mistake this
    // catches: it would park every payload well past the end of the show.
    assert!(delay_error("12000")?.contains("exceeds the 300 second maximum"));
    Ok(())
}
