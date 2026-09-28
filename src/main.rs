use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use musicindex_live_publisher::{
    ConfigEditError, ConfigOverrides, DEFAULT_CONFIG_PATH, DEFAULT_DEBOUNCE_WINDOW, DropEvent,
    DropEventKind, DropWatcher, LiveValuePayload, ProducerState, PublishSchedule,
    RedactedPublisherConfig, RelayClient, RelayPublisher, TargetConfigEdit, TargetConfigSummary,
    add_target_to_config, list_config_targets, load_config, probe_producer,
    remove_target_from_config, show_config, write_token_file,
};
use notify::event::{CreateKind, ModifyKind, RemoveKind, RenameMode};
use notify::{EventKind, RecursiveMode, Watcher};
use serde::Serialize;
use tracing_subscriber::EnvFilter;

/// How often an idle watch loop asks whether relay publishing is still alive.
const HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(1);
const EXIT_TARGET_EXISTS: i32 = 2;
const EXIT_TARGET_NOT_FOUND: i32 = 3;

fn main() -> Result<()> {
    let cli = Cli::parse(std::env::args_os())?;
    init_tracing(cli.verbose)?;
    let json_errors = cli.command.wants_json_errors();

    if let Err(error) = run_cli(cli) {
        return handle_command_error(error, json_errors);
    }

    Ok(())
}

fn run_cli(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Provision {
            endpoint,
            token_file,
            target,
            json,
        } => return provision(&endpoint, &token_file, &target, json),
        Command::Target(command) => return run_target_command(command),
        Command::Config(command) => return run_config_command(command),
        Command::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Command::Run => {}
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
    let producer_state = probe_producer(&config.watch_dir)?;
    tracing::info!(producer = %producer_state, "probed producer lock at startup");
    // The startup probe result is the first `Producer` command every worker
    // sees (relay-lease-task-004), before any payload. This gates whether a
    // worker may ever consider a keepalive: see `keepalive_wait` in
    // `src/relay.rs`.
    if let Some(publisher) = publisher.as_ref() {
        publisher.set_producer(producer_state);
    }
    // Startup state is recovery, not a track change: the drop file may have
    // been sitting there for most of a song, and holding it would leave the
    // relay serving nothing for the length of the delay. Emit it directly.
    let mut pending_missing = HashMap::new();
    emit_payloads(
        processor.startup_payloads(&config.watch_dir, producer_state)?,
        publisher.as_ref(),
        &mut pending_missing,
        producer_state,
    )?;
    run_watch_loop(
        &config.watch_dir,
        &mut processor,
        &mut schedule,
        publisher.as_ref(),
        producer_state,
        pending_missing,
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
        json: bool,
    },
    Target(TargetCommand),
    Config(ConfigCommand),
    Version,
}

impl Command {
    fn wants_json_errors(&self) -> bool {
        match self {
            Self::Provision { json, .. } => *json,
            Self::Target(TargetCommand::List(command)) => command.json,
            Self::Config(ConfigCommand::Show(command)) => command.json,
            Self::Run | Self::Target(_) | Self::Version => false,
        }
    }
}

#[derive(Debug)]
enum TargetCommand {
    Add(TargetAddCommand),
    List(TargetListCommand),
    Remove(TargetRemoveCommand),
}

#[derive(Debug)]
struct TargetAddCommand {
    config: PathBuf,
    edit: TargetConfigEdit,
    replace: bool,
}

#[derive(Debug)]
struct TargetListCommand {
    config: PathBuf,
    json: bool,
}

#[derive(Debug)]
struct TargetRemoveCommand {
    config: PathBuf,
    name: String,
}

#[derive(Debug)]
enum ConfigCommand {
    Show(ConfigShowCommand),
}

