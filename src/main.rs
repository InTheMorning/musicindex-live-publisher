use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use musicindex_live_publisher::{
    ConfigOverrides, DEFAULT_CONFIG_PATH, DEFAULT_DEBOUNCE_WINDOW, DropEvent, DropEventKind,
    DropWatcher, LiveValuePayload, PublishSchedule, RelayClient, RelayPublisher, load_config,
    write_token_file,
};
use notify::event::{CreateKind, ModifyKind, RemoveKind, RenameMode};
use notify::{EventKind, RecursiveMode, Watcher};
use tracing_subscriber::EnvFilter;

/// How often an idle watch loop asks whether relay publishing is still alive.
const HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(1);

fn main() -> Result<()> {
    let cli = Cli::parse(std::env::args_os())?;
    init_tracing(cli.verbose)?;

    if let Command::Provision {
        endpoint,
        token_file,
        target,
    } = cli.command
    {
        return provision(&endpoint, &token_file, &target);
    }

    let config_path = cli
        .config
        .clone()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH));
    let config = load_config(
        &config_path,
        ConfigOverrides {
            watch_dir: cli.watch_dir,
            endpoint: cli.endpoint,
        },
    )?;
    tracing::info!(
        config = %config_path.display(),
        watch_dir = %config.watch_dir.display(),
        endpoint = %config.endpoint,
        target_count = config.targets.len(),
        "loaded publisher config"
    );
    for target in &config.targets {
        tracing::info!(
            target = %target.name,
            event_id = %target.event_id,
            stream_delay_secs = target.stream_delay.as_secs_f64(),
            "configured publish target"
        );
    }

    let publisher = if cli.dry_run {
        None
    } else {
        Some(RelayPublisher::start(&config)?)
    };
    let targets = config
        .targets
        .iter()
        .map(|target| target.watch_target())
        .collect();
    let mut processor = DropWatcher::new_targets(targets, DEFAULT_DEBOUNCE_WINDOW);
    let mut schedule = PublishSchedule::new(
        config
            .targets
            .iter()
            .map(|target| (target.event_id.clone(), target.stream_delay))
            .collect(),
    );

    wait_for_watch_dir(&config.watch_dir)?;
    // Startup state is recovery, not a track change: the drop file may have
    // been sitting there for most of a song, and holding it would leave the
    // relay serving nothing for the length of the delay. Emit it directly.
    emit_payloads(
        processor.initial_payloads(&config.watch_dir)?,
        publisher.as_ref(),
    )?;
    run_watch_loop(
        &config.watch_dir,
        &mut processor,
        &mut schedule,
        publisher.as_ref(),
    )
}

#[derive(Debug)]
struct Cli {
    watch_dir: Option<PathBuf>,
    endpoint: Option<String>,
    dry_run: bool,
    config: Option<PathBuf>,
    verbose: bool,
    command: Command,
}

impl Default for Cli {
    fn default() -> Self {
        Self {
            watch_dir: None,
            endpoint: None,
            dry_run: false,
            config: None,
            verbose: false,
            command: Command::Run,
        }
    }
}

#[derive(Debug, Default)]
enum Command {
    #[default]
    Run,
    Provision {
        endpoint: String,
        token_file: PathBuf,
        target: String,
    },
}

impl Cli {
    fn parse<I, S>(args: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        let mut cli = Self::default();
        let mut args = args.into_iter().map(Into::into);
        let _program = args.next();

        let remaining = args.collect::<Vec<_>>();
        if remaining
            .first()
            .is_some_and(|value| value == OsStr::new("provision"))
        {
            cli.command = parse_provision(remaining.into_iter().skip(1))?;
            return Ok(cli);
        }
        let mut args = remaining.into_iter();

        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("--watch-dir") => cli.watch_dir = Some(next_path(&mut args, "--watch-dir")?),
                Some("--endpoint") => cli.endpoint = Some(next_string(&mut args, "--endpoint")?),
                Some("--dry-run") => cli.dry_run = true,
                Some("--config") => cli.config = Some(next_path(&mut args, "--config")?),
                Some("--verbose") => cli.verbose = true,
                Some(flag) if flag.starts_with("--") => return Err(anyhow!("unknown flag {flag}")),
                Some(value) => return Err(anyhow!("unexpected argument {value}")),
                None => {
                    return Err(anyhow!(
                        "argument is not valid UTF-8: {}",
                        arg.to_string_lossy()
                    ));
                }
            }
        }

        Ok(cli)
    }
}

