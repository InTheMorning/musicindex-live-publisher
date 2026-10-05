//! `provision` never replaces a token file (reserved safety task 001).

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Output};
use std::sync::mpsc;
use std::thread;

use anyhow::{Result, anyhow};
use musicindex_live_publisher::write_token_file;
use serde_json::{Value, json};
use tempfile::TempDir;

/// A relay stub that records each request path before it answers.
struct RecordingRelay {
    endpoint: String,
    received: mpsc::Receiver<String>,
}

impl RecordingRelay {
    fn start() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let endpoint = format!("http://{}", listener.local_addr()?);
        let (sender, received) = mpsc::channel();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    return;
                };
                let _ignored = answer(stream, &sender);
            }
        });
        Ok(Self { endpoint, received })
    }

    fn requests(&self) -> Vec<String> {
        self.received.try_iter().collect()
    }
}

fn answer(stream: TcpStream, sender: &mpsc::Sender<String>) -> Result<()> {
    let mut reader = BufReader::new(stream);
    let mut first_line = String::new();
    reader.read_line(&mut first_line)?;
    let path = first_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow!("request line missing path"))?
        .to_owned();
    sender.send(path)?;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line.trim_end_matches(['\r', '\n']).is_empty() {
            break;
        }
    }
    let body = json!({
        "event_id": "created-event",
        "broadcaster_token": "created-secret",
        "metadata_url": "/v1/liveitems/created-event/metadata",
        "remote_value_url": "/v1/liveitems/created-event/remoteValue",
        "events_url": "/v1/liveitems/created-event/events",
        "socket_io_url": "/event?event_id=created-event"
    })
    .to_string();
    let raw = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    reader.get_mut().write_all(raw.as_bytes())?;
    Ok(())
}

fn provision(relay: &RecordingRelay, token_file: &Path, json: bool) -> Result<Output> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_musicindex-live-publisher"));
    command
        .arg("provision")
        .arg("--endpoint")
        .arg(&relay.endpoint)
        .arg("--token-file")
        .arg(token_file);
    if json {
        command.arg("--json");
    }
    Ok(command.output()?)
}

fn directory_names(directory: &Path) -> Result<Vec<String>> {
    let mut names = fs::read_dir(directory)?
        .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
        .collect::<Result<Vec<_>>>()?;
    names.sort();
    Ok(names)
}

fn assert_refused(output: &Output, token_file: &Path) -> Result<()> {
    assert!(!output.status.success(), "provision must fail");
    let stderr = String::from_utf8(output.stderr.clone())?;
    assert!(
        stderr.contains(&token_file.display().to_string()),
        "the error must name the path: {stderr}"
    );
    assert!(
        stderr.contains("sent no relay request"),
        "the error must say that no request was sent: {stderr}"
    );
    Ok(())
}

#[test]
fn provision_refuses_existing_token_file_without_relay_request() -> Result<()> {
    let relay = RecordingRelay::start()?;
    let temp = TempDir::new()?;
    let token_file = temp.path().join("default.token");
    fs::write(&token_file, "old-secret\n")?;

    let output = provision(&relay, &token_file, false)?;

    assert_refused(&output, &token_file)?;
    assert_eq!(fs::read_to_string(&token_file)?, "old-secret\n");
    assert_eq!(relay.requests(), Vec::<String>::new());
    assert_eq!(directory_names(temp.path())?, vec!["default.token"]);
    Ok(())
}

#[test]
fn provision_refuses_existing_directory_without_relay_request() -> Result<()> {
    let relay = RecordingRelay::start()?;
    let temp = TempDir::new()?;
    let token_file = temp.path().join("default.token");
    fs::create_dir(&token_file)?;

    let output = provision(&relay, &token_file, false)?;

    assert_refused(&output, &token_file)?;
    assert!(token_file.is_dir());
    assert_eq!(relay.requests(), Vec::<String>::new());
    Ok(())
}