#[derive(Debug)]
struct ConfigShowCommand {
    config: PathBuf,
    json: bool,
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
            .is_some_and(|value| value == OsStr::new("--version"))
        {
            if remaining.len() != 1 {
                return Err(anyhow!("--version does not accept arguments"));
            }
            cli.command = Command::Version;
            return Ok(cli);
        }
        if remaining
            .first()
            .is_some_and(|value| value == OsStr::new("provision"))
        {
            cli.command = parse_provision(remaining.into_iter().skip(1))?;
            return Ok(cli);
        }
        if remaining
            .first()
            .is_some_and(|value| value == OsStr::new("target"))
        {
            cli.command = parse_target(remaining.into_iter().skip(1))?;
            return Ok(cli);
        }
        if remaining
            .first()
            .is_some_and(|value| value == OsStr::new("config"))
        {
            cli.command = parse_config(remaining.into_iter().skip(1))?;
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

fn parse_config(mut args: impl Iterator<Item = OsString>) -> Result<Command> {
    let subcommand = args
        .next()
        .ok_or_else(|| anyhow!("config requires a subcommand"))?;
    let command = match subcommand.to_str() {
        Some("show") => ConfigCommand::Show(parse_config_show(args)?),
        Some(value) => return Err(anyhow!("unknown config subcommand {value}")),
        None => {
            return Err(anyhow!(
                "argument is not valid UTF-8: {}",
                subcommand.to_string_lossy()
            ));
        }
    };
    Ok(Command::Config(command))
}

fn parse_config_show(mut args: impl Iterator<Item = OsString>) -> Result<ConfigShowCommand> {
    let mut config = None;
    let mut json = false;

    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--config") => config = Some(next_path(&mut args, "--config")?),
            Some("--json") => json = true,
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

    Ok(ConfigShowCommand {
        config: config.unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH)),
        json,
    })
}

fn parse_target(mut args: impl Iterator<Item = OsString>) -> Result<Command> {
    let subcommand = args
        .next()
        .ok_or_else(|| anyhow!("target requires a subcommand"))?;
    let command = match subcommand.to_str() {
        Some("add") => TargetCommand::Add(parse_target_add(args)?),
        Some("list") => TargetCommand::List(parse_target_list(args)?),
        Some("remove") => TargetCommand::Remove(parse_target_remove(args)?),
        Some(value) => return Err(anyhow!("unknown target subcommand {value}")),
        None => {
            return Err(anyhow!(
                "argument is not valid UTF-8: {}",
                subcommand.to_string_lossy()
            ));
        }
    };
    Ok(Command::Target(command))
}

fn parse_target_add(mut args: impl Iterator<Item = OsString>) -> Result<TargetAddCommand> {
    let mut config = None;
    let mut name = None;
    let mut event_id = None;
    let mut token_file = None;
    let mut stream_delay_secs = None;
    let mut replace = false;

    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--config") => config = Some(next_path(&mut args, "--config")?),
            Some("--name") => name = Some(next_string(&mut args, "--name")?),
            Some("--event-id") => event_id = Some(next_string(&mut args, "--event-id")?),
            Some("--token-file") => token_file = Some(next_path(&mut args, "--token-file")?),
            Some("--stream-delay-secs") => {
                stream_delay_secs = Some(next_f64(&mut args, "--stream-delay-secs")?);
            }
            Some("--replace") => replace = true,
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

    Ok(TargetAddCommand {
        config: config.unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH)),
        edit: TargetConfigEdit {
            name: name.ok_or_else(|| anyhow!("target add requires --name <name>"))?,
            event_id: event_id
                .ok_or_else(|| anyhow!("target add requires --event-id <event_id>"))?,
            token_file: token_file
                .ok_or_else(|| anyhow!("target add requires --token-file <path>"))?,
            stream_delay_secs,
        },
        replace,
    })
}

fn parse_target_list(mut args: impl Iterator<Item = OsString>) -> Result<TargetListCommand> {
    let mut config = None;
    let mut json = false;

    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--config") => config = Some(next_path(&mut args, "--config")?),
            Some("--json") => json = true,
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

    Ok(TargetListCommand {
        config: config.unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH)),
        json,
    })
}