fn parse_provision(mut args: impl Iterator<Item = OsString>) -> Result<Command> {
    let mut endpoint = None;
    let mut token_file = None;
    let mut target = None;

    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--endpoint") => endpoint = Some(next_string(&mut args, "--endpoint")?),
            Some("--token-file") => token_file = Some(next_path(&mut args, "--token-file")?),
            Some("--target") => target = Some(next_string(&mut args, "--target")?),
            Some(flag) if flag.starts_with("--") => return Err(anyhow!("unknown flag {flag}")),
            Some(value) => return Err(anyhow!("unexpected argument {value}")),
            None => {
                return Err(anyhow!(
                    "argument is not valid UTF-8: {}",
                    arg.to_string_lossy()
                ));
            }
        }
    }

    let target = target.unwrap_or_else(|| "default".to_owned());
    if target.trim().is_empty() {
        return Err(anyhow!(
            "provision requires --target <name> to be non-empty"
        ));
    }

    Ok(Command::Provision {
        endpoint: endpoint.ok_or_else(|| anyhow!("provision requires --endpoint <url>"))?,
        token_file: token_file.ok_or_else(|| anyhow!("provision requires --token-file <path>"))?,
        target,
    })
}

fn next_path(args: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<PathBuf> {
    let value = args
        .next()
        .ok_or_else(|| anyhow!("{flag} requires a value"))?;
    if value == OsStr::new("") {
        return Err(anyhow!("{flag} requires a non-empty value"));
    }
    Ok(PathBuf::from(value))
}

fn next_string(args: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<String> {
    let value = args
        .next()
        .ok_or_else(|| anyhow!("{flag} requires a value"))?;
    if value == OsStr::new("") {
        return Err(anyhow!("{flag} requires a non-empty value"));
    }
    value
        .into_string()
        .map_err(|value| anyhow!("argument is not valid UTF-8: {}", value.to_string_lossy()))
}

fn init_tracing(verbose: bool) -> Result<()> {
    let default_level = if verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level)),
        )
        .with_writer(std::io::stderr)
        .try_init()
        .map_err(|error| anyhow!("initialize tracing subscriber: {error}"))
}

fn wait_for_watch_dir(watch_dir: &Path) -> Result<()> {
    let mut logged = false;
    loop {
        match watch_dir.try_exists() {
            Ok(true) if watch_dir.is_dir() => return Ok(()),
            Ok(true) => {
                return Err(anyhow!(
                    "watch path is not a directory: {}",
                    watch_dir.display()
                ));
            }
            Ok(false) => {
                if !logged {
                    tracing::warn!(path = %watch_dir.display(), "watch directory does not exist; waiting");
                    logged = true;
                }
                thread::sleep(Duration::from_secs(1));
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("check watch directory {}", watch_dir.display()));
            }
        }
    }
}

fn run_watch_loop(
    watch_dir: &Path,
    processor: &mut DropWatcher,
    schedule: &mut PublishSchedule,
    publisher: Option<&RelayPublisher>,
) -> Result<()> {
    let (sender, receiver) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(sender).context("create filesystem watcher")?;
    watcher
        .watch(watch_dir, RecursiveMode::NonRecursive)
        .with_context(|| format!("watch {}", watch_dir.display()))?;

    loop {
        // Poll rather than block forever so a relay worker that stopped
        // fatally surfaces within a second, instead of waiting for whenever the
        // next track happens to change. A pending stream-delay deadline
        // shortens the wait further, so a held payload is released on time
        // rather than at the next health check.
        let event = match receiver.recv_timeout(next_wakeup(schedule, Instant::now())) {
            Ok(event) => event,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                emit_payloads(schedule.take_due(Instant::now()), publisher)?;
                if let Some(publisher) = publisher {
                    publisher.check_health()?;
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(anyhow!("filesystem watcher stopped"));
            }
        };
        match event {
            Ok(event) => {
                for drop_event in normalize_notify_event(event) {
                    let now = Instant::now();
                    for payload in processor.process_event(drop_event, now)? {
                        schedule.schedule(payload, now);
                    }
                }
                emit_payloads(schedule.take_due(Instant::now()), publisher)?;
            }
            Err(error) => return Err(error).context("watch directory event error"),
        }
    }
}