#[cfg(unix)]
#[test]
fn provision_refuses_symbolic_link_and_keeps_its_target() -> Result<()> {
    let relay = RecordingRelay::start()?;
    let temp = TempDir::new()?;
    let target = temp.path().join("real.token");
    fs::write(&target, "old-secret\n")?;
    let token_file = temp.path().join("default.token");
    std::os::unix::fs::symlink(&target, &token_file)?;

    let output = provision(&relay, &token_file, false)?;

    assert_refused(&output, &token_file)?;
    assert_eq!(fs::read_link(&token_file)?, target);
    assert_eq!(fs::read_to_string(&target)?, "old-secret\n");
    assert_eq!(relay.requests(), Vec::<String>::new());
    Ok(())
}

#[cfg(unix)]
#[test]
fn provision_refuses_dangling_symbolic_link_without_relay_request() -> Result<()> {
    let relay = RecordingRelay::start()?;
    let temp = TempDir::new()?;
    let missing = temp.path().join("missing.token");
    let token_file = temp.path().join("default.token");
    std::os::unix::fs::symlink(&missing, &token_file)?;

    let output = provision(&relay, &token_file, false)?;

    assert_refused(&output, &token_file)?;
    assert_eq!(fs::read_link(&token_file)?, missing);
    assert!(!missing.exists());
    assert_eq!(relay.requests(), Vec::<String>::new());
    Ok(())
}

#[test]
fn provision_json_refuses_existing_token_file_with_error_object() -> Result<()> {
    let relay = RecordingRelay::start()?;
    let temp = TempDir::new()?;
    let token_file = temp.path().join("default.token");
    fs::write(&token_file, "old-secret\n")?;

    let output = provision(&relay, &token_file, true)?;

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout)?;
    let value: Value = serde_json::from_str(&stdout)?;
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("expected one JSON object"))?;
    assert_eq!(object.keys().collect::<Vec<_>>(), vec!["error"]);
    assert!(object["error"].as_str().is_some_and(|error| {
        error.contains(&token_file.display().to_string()) && error.contains("sent no relay request")
    }));
    assert_eq!(fs::read_to_string(&token_file)?, "old-secret\n");
    assert_eq!(relay.requests(), Vec::<String>::new());
    Ok(())
}

#[test]
fn provision_to_new_path_writes_private_token_and_leaves_no_temporary_file() -> Result<()> {
    let relay = RecordingRelay::start()?;
    let temp = TempDir::new()?;
    let token_file = temp.path().join("default.token");

    let output = provision(&relay, &token_file, true)?;

    assert!(output.status.success(), "provision should succeed");
    assert_eq!(relay.requests(), vec!["/v1/liveitems"]);
    assert_eq!(fs::read_to_string(&token_file)?, "created-secret\n");
    assert_eq!(directory_names(temp.path())?, vec!["default.token"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::symlink_metadata(&token_file)?.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    Ok(())
}

#[test]
fn write_token_file_keeps_file_that_appeared_and_names_temporary_file() -> Result<()> {
    let temp = TempDir::new()?;
    let token_file = temp.path().join("default.token");
    fs::write(&token_file, "old-secret\n")?;

    let error = write_token_file(&token_file, "new-secret")
        .err()
        .ok_or_else(|| anyhow!("the write must fail when the target exists"))?;
    let message = format!("{error:#}");

    assert_eq!(fs::read_to_string(&token_file)?, "old-secret\n");
    assert!(!message.contains("new-secret"), "no token in the error");
    let names = directory_names(temp.path())?;
    assert_eq!(names.len(), 2, "the temporary file must stay: {names:?}");
    let temp_name = names
        .iter()
        .find(|name| name.as_str() != "default.token")
        .ok_or_else(|| anyhow!("no temporary file"))?;
    let temp_path = temp.path().join(temp_name);
    assert!(
        message.contains(&temp_path.display().to_string()),
        "the error must name the temporary file: {message}"
    );
    assert_eq!(fs::read_to_string(&temp_path)?, "new-secret\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&temp_path)?.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    Ok(())
}

#[test]
fn write_token_file_creates_private_file_without_temporary_file() -> Result<()> {
    let temp = TempDir::new()?;
    let token_file = temp.path().join("default.token");

    write_token_file(&token_file, "new-secret")?;

    assert_eq!(fs::read_to_string(&token_file)?, "new-secret\n");
    assert_eq!(directory_names(temp.path())?, vec!["default.token"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&token_file)?.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    Ok(())
}