fn parse_target_remove(mut args: impl Iterator<Item = OsString>) -> Result<TargetRemoveCommand> {
    let mut config = None;
    let mut name = None;

    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--config") => config = Some(next_path(&mut args, "--config")?),
            Some("--name") => name = Some(next_string(&mut args, "--name")?),
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

    Ok(TargetRemoveCommand {
        config: config.unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH)),
        name: name.ok_or_else(|| anyhow!("target remove requires --name <name>"))?,
    })
}

fn parse_provision(mut args: impl Iterator<Item = OsString>) -> Result<Command> {
    let mut endpoint = None;
    let mut token_file = None;
    let mut target = None;
    let mut json = false;

    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--endpoint") => endpoint = Some(next_string(&mut args, "--endpoint")?),
            Some("--token-file") => token_file = Some(next_path(&mut args, "--token-file")?),
            Some("--target") => target = Some(next_string(&mut args, "--target")?),
            Some("--json") => json = true,
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
        json,
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

fn next_f64(args: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<f64> {
    let value = next_string(args, flag)?;
    value
        .parse()
        .with_context(|| format!("{flag} must be a number"))
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

fn run_target_command(command: TargetCommand) -> Result<()> {
    match command {
        TargetCommand::Add(command) => {
            let target_name = command.edit.name.clone();
            add_target_to_config(&command.config, &command.edit, command.replace)?;
            println!("target {target_name} written");
        }
        TargetCommand::List(command) => {
            let targets = list_config_targets(&command.config)?;
            if command.json {
                println!("{}", render_target_list_json(&targets)?);
            } else {
                print!("{}", render_target_list_text(&targets));
            }
        }
        TargetCommand::Remove(command) => {
            remove_target_from_config(&command.config, &command.name)?;
            println!("target {} removed", command.name);
        }
    }
    Ok(())
}

fn run_config_command(command: ConfigCommand) -> Result<()> {
    match command {
        ConfigCommand::Show(command) => {
            let config = show_config(&command.config)?;
            if command.json {
                println!("{}", render_config_show_json(&config)?);
            } else {
                print!("{}", render_config_show_text(&config));
            }
        }
    }
    Ok(())
}

fn handle_command_error(error: anyhow::Error, json: bool) -> Result<()> {
    let exit_code = command_exit_code(&error).unwrap_or(1);
    if json {
        println!("{}", serde_json::json!({ "error": format!("{error:#}") }));
        std::process::exit(exit_code);
    }
    if command_exit_code(&error).is_some() {
        eprintln!("{error}");
        std::process::exit(exit_code);
    }
    Err(error)
}

fn command_exit_code(error: &anyhow::Error) -> Option<i32> {
    error
        .downcast_ref::<ConfigEditError>()
        .map(|error| match error {
            ConfigEditError::TargetExists(_) => EXIT_TARGET_EXISTS,
            ConfigEditError::TargetNotFound(_) => EXIT_TARGET_NOT_FOUND,
        })
}

#[derive(Serialize)]
struct TargetListOutput<'a> {
    targets: &'a [TargetConfigSummary],
}

fn render_target_list_json(targets: &[TargetConfigSummary]) -> Result<String> {
    serde_json::to_string_pretty(&TargetListOutput { targets })
        .context("serialize target list JSON")
}

fn render_target_list_text(targets: &[TargetConfigSummary]) -> String {
    let mut output = String::new();
    for target in targets {
        output.push_str(&format!(
            "{}\tevent_id={}\ttoken_file={}\tstream_delay_secs={}\n",
            target.name,
            target.event_id,
            target.token_file.display(),
            target.stream_delay_secs
        ));
    }
    output
}

fn render_config_show_json(config: &RedactedPublisherConfig) -> Result<String> {
    serde_json::to_string_pretty(config).context("serialize config show JSON")
}

fn render_config_show_text(config: &RedactedPublisherConfig) -> String {
    let mut output = format!(
        "watch_dir={}\nendpoint={}\n",
        config.watch_dir.display(),
        config.endpoint
    );
    for target in &config.targets {
        output.push_str(&format!(
            "target {}\tevent_id={}\ttoken_file={}\tstream_delay_secs={}\n",
            target.name,
            target.event_id,
            target.token_file.display(),
            target.stream_delay_secs
        ));
    }
    output
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
    mut producer_state: ProducerState,
    mut pending_missing: HashMap<String, String>,
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
                producer_state = update_producer_state(
                    watch_dir,
                    processor,
                    schedule,
                    publisher,
                    &mut pending_missing,
                    producer_state,
                    Instant::now(),
                )?;
                emit_payloads(
                    schedule.take_due(Instant::now()),
                    publisher,
                    &mut pending_missing,
                    producer_state,
                )?;
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
                let now = Instant::now();
                producer_state = update_producer_state(
                    watch_dir,
                    processor,
                    schedule,
                    publisher,
                    &mut pending_missing,
                    producer_state,
                    now,
                )?;
                if matches!(producer_state, ProducerState::Missing) {
                    tracing::debug!("discarding drop file event while the producer is missing");
                } else {
                    for drop_event in normalize_notify_event(event) {
                        for payload in processor.process_event(drop_event, now)? {
                            schedule.schedule(payload, now);
                        }
                    }
                }
                emit_payloads(
                    schedule.take_due(Instant::now()),
                    publisher,
                    &mut pending_missing,
                    producer_state,
                )?;
            }
            Err(error) => return Err(error).context("watch directory event error"),
        }
    }
}

