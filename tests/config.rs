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
# default target comment
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
        "schema": "musicindex.nowplaying/2",
        "target": target,
        "artist": "Alice",
        "title": title,
        "duration_secs": 187.326,
        "image": null,
        "feed_guid": null,
        "track_guid": null,
        "album": null,
        "play_id": null,
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
    let mut target_keys: Vec<&str> = value["targets"][0]
        .as_object()
        .ok_or_else(|| anyhow!("target should be an object"))?
        .keys()
        .map(String::as_str)
        .collect();
    target_keys.sort_unstable();
    assert_eq!(
        target_keys,
        vec![
            "display_dir",
            "event_id",
            "name",
            "stream_delay_secs",
            "token_file"
        ]
    );
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
    assert!(text.contains("# default target comment"));
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

/// A config with a top-level comment and three targets. The middle target
/// `mixxx` has a display directory, comments, a blank line and a trailing
/// comment on `event_id`. The text uses no `stream_delay_secs` on `mixxx`
/// when `delay` is `None`.
fn replace_fixture(
    watch_dir: &Path,
    old_token: &Path,
    display_dir: &Path,
    delay: Option<&str>,
) -> String {
    let delay_line = delay
        .map(|literal| format!("stream_delay_secs = {literal}\n"))
        .unwrap_or_default();
    format!(
        r#"# top-level comment
watch_dir = "{watch}"
endpoint = "https://api.example.test"

[[target]]
name = "first"
event_id = "event-first"
token_file = "{old}"

# mixxx target comment
[[target]]
name = "mixxx"
  event_id   =   "event-old"   # reserved event, keep this comment
# comment between keys

token_file = "{old}"
{delay_line}display_dir = "{display}"
# last stanza comment

[[target]]
name = "last"
event_id = "event-last"
token_file = "{old}"
stream_delay_secs = 4.5
# trailing comment
"#,
        watch = watch_dir.display(),
        old = old_token.display(),
        display = display_dir.display(),
    )
}

#[test]
fn target_add_replace_keeps_display_dir_and_comments() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let display_dir = temp.path().join("display");
    let old_token = write_token(temp.path(), "old.token", "old-secret")?;
    let new_token = write_token(temp.path(), "new.token", "new-secret")?;
    let original = replace_fixture(&watch_dir, &old_token, &display_dir, None);
    let config_path = write_config(temp.path(), &original)?;

    add_target_to_config(
        &config_path,
        &target_edit("mixxx", "event-new", &new_token),
        true,
    )?;

    let edited = fs::read_to_string(&config_path)?;
    let expected = original
        .replace(
            "  event_id   =   \"event-old\"   # reserved event",
            "  event_id   =   \"event-new\"   # reserved event",
        )
        .replace(
            &format!(
                "# comment between keys\n\ntoken_file = \"{}\"",
                old_token.display()
            ),
            &format!(
                "# comment between keys\n\ntoken_file = \"{}\"",
                new_token.display()
            ),
        );
    assert_eq!(edited, expected);

    let config = load_config(&config_path, ConfigOverrides::default())?;
    let mixxx = &config.targets[1];
    assert_eq!(mixxx.event_id, "event-new");
    assert_eq!(mixxx.token_file, new_token);
    assert_eq!(mixxx.display_dir.as_deref(), Some(display_dir.as_path()));
    assert_eq!(mixxx.stream_delay, Duration::ZERO);
    Ok(())
}

#[test]
fn target_add_replace_without_delay_flag_keeps_the_present_delay() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let display_dir = temp.path().join("display");
    let old_token = write_token(temp.path(), "old.token", "old-secret")?;
    let new_token = write_token(temp.path(), "new.token", "new-secret")?;
    let original = replace_fixture(&watch_dir, &old_token, &display_dir, Some("12.5"));
    let config_path = write_config(temp.path(), &original)?;

    add_target_to_config(
        &config_path,
        &target_edit("mixxx", "event-new", &new_token),
        true,
    )?;

    let edited = fs::read_to_string(&config_path)?;
    assert!(edited.contains(&format!(
        "token_file = \"{}\"\nstream_delay_secs = 12.5\ndisplay_dir",
        new_token.display()
    )));
    let config = load_config(&config_path, ConfigOverrides::default())?;
    assert_eq!(
        config.targets[1].stream_delay,
        Duration::from_millis(12_500)
    );
    Ok(())
}

#[test]
fn target_add_replace_with_delay_flag_changes_the_present_delay() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let display_dir = temp.path().join("display");
    let old_token = write_token(temp.path(), "old.token", "old-secret")?;
    let original = replace_fixture(&watch_dir, &old_token, &display_dir, Some("12.5"));
    let config_path = write_config(temp.path(), &original)?;
    let mut edit = target_edit("mixxx", "event-old", &old_token);
    edit.stream_delay_secs = Some(30.0);

    add_target_to_config(&config_path, &edit, true)?;

    let edited = fs::read_to_string(&config_path)?;
    let expected = original.replace("stream_delay_secs = 12.5\n", "stream_delay_secs = 30\n");
    assert_eq!(edited, expected);
    let config = load_config(&config_path, ConfigOverrides::default())?;
    assert_eq!(config.targets[1].stream_delay, Duration::from_secs(30));
    assert_eq!(
        config.targets[1].display_dir.as_deref(),
        Some(display_dir.as_path())
    );
    Ok(())
}