/// Returns how long the watch loop may block before it must act again.
///
/// A pending stream-delay deadline takes precedence over the health check
/// interval, but never lengthens it.
fn next_wakeup(schedule: &PublishSchedule, now: Instant) -> Duration {
    schedule
        .next_deadline()
        .map_or(HEALTH_CHECK_INTERVAL, |deadline| {
            deadline
                .saturating_duration_since(now)
                .min(HEALTH_CHECK_INTERVAL)
        })
}

fn normalize_notify_event(event: notify::Event) -> Vec<DropEvent> {
    match event.kind {
        EventKind::Create(CreateKind::File | CreateKind::Any) => upsert_events(event.paths),
        EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Metadata(_) | ModifyKind::Any) => {
            upsert_events(event.paths)
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => upsert_events(event.paths),
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => event
            .paths
            .into_iter()
            .nth(1)
            .map(|path| {
                vec![DropEvent {
                    kind: DropEventKind::Upsert,
                    path,
                }]
            })
            .unwrap_or_default(),
        EventKind::Remove(RemoveKind::File | RemoveKind::Any) => remove_events(event.paths),
        _ => Vec::new(),
    }
}

fn upsert_events(paths: Vec<PathBuf>) -> Vec<DropEvent> {
    paths
        .into_iter()
        .map(|path| DropEvent {
            kind: DropEventKind::Upsert,
            path,
        })
        .collect()
}

fn remove_events(paths: Vec<PathBuf>) -> Vec<DropEvent> {
    paths
        .into_iter()
        .map(|path| DropEvent {
            kind: DropEventKind::Remove,
            path,
        })
        .collect()
}

fn emit_payloads(
    payloads: Vec<LiveValuePayload>,
    publisher: Option<&RelayPublisher>,
) -> Result<()> {
    for payload in payloads {
        if let Some(publisher) = publisher {
            publisher.publish(payload)?;
        } else {
            println!("{}", serde_json::to_string_pretty(&payload)?);
        }
    }
    Ok(())
}

fn provision(endpoint: &str, token_file: &Path, target: &str) -> Result<()> {
    let client = RelayClient::new(musicindex_live_publisher::DEFAULT_REQUEST_TIMEOUT)?;
    let item = client.provision(endpoint)?;
    write_token_file(token_file, &item.broadcaster_token)?;

    println!("Live item provisioned.");
    println!(
        "The broadcaster token was returned exactly once and cannot be recovered by the relay."
    );
    println!("Token written to {}", token_file.display());
    println!();
    println!("[[target]]");
    println!("name = {}", toml_string(target));
    println!("event_id = {}", toml_string(&item.event_id));
    println!(
        "token_file = {}",
        toml_string(&token_file.display().to_string())
    );

    Ok(())
}

fn toml_string(value: &str) -> String {
    toml::Value::String(value.to_owned()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provision_defaults_target_to_default() -> Result<()> {
        let command = parse_provision(
            [
                "--endpoint",
                "https://api.example.test",
                "--token-file",
                "/tmp/default.token",
            ]
            .into_iter()
            .map(OsString::from),
        )?;

        let Command::Provision { target, .. } = command else {
            panic!("expected provision command");
        };
        assert_eq!(target, "default");
        Ok(())
    }

    #[test]
    fn provision_accepts_target_name() -> Result<()> {
        let command = parse_provision(
            [
                "--endpoint",
                "https://api.example.test",
                "--token-file",
                "/tmp/late-night.token",
                "--target",
                "late-night",
            ]
            .into_iter()
            .map(OsString::from),
        )?;

        let Command::Provision { target, .. } = command else {
            panic!("expected provision command");
        };
        assert_eq!(target, "late-night");
        Ok(())
    }

    #[test]
    fn toml_string_renders_parseable_string() -> Result<()> {
        let parsed: toml::Value =
            toml::from_str(&format!("value = {}", toml_string("late \"night\"")))?;

        assert_eq!(parsed["value"].as_str(), Some("late \"night\""));
        Ok(())
    }
}