/// Probes the producer lock and, on a change, publishes the transition's
/// block for each affected target (ADR 0005).
///
/// A change from running to missing publishes the dead block for each
/// target through `schedule` and clears the watcher's block identity state.
/// It also records each target's dead-block `blockGuid` in `pending_missing`,
/// keyed by `eventGuid`. `emit_payloads` reads that record: only once a
/// target's own dead block leaves `schedule` does its worker learn the
/// producer is missing and stop its keepalive (relay-lease-task-004). A
/// stream delay can outlast the relay lease, so stopping the keepalive any
/// earlier could let the lease expire before the dead block goes out.
///
/// A change from missing to running publishes nothing and does not scan the
/// drop directory. The producer takes its lock before it removes a stale drop
/// file, so a scan at that moment could publish the stale track. The producer
/// writes its present track again after it starts, and that event reaches the
/// watch loop as usual. The dead block stays live until then. This change
/// also tells every worker `Producer(Running)` at once, and forgets any
/// `pending_missing` record left over from a dead block that had not yet
/// been released: a worker must not later be told the producer is missing
/// when it has since returned.
///
/// # Errors
///
/// Returns an error when the probe fails.
fn update_producer_state(
    watch_dir: &Path,
    processor: &mut DropWatcher,
    schedule: &mut PublishSchedule,
    publisher: Option<&RelayPublisher>,
    pending_missing: &mut HashMap<String, String>,
    previous: ProducerState,
    now: Instant,
) -> Result<ProducerState> {
    let current = probe_producer(watch_dir)?;
    if current == previous {
        return Ok(current);
    }

    match current {
        ProducerState::Missing => {
            tracing::info!(
                producer = %current,
                "producer lock freed; publishing the dead block for each target"
            );
            for payload in processor.producer_missing_payloads() {
                pending_missing.insert(payload.event_guid.clone(), payload.block_guid.clone());
                schedule.schedule(payload, now);
            }
        }
        ProducerState::Running => {
            tracing::info!(
                producer = %current,
                "producer lock held again; waiting for the producer to write its drop file"
            );
            pending_missing.clear();
            if let Some(publisher) = publisher {
                publisher.set_producer(ProducerState::Running);
            }
        }
    }
    Ok(current)
}