#[test]
fn target_add_replace_with_delay_flag_adds_a_missing_delay_after_token_file() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let display_dir = temp.path().join("display");
    let old_token = write_token(temp.path(), "old.token", "old-secret")?;
    let original = replace_fixture(&watch_dir, &old_token, &display_dir, None);
    let config_path = write_config(temp.path(), &original)?;
    let mut edit = target_edit("mixxx", "event-old", &old_token);
    edit.stream_delay_secs = Some(7.25);

    add_target_to_config(&config_path, &edit, true)?;

    let edited = fs::read_to_string(&config_path)?;
    let expected = replace_fixture(&watch_dir, &old_token, &display_dir, Some("7.25"));
    assert_eq!(edited, expected);
    Ok(())
}

#[test]
fn target_add_replace_keeps_other_targets_and_top_level_keys_byte_for_byte() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let display_dir = temp.path().join("display");
    let old_token = write_token(temp.path(), "old.token", "old-secret")?;
    let new_token = write_token(temp.path(), "new.token", "new-secret")?;
    let original = replace_fixture(&watch_dir, &old_token, &display_dir, Some("12.5"));
    let config_path = write_config(temp.path(), &original)?;
    let mut edit = target_edit("mixxx", "event-new", &new_token);
    edit.stream_delay_secs = Some(1.5);

    add_target_to_config(&config_path, &edit, true)?;

    let edited = fs::read_to_string(&config_path)?;
    let head_end = original
        .find("  event_id   =")
        .ok_or_else(|| anyhow!("fixture should hold the mixxx event_id line"))?;
    let tail_start = original
        .find("display_dir = ")
        .ok_or_else(|| anyhow!("fixture should hold the display_dir line"))?;
    let tail = &original[tail_start..];
    assert!(edited.starts_with(&original[..head_end]));
    assert!(edited.ends_with(tail));
    assert!(tail.contains("name = \"last\""));
    assert!(original[..head_end].contains("name = \"first\""));
    Ok(())
}

#[test]
fn target_add_replace_refuses_a_multi_line_value_and_keeps_the_file() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let old_token = write_token(temp.path(), "old.token", "old-secret")?;
    let new_token = write_token(temp.path(), "new.token", "new-secret")?;
    let original = format!(
        "watch_dir = \"{}\"\nendpoint = \"https://api.example.test\"\n\n[[target]]\nname = \"mixxx\"\nevent_id = \"\"\"\nevent-old\"\"\"\ntoken_file = \"{}\"\n",
        watch_dir.display(),
        old_token.display()
    );
    let config_path = write_config(temp.path(), &original)?;

    let error = add_target_to_config(
        &config_path,
        &target_edit("mixxx", "event-new", &new_token),
        true,
    )
    .err()
    .ok_or_else(|| anyhow!("expected a multi-line value error"))?;

    assert!(format!("{error:#}").contains("ADR 0004"));
    assert_eq!(fs::read_to_string(&config_path)?, original);
    Ok(())
}

#[test]
fn target_add_replace_command_keeps_display_dir() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let display_dir = temp.path().join("display");
    let old_token = write_token(temp.path(), "old.token", "old-secret")?;
    let new_token = write_token(temp.path(), "new.token", "new-secret")?;
    let original = replace_fixture(&watch_dir, &old_token, &display_dir, None);
    let config_path = write_config(temp.path(), &original)?;

    let output = publisher_command()
        .args([
            "target",
            "add",
            "--config",
            &config_path.display().to_string(),
            "--name",
            "mixxx",
            "--event-id",
            "event-new",
            "--token-file",
            &new_token.display().to_string(),
            "--replace",
        ])
        .output()?;

    assert!(
        output.status.success(),
        "target add --replace should succeed"
    );
    let config = load_config(&config_path, ConfigOverrides::default())?;
    assert_eq!(config.targets[1].event_id, "event-new");
    assert_eq!(
        config.targets[1].display_dir.as_deref(),
        Some(display_dir.as_path())
    );
    Ok(())
}