/// Decides which targets' workers should learn the producer is missing, for
/// one batch of payloads `PublishSchedule` just released.
///
/// `pending` maps an `eventGuid` to the `blockGuid` `update_producer_state`
/// recorded for that target's dead block on the last missing-producer
/// transition. A released payload that matches a recorded pair is that
/// transition's dead block reaching the relay: its entry is removed from
/// `pending` either way, and its `eventGuid` is returned only if
/// `producer_state` is still [`ProducerState::Missing`]. When the producer
/// has since returned, the transition is stale, so the match is still
/// cleared but nothing is returned: relay-lease-task-004 forbids sending a
/// stale `Producer(Missing)` to a worker whose producer already came back.
fn missing_transitions_for_released(
    released: &[LiveValuePayload],
    pending: &mut HashMap<String, String>,
    producer_state: ProducerState,
) -> Vec<String> {
    let mut targets = Vec::new();
    for payload in released {
        let matches_recorded = pending
            .get(&payload.event_guid)
            .is_some_and(|block_guid| *block_guid == payload.block_guid);
        if !matches_recorded {
            continue;
        }
        pending.remove(&payload.event_guid);
        if matches!(producer_state, ProducerState::Missing) {
            targets.push(payload.event_guid.clone());
        }
    }
    targets
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

/// Publishes each released payload and, for a target whose dead block just
/// went out, tells that target's worker the producer is missing.
///
/// See `missing_transitions_for_released` and `update_producer_state` for
/// the rule this follows (ADR 0005, relay-lease-task-004). Each payload is
/// sent to its target's worker before that target's `Producer(Missing)`
/// command: they share one channel, so this order is what the worker
/// receives them in.
fn emit_payloads(
    payloads: Vec<LiveValuePayload>,
    publisher: Option<&RelayPublisher>,
    pending_missing: &mut HashMap<String, String>,
    producer_state: ProducerState,
) -> Result<()> {
    let missing_targets =
        missing_transitions_for_released(&payloads, pending_missing, producer_state);
    for payload in payloads {
        if let Some(publisher) = publisher {
            publisher.publish(payload)?;
        } else {
            println!("{}", serde_json::to_string_pretty(&payload)?);
        }
    }
    if let Some(publisher) = publisher {
        for event_id in &missing_targets {
            publisher.set_producer_for_target(event_id, ProducerState::Missing)?;
        }
    }
    Ok(())
}

fn provision(endpoint: &str, token_file: &Path, target: &str, json: bool) -> Result<()> {
    let client = RelayClient::new(musicindex_live_publisher::DEFAULT_REQUEST_TIMEOUT)?;
    let item = client.provision(endpoint)?;
    write_token_file(token_file, &item.broadcaster_token)?;

    if json {
        println!("{}", render_provision_json(&item, token_file, target)?);
        return Ok(());
    }

    print_provision_text(&item, token_file, target);
    Ok(())
}

fn print_provision_text(
    item: &musicindex_live_publisher::ProvisionedLiveItem,
    token_file: &Path,
    target: &str,
) {
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
}

#[derive(Serialize)]
struct ProvisionJsonOutput<'a> {
    event_id: &'a str,
    token_file: String,
    target: &'a str,
    metadata_url: &'a str,
    remote_value_url: &'a str,
    events_url: &'a str,
    socket_io_url: &'a str,
}

fn render_provision_json(
    item: &musicindex_live_publisher::ProvisionedLiveItem,
    token_file: &Path,
    target: &str,
) -> Result<String> {
    serde_json::to_string_pretty(&ProvisionJsonOutput {
        event_id: &item.event_id,
        token_file: token_file.display().to_string(),
        target,
        metadata_url: &item.metadata_url,
        remote_value_url: &item.remote_value_url,
        events_url: &item.events_url,
        socket_io_url: &item.socket_io_url,
    })
    .context("serialize provision JSON")
}

fn toml_string(value: &str) -> String {
    toml::Value::String(value.to_owned()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload_for(event_guid: &str, block_guid: &str) -> LiveValuePayload {
        musicindex_live_publisher::dead_payload(event_guid, block_guid)
    }

    #[test]
    fn missing_transitions_ignores_a_released_payload_with_a_different_block_guid() {
        let mut pending = HashMap::from([("event".to_owned(), "dead-guid".to_owned())]);
        let released = vec![payload_for("event", "other-guid")];

        let targets =
            missing_transitions_for_released(&released, &mut pending, ProducerState::Missing);

        assert!(targets.is_empty());
        assert_eq!(pending.get("event"), Some(&"dead-guid".to_owned()));
    }

    #[test]
    fn missing_transitions_fires_when_the_recorded_dead_block_is_released() {
        let mut pending = HashMap::from([("event".to_owned(), "dead-guid".to_owned())]);
        let released = vec![payload_for("event", "dead-guid")];

        let targets =
            missing_transitions_for_released(&released, &mut pending, ProducerState::Missing);

        assert_eq!(targets, vec!["event".to_owned()]);
        assert!(pending.is_empty());
    }

    #[test]
    fn missing_transitions_forgets_the_recorded_guid_but_does_not_fire_once_the_producer_returned()
    {
        let mut pending = HashMap::from([("event".to_owned(), "dead-guid".to_owned())]);
        let released = vec![payload_for("event", "dead-guid")];

        let targets =
            missing_transitions_for_released(&released, &mut pending, ProducerState::Running);

        assert!(targets.is_empty());
        assert!(pending.is_empty());
    }

    #[test]
    fn missing_transitions_handles_more_than_one_target_independently() {
        let mut pending = HashMap::from([
            ("event-a".to_owned(), "dead-a".to_owned()),
            ("event-b".to_owned(), "dead-b".to_owned()),
        ]);
        let released = vec![
            payload_for("event-a", "dead-a"),
            payload_for("event-b", "not-the-recorded-one"),
        ];

        let targets =
            missing_transitions_for_released(&released, &mut pending, ProducerState::Missing);

        assert_eq!(targets, vec!["event-a".to_owned()]);
        assert_eq!(pending.get("event-b"), Some(&"dead-b".to_owned()));
        assert!(!pending.contains_key("event-a"));
    }

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

        let Command::Provision { target, json, .. } = command else {
            panic!("expected provision command");
        };
        assert_eq!(target, "default");
        assert!(!json);
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

        let Command::Provision { target, json, .. } = command else {
            panic!("expected provision command");
        };
        assert_eq!(target, "late-night");
        assert!(!json);
        Ok(())
    }

    #[test]
    fn provision_accepts_json_flag() -> Result<()> {
        let command = parse_provision(
            [
                "--endpoint",
                "https://api.example.test",
                "--token-file",
                "/tmp/default.token",
                "--json",
            ]
            .into_iter()
            .map(OsString::from),
        )?;

        let Command::Provision { json, .. } = command else {
            panic!("expected provision command");
        };
        assert!(json);
        Ok(())
    }

    #[test]
    fn target_add_parser_accepts_all_options() -> Result<()> {
        let command = parse_target(
            [
                "add",
                "--config",
                "/tmp/publisher.toml",
                "--name",
                "late-night",
                "--event-id",
                "event-late-night",
                "--token-file",
                "/tmp/late-night.token",
                "--stream-delay-secs",
                "12.5",
                "--replace",
            ]
            .into_iter()
            .map(OsString::from),
        )?;

        let Command::Target(TargetCommand::Add(command)) = command else {
            panic!("expected target add command");
        };
        assert_eq!(command.config, PathBuf::from("/tmp/publisher.toml"));
        assert_eq!(command.edit.name, "late-night");
        assert_eq!(command.edit.event_id, "event-late-night");
        assert_eq!(
            command.edit.token_file,
            PathBuf::from("/tmp/late-night.token")
        );
        assert_eq!(command.edit.stream_delay_secs, Some(12.5));
        assert!(command.replace);
        Ok(())
    }

    #[test]
    fn target_list_parser_accepts_json() -> Result<()> {
        let command = parse_target(
            ["list", "--config", "/tmp/publisher.toml", "--json"]
                .into_iter()
                .map(OsString::from),
        )?;

        let Command::Target(TargetCommand::List(command)) = command else {
            panic!("expected target list command");
        };
        assert_eq!(command.config, PathBuf::from("/tmp/publisher.toml"));
        assert!(command.json);
        Ok(())
    }

    #[test]
    fn target_remove_parser_requires_name() {
        let error = parse_target(["remove"].into_iter().map(OsString::from));

        assert!(
            error.is_err_and(|error| error.to_string().contains("target remove requires --name"))
        );
    }

    #[test]
    fn target_list_text_contains_fields_without_token_content() {
        let targets = vec![TargetConfigSummary {
            name: "default".to_owned(),
            event_id: "event-default".to_owned(),
            token_file: PathBuf::from("/tmp/default.token"),
            stream_delay_secs: 12.5,
        }];

        let output = render_target_list_text(&targets);

        assert!(output.contains("default"));
        assert!(output.contains("event-default"));
        assert!(output.contains("/tmp/default.token"));
        assert!(!output.contains("secret-token"));
    }

    #[test]
    fn target_list_json_is_machine_readable_without_token_content() -> Result<()> {
        let targets = vec![TargetConfigSummary {
            name: "default".to_owned(),
            event_id: "event-default".to_owned(),
            token_file: PathBuf::from("/tmp/default.token"),
            stream_delay_secs: 0.0,
        }];

        let output = render_target_list_json(&targets)?;
        let value: serde_json::Value = serde_json::from_str(&output)?;

        assert_eq!(value["targets"][0]["name"], "default");
        assert_eq!(value["targets"][0]["event_id"], "event-default");
        assert_eq!(value["targets"][0]["token_file"], "/tmp/default.token");
        assert!(!output.contains("secret-token"));
        Ok(())
    }

    #[test]
    fn config_show_parser_accepts_json() -> Result<()> {
        let command = parse_config(
            ["show", "--config", "/tmp/publisher.toml", "--json"]
                .into_iter()
                .map(OsString::from),
        )?;

        let Command::Config(ConfigCommand::Show(command)) = command else {
            panic!("expected config show command");
        };
        assert_eq!(command.config, PathBuf::from("/tmp/publisher.toml"));
        assert!(command.json);
        Ok(())
    }

    #[test]
    fn version_parser_accepts_only_version_flag() -> Result<()> {
        let cli = Cli::parse(["publisher", "--version"])?;

        assert!(matches!(cli.command, Command::Version));
        Ok(())
    }

    #[test]
    fn provision_json_output_redacts_token() -> Result<()> {
        let item = musicindex_live_publisher::ProvisionedLiveItem {
            event_id: "created-event".to_owned(),
            broadcaster_token: "created-secret".to_owned(),
            metadata_url: "/v1/liveitems/created-event/metadata".to_owned(),
            remote_value_url: "/v1/liveitems/created-event/remoteValue".to_owned(),
            events_url: "/v1/liveitems/created-event/events".to_owned(),
            socket_io_url: "/event?event_id=created-event".to_owned(),
        };

        let output = render_provision_json(&item, Path::new("/tmp/default.token"), "default")?;
        let value: serde_json::Value = serde_json::from_str(&output)?;

        assert_eq!(value["event_id"], "created-event");
        assert_eq!(value["token_file"], "/tmp/default.token");
        assert_eq!(value["target"], "default");
        assert!(value.get("broadcaster_token").is_none());
        assert!(!output.contains("created-secret"));
        Ok(())
    }

    #[test]
    fn config_show_json_output_redacts_secret_fields() -> Result<()> {
        let config = RedactedPublisherConfig {
            watch_dir: PathBuf::from("/tmp/watch"),
            endpoint: "https://api.example.test".to_owned(),
            targets: vec![musicindex_live_publisher::RedactedPublisherTarget {
                name: "default".to_owned(),
                event_id: "event-default".to_owned(),
                token_file: PathBuf::from("/tmp/default.token"),
                stream_delay_secs: 12.5,
            }],
        };

        let output = render_config_show_json(&config)?;
        let value: serde_json::Value = serde_json::from_str(&output)?;

        assert_eq!(value["watch_dir"], "/tmp/watch");
        assert_eq!(value["endpoint"], "https://api.example.test");
        let mut target_keys: Vec<&str> = value["targets"][0]
            .as_object()
            .expect("target should be an object")
            .keys()
            .map(String::as_str)
            .collect();
        target_keys.sort_unstable();
        assert_eq!(
            target_keys,
            vec!["event_id", "name", "stream_delay_secs", "token_file"]
        );
        assert!(!output.contains("secret-token"));
        assert!(!output.contains("03station"));
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