#[test]
fn config_show_and_target_list_json_give_display_dir_or_null() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let display_dir = temp.path().join("display");
    let token = write_token(temp.path(), "old.token", "secret-token")?;
    let config_path = write_config(
        temp.path(),
        &replace_fixture(&watch_dir, &token, &display_dir, None),
    )?;
    let config_arg = config_path.display().to_string();

    for args in [
        ["config", "show", "--config", config_arg.as_str(), "--json"],
        ["target", "list", "--config", config_arg.as_str(), "--json"],
    ] {
        let output = publisher_command().args(args).output()?;
        assert!(output.status.success(), "{args:?} should succeed");
        let stdout = String::from_utf8(output.stdout)?;
        let value: Value = serde_json::from_str(&stdout)?;

        assert_eq!(value["targets"][0]["name"], "first");
        assert_eq!(value["targets"][0].get("display_dir"), Some(&Value::Null));
        assert_eq!(value["targets"][1]["name"], "mixxx");
        assert_eq!(
            value["targets"][1]["display_dir"],
            display_dir.display().to_string()
        );
        assert!(!stdout.contains("secret-token"));
    }
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
fn config_target_fallback_table_is_load_error_naming_adr() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let token = write_token(temp.path(), "default.token", "secret-token")?;
    let text = format!(
        r#"
watch_dir = "{}"
endpoint = "https://api.example.test"

[[target]]
name = "default"
event_id = "event-default"
token_file = "{}"

  [target.fallback]
  title = "Station"
"#,
        watch_dir.display(),
        token.display()
    );
    let config_path = write_config(temp.path(), &text)?;

    let error = load_config(&config_path, ConfigOverrides::default()).err();

    assert!(error.is_some_and(|error| {
        let message = error.to_string();
        message.contains("ADR 0005")
            && message.contains("[target.fallback]")
            && message.contains("target default")
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

fn config_text_with_display_dir(watch_dir: &Path, token: &Path, display_dir: &Path) -> String {
    format!(
        "{}display_dir = \"{}\"\n",
        config_text(watch_dir, token, None),
        display_dir.display()
    )
}

#[test]
fn config_loads_an_optional_display_dir() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let display_dir = temp.path().join("display");
    let token = write_token(temp.path(), "default.token", "secret-token")?;
    let config_path = write_config(
        temp.path(),
        &config_text_with_display_dir(&watch_dir, &token, &display_dir),
    )?;

    let config = load_config(&config_path, ConfigOverrides::default())?;

    assert_eq!(
        config.targets[0].display_dir.as_deref(),
        Some(display_dir.as_path())
    );
    Ok(())
}

#[test]
fn config_without_display_dir_has_no_display_path() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let token = write_token(temp.path(), "default.token", "secret-token")?;
    let config_path = write_config(temp.path(), &config_text(&watch_dir, &token, None))?;

    let config = load_config(&config_path, ConfigOverrides::default())?;

    assert_eq!(config.targets[0].display_dir, None);
    Ok(())
}

#[test]
fn config_display_dir_equal_to_watch_dir_is_error_naming_target() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let token = write_token(temp.path(), "default.token", "secret-token")?;
    let config_path = write_config(
        temp.path(),
        &config_text_with_display_dir(&watch_dir, &token, &watch_dir),
    )?;

    let error = load_config(&config_path, ConfigOverrides::default())
        .expect_err("display_dir equal to watch_dir must be refused");

    let message = format!("{error:#}");
    assert!(message.contains("ADR 0008"), "{message}");
    assert!(message.contains("default"), "{message}");
    assert!(message.contains("display_dir"), "{message}");
    Ok(())
}

#[test]
fn config_display_dir_equal_to_the_watch_dir_override_is_error() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let display_dir = temp.path().join("display");
    let token = write_token(temp.path(), "default.token", "secret-token")?;
    let config_path = write_config(
        temp.path(),
        &config_text_with_display_dir(&watch_dir, &token, &display_dir),
    )?;

    let result = load_config(
        &config_path,
        ConfigOverrides {
            watch_dir: Some(display_dir),
            endpoint: None,
        },
    );

    assert!(result.is_err_and(|error| format!("{error:#}").contains("ADR 0008")));
    Ok(())
}

#[cfg(unix)]
#[test]
fn config_display_dir_equal_to_watch_dir_after_canonicalization_is_error() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    fs::create_dir(&watch_dir)?;
    let link = temp.path().join("link-to-watch");
    std::os::unix::fs::symlink(&watch_dir, &link)?;
    let token = write_token(temp.path(), "default.token", "secret-token")?;
    let config_path = write_config(
        temp.path(),
        &config_text_with_display_dir(&watch_dir, &token, &link),
    )?;

    let result = load_config(&config_path, ConfigOverrides::default());

    assert!(result.is_err_and(|error| format!("{error:#}").contains("ADR 0008")));
    Ok(())
}

#[test]
fn config_target_debug_with_display_dir_redacts_the_token() -> Result<()> {
    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("watch");
    let display_dir = temp.path().join("display");
    let token = write_token(temp.path(), "default.token", "secret-token")?;
    let config_path = write_config(
        temp.path(),
        &config_text_with_display_dir(&watch_dir, &token, &display_dir),
    )?;

    let config = load_config(&config_path, ConfigOverrides::default())?;
    let rendered = format!("{config:?}");

    assert!(rendered.contains("display_dir"));
    assert!(!rendered.contains("secret-token"));
    Ok(())
}
