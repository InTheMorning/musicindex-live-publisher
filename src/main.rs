use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use musicindex_live_publisher::{
    ConfigEditError, ConfigOverrides, DEFAULT_CONFIG_PATH, DEFAULT_DEBOUNCE_WINDOW,
    DISPLAY_FILE_NAME, DisplayEntry, DisplayState, DropEvent, DropEventKind, DropWatcher,
    LiveValuePayload, ProducerState, PublisherConfig, RedactedPublisherConfig, RelayClient,
    RelayPublisher, TargetConfigEdit, TargetConfigSummary, add_target_to_config,
    list_config_targets, load_config, probe_producer, read_display_state,
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
            display_dir = ?target.display_dir,
            "configured publish target"
        );
    }
    let mut display = DisplayPath::from_config(&config);

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
    // Startup state is recovery, not a track change, so it is sent at once,
    // the same way every item leaves through `emit_items` (ADR 0011).
    let mut pending_missing = HashMap::new();
    emit_items(
        processor
            .startup_payloads(&config.watch_dir, producer_state)?
            .into_iter()
            .map(EmitItem::Payload)
            .collect(),
        publisher.as_ref(),
        &mut pending_missing,
        producer_state,
    )?;
    // The display state follows the same recovery rule, after the payloads.
    emit_items(
        display.startup_items(producer_state),
        publisher.as_ref(),
        &mut pending_missing,
        producer_state,
    )?;
    run_watch_loop(
        &config.watch_dir,
        &mut processor,
        &mut display,
        publisher.as_ref(),
        producer_state,
        pending_missing,
    )
}

/// One item ready for `emit_items`: a live value payload or a display state
/// (ADR 0011).
///
/// Both kinds share one worker channel for their target (`src/relay.rs`), so
/// this enum lets `emit_items` send a batch of each kind, in the order the
/// caller built it, with no delay.
#[derive(Debug, Clone, PartialEq)]
#[allow(
    clippy::large_enum_variant,
    reason = "a batch holds a few items, so a box gives no gain"
)]
enum EmitItem {
    /// A live value payload for the payload worker of its target.
    Payload(LiveValuePayload),
    /// A display state for the display worker (ADR 0008).
    Display(DisplayEntry),
}

impl EmitItem {
    /// The payload, when this item is a payload.
    fn as_payload(&self) -> Option<&LiveValuePayload> {
        match self {
            Self::Payload(payload) => Some(payload),
            Self::Display(_) => None,
        }
    }
}

/// One display directory (ADR 0008) and the targets that read it.
#[derive(Debug)]
struct DisplayDir {
    path: PathBuf,
    event_ids: Vec<String>,
    watched: bool,
}

/// The display directories of every target with `display_dir`, and the last
/// display state that the watch loop sent for each target.
#[derive(Debug, Default)]
struct DisplayPath {
    dirs: Vec<DisplayDir>,
    last: HashMap<String, DisplayState>,
}

impl DisplayPath {
    fn from_config(config: &PublisherConfig) -> Self {
        let mut display = Self::default();
        for target in &config.targets {
            let Some(path) = &target.display_dir else {
                continue;
            };
            match display.dirs.iter_mut().find(|dir| dir.path == *path) {
                Some(dir) => dir.event_ids.push(target.event_id.clone()),
                None => display.dirs.push(DisplayDir {
                    path: path.clone(),
                    event_ids: vec![target.event_id.clone()],
                    watched: false,
                }),
            }
        }
        display
    }

    /// The index of the display directory that holds `path`.
    fn dir_index(&self, path: &Path) -> Option<usize> {
        let parent = path.parent()?;
        self.dirs.iter().position(|dir| dir.path == parent)
    }

    /// The display state of each target at startup, for a direct send.
    ///
    /// A running producer gives the state in `display.json`. A missing
    /// producer gives `null`.
    fn startup_items(&mut self, producer: ProducerState) -> Vec<EmitItem> {
        let mut items = Vec::new();
        for index in 0..self.dirs.len() {
            let state = match producer {
                ProducerState::Running => read_display_state(&self.dirs[index].path),
                ProducerState::Missing => Some(DisplayState::null()),
            };
            if let Some(state) = state {
                items.extend(
                    self.changed_entries(index, &state)
                        .into_iter()
                        .map(EmitItem::Display),
                );
            }
        }
        items
    }

    /// Reads `display.json` of one directory and gives its changed entries,
    /// ready to send through `emit_items` at once (ADR 0011).
    fn items_for_dir(&mut self, index: usize) -> Vec<EmitItem> {
        let Some(state) = read_display_state(&self.dirs[index].path) else {
            return Vec::new();
        };
        self.changed_entries(index, &state)
            .into_iter()
            .map(EmitItem::Display)
            .collect()
    }

    /// Gives the display state `null` for each display target, ready to send
    /// through `emit_items` at once (ADR 0008, ADR 0011).
    fn null_items(&mut self) -> Vec<EmitItem> {
        let mut items = Vec::new();
        for index in 0..self.dirs.len() {
            items.extend(
                self.changed_entries(index, &DisplayState::null())
                    .into_iter()
                    .map(EmitItem::Display),
            );
        }
        items
    }

    /// The entries for the targets of one directory whose last sent state is
    /// not `state`. A state that did not change costs no request.
    fn changed_entries(&mut self, index: usize, state: &DisplayState) -> Vec<DisplayEntry> {
        let mut entries = Vec::new();
        for event_id in &self.dirs[index].event_ids {
            if self.last.get(event_id) == Some(state) {
                continue;
            }
            self.last.insert(event_id.clone(), state.clone());
            entries.push(DisplayEntry {
                event_id: event_id.clone(),
                state: state.clone(),
            });
        }
        entries
    }

    /// Adds a watch for each display directory that has none yet, and gives
    /// the index of each directory that it added.
    ///
    /// A directory that does not exist yet gives one warning, and the loop
    /// tries again later. A display directory never stops the payment path.
    fn watch_new_dirs(&mut self, watcher: &mut impl Watcher) -> Vec<usize> {
        let mut added = Vec::new();
        for (index, dir) in self.dirs.iter_mut().enumerate() {
            if dir.watched {
                continue;
            }
            match watcher.watch(&dir.path, RecursiveMode::NonRecursive) {
                Ok(()) => {
                    tracing::info!(path = %dir.path.display(), "watching display directory");
                    dir.watched = true;
                    added.push(index);
                }
                Err(error) => {
                    tracing::debug!(
                        path = %dir.path.display(),
                        %error,
                        "cannot watch display directory yet"
                    );
                }
            }
        }
        added
    }
}

/// Divides watch events into drop file events and display directory changes.
///
/// An event in a display directory is never a drop file event, so it never
/// starts a producer probe and never reaches the drop file processor. Only an
/// upsert of `display.json` is a display change. Each changed directory is
/// given one time.
fn split_events(events: Vec<DropEvent>, display: &DisplayPath) -> (Vec<DropEvent>, Vec<usize>) {
    let mut drop_events = Vec::new();
    let mut changed = Vec::new();
    for event in events {
        let Some(index) = display.dir_index(&event.path) else {
            drop_events.push(event);
            continue;
        };
        let is_display_file = event
            .path
            .file_name()
            .is_some_and(|name| name == DISPLAY_FILE_NAME);
        if is_display_file && event.kind == DropEventKind::Upsert && !changed.contains(&index) {
            changed.push(index);
        }
    }
    (drop_events, changed)
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
    display: &mut DisplayPath,
    publisher: Option<&RelayPublisher>,
    mut producer_state: ProducerState,
    mut pending_missing: HashMap<String, String>,
) -> Result<()> {
    let (sender, receiver) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(sender).context("create filesystem watcher")?;
    watcher
        .watch(watch_dir, RecursiveMode::NonRecursive)
        .with_context(|| format!("watch {}", watch_dir.display()))?;
    // `startup_items` already read each display directory that exists now.
    let _watched_at_startup = display.watch_new_dirs(&mut watcher);
    for dir in display.dirs.iter().filter(|dir| !dir.watched) {
        tracing::warn!(
            path = %dir.path.display(),
            "display directory does not exist yet; the publisher tries again each second"
        );
    }

    let mut last_probe = Instant::now();
    loop {
        // Poll rather than block forever so a relay worker that stopped
        // fatally surfaces within a second, instead of waiting for whenever
        // the next track happens to change.
        let events = match receiver.recv_timeout(next_wakeup()) {
            Ok(Ok(event)) => normalize_notify_event(event),
            Ok(Err(error)) => return Err(error).context("watch directory event error"),
            Err(mpsc::RecvTimeoutError::Timeout) => Vec::new(),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(anyhow!("filesystem watcher stopped"));
            }
        };
        let (drop_events, mut display_changes) = split_events(events, display);

        let now = Instant::now();
        let mut items = Vec::new();
        if probe_due(&drop_events, last_probe, now) {
            let (state, transition_items) = update_producer_state(
                watch_dir,
                processor,
                display,
                publisher,
                &mut pending_missing,
                producer_state,
            )?;
            producer_state = state;
            items.extend(transition_items);
            last_probe = now;
            if let Some(publisher) = publisher {
                publisher.check_health()?;
            }
            for index in display.watch_new_dirs(&mut watcher) {
                if !display_changes.contains(&index) {
                    display_changes.push(index);
                }
            }
        }
        if !drop_events.is_empty() {
            if matches!(producer_state, ProducerState::Missing) {
                tracing::debug!("discarding drop file event while the producer is missing");
            } else {
                for drop_event in drop_events {
                    items.extend(
                        processor
                            .process_event(drop_event, now)?
                            .into_iter()
                            .map(EmitItem::Payload),
                    );
                }
            }
        }
        for index in display_changes {
            items.extend(display.items_for_dir(index));
        }
        emit_items(items, publisher, &mut pending_missing, producer_state)?;
    }
}

/// Gives true when the watch loop must probe the producer lock.
///
/// A drop file event needs a probe first, so a file from a producer that
/// stopped is never published. Other events do not start a probe. The probe
/// opens `.producer.lock`, and the watcher reports that open as an event. A
/// probe on each event then never stops. Without a drop file event, the loop
/// probes when `HEALTH_CHECK_INTERVAL` has passed since the last probe.
fn probe_due(drop_events: &[DropEvent], last_probe: Instant, now: Instant) -> bool {
    !drop_events.is_empty() || now.saturating_duration_since(last_probe) >= HEALTH_CHECK_INTERVAL
}

/// Probes the producer lock and, on a change, gives the items of the
/// transition's block for each affected target (ADR 0005), for the caller to
/// send through `emit_items` at once.
///
/// A change from running to missing gives the dead block for each target and
/// clears the watcher's block identity state. It also gives the display
/// state `null` for each display target, next to the dead block (ADR 0008).
/// It also records each target's dead-block `blockGuid` in `pending_missing`,
/// keyed by `eventGuid`. `emit_items` reads that record: only once the
/// caller sends a target's own dead block does its worker learn the producer
/// is missing and stop its keepalive (relay-lease-task-004). `run_publish` in
/// `src/relay.rs` retries the dead block until the relay accepts it, so a
/// `Producer` command that arrives there while it retries only sets the
/// worker's state; the keepalive thus stops only after the dead block
/// publish succeeds, with no new code here (ADR 0011).
///
/// A change from missing to running gives no item and does not scan the drop
/// directory. The producer takes its lock before it removes a stale drop
/// file, so a scan at that moment could publish the stale track. The producer
/// writes its present track again after it starts, and that event reaches the
/// watch loop as usual. The dead block stays live until then. This change
/// also tells every worker `Producer(Running)` at once, and forgets any
/// `pending_missing` record left over from a dead block that had not yet
/// been sent: a worker must not later be told the producer is missing when it
/// has since returned.
///
/// # Errors
///
/// Returns an error when the probe fails.
fn update_producer_state(
    watch_dir: &Path,
    processor: &mut DropWatcher,
    display: &mut DisplayPath,
    publisher: Option<&RelayPublisher>,
    pending_missing: &mut HashMap<String, String>,
    previous: ProducerState,
) -> Result<(ProducerState, Vec<EmitItem>)> {
    let current = probe_producer(watch_dir)?;
    if current == previous {
        return Ok((current, Vec::new()));
    }

    let mut items = Vec::new();
    match current {
        ProducerState::Missing => {
            tracing::info!(
                producer = %current,
                "producer lock freed; publishing the dead block for each target"
            );
            for payload in processor.producer_missing_payloads() {
                pending_missing.insert(payload.event_guid.clone(), payload.block_guid.clone());
                items.push(EmitItem::Payload(payload));
            }
            items.extend(display.null_items());
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
    Ok((current, items))
}

/// Decides which targets' workers should learn the producer is missing, for
/// one batch of payloads that `emit_items` is about to publish.
///
/// `pending` maps an `eventGuid` to the `blockGuid` `update_producer_state`
/// recorded for that target's dead block on the last missing-producer
/// transition. A payload in this batch that matches a recorded pair is that
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
/// No deadline ever waits: every item leaves through `emit_items` at once
/// (ADR 0011). The watch loop thus always waits for the health check
/// interval.
fn next_wakeup() -> Duration {
    HEALTH_CHECK_INTERVAL
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

/// Publishes each released payload and display state in order and, for a
/// target whose dead block just went out, tells that target's worker the
/// producer is missing.
///
/// See `missing_transitions_for_released` and `update_producer_state` for
/// the rule this follows (ADR 0005, relay-lease-task-004). Each payload is
/// sent to its target's worker before that target's `Producer(Missing)`
/// command: they share one channel, so this order is what the worker
/// receives them in.
///
/// A display state goes to the display worker. That send never blocks and
/// never fails, so a display state never delays a payload (ADR 0008).
fn emit_items(
    items: Vec<EmitItem>,
    publisher: Option<&RelayPublisher>,
    pending_missing: &mut HashMap<String, String>,
    producer_state: ProducerState,
) -> Result<()> {
    let payloads: Vec<LiveValuePayload> = items
        .iter()
        .filter_map(EmitItem::as_payload)
        .cloned()
        .collect();
    let missing_targets =
        missing_transitions_for_released(&payloads, pending_missing, producer_state);
    for item in items {
        match (item, publisher) {
            (EmitItem::Payload(payload), Some(publisher)) => publisher.publish(payload)?,
            (EmitItem::Payload(payload), None) => {
                println!("{}", serde_json::to_string_pretty(&payload)?);
            }
            (EmitItem::Display(entry), Some(publisher)) => publisher.publish_display(entry),
            (EmitItem::Display(entry), None) => tracing::info!(
                event_id = %entry.event_id,
                body = %entry.state.body(),
                "dry run: display state"
            ),
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
    ensure_token_path_is_free(token_file)?;
    let client = RelayClient::new(musicindex_live_publisher::DEFAULT_REQUEST_TIMEOUT)?;
    let item = client.provision(endpoint)?;
    write_token_file(token_file, &item.broadcaster_token).with_context(|| {
        format!(
            "the relay provisioned event_id {}, but the token file {} was not written",
            item.event_id,
            token_file.display()
        )
    })?;

    if json {
        println!("{}", render_provision_json(&item, token_file, target)?);
        return Ok(());
    }

    print_provision_text(&item, token_file, target);
    Ok(())
}

/// Stops `provision` before the relay request when the token path is in use.
///
/// `symlink_metadata` does not follow a symbolic link, so a dangling link also
/// counts as an existing path.
fn ensure_token_path_is_free(token_file: &Path) -> Result<()> {
    match std::fs::symlink_metadata(token_file) {
        Ok(_) => Err(anyhow!(
            "token file {} already exists; provision sent no relay request. \
             Move the old token file to a different path first",
            token_file.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| {
            format!(
                "check token file {}; provision sent no relay request",
                token_file.display()
            )
        }),
    }
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
    fn an_open_of_the_lock_file_does_not_start_a_probe() {
        let lock = PathBuf::from("/drop").join(musicindex_live_publisher::LOCK_FILE_NAME);
        let open = notify::Event::new(EventKind::Access(notify::event::AccessKind::Open(
            notify::event::AccessMode::Any,
        )))
        .add_path(lock);
        let drop_events = normalize_notify_event(open);
        let last_probe = Instant::now();

        assert!(drop_events.is_empty());
        assert!(!probe_due(
            &drop_events,
            last_probe,
            last_probe + Duration::from_millis(10)
        ));
    }

    #[test]
    fn a_drop_file_event_starts_a_probe_at_once() {
        let drop_events = vec![DropEvent {
            kind: DropEventKind::Upsert,
            path: PathBuf::from("/drop/default.json"),
        }];
        let last_probe = Instant::now();

        assert!(probe_due(&drop_events, last_probe, last_probe));
    }

    #[test]
    fn the_probe_runs_after_the_interval_with_no_drop_file_event() {
        let last_probe = Instant::now();

        assert!(!probe_due(
            &[],
            last_probe,
            last_probe + Duration::from_millis(999)
        ));
        assert!(probe_due(
            &[],
            last_probe,
            last_probe + HEALTH_CHECK_INTERVAL
        ));
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

    fn display_path(dir: &Path, event_ids: &[&str]) -> DisplayPath {
        DisplayPath {
            dirs: vec![DisplayDir {
                path: dir.to_path_buf(),
                event_ids: event_ids.iter().map(|id| (*id).to_owned()).collect(),
                watched: true,
            }],
            last: HashMap::new(),
        }
    }

    #[test]
    fn a_display_event_does_not_start_a_producer_probe() {
        let display = display_path(Path::new("/display"), &["event"]);
        let write = notify::Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::To)))
            .add_path(PathBuf::from("/display").join(DISPLAY_FILE_NAME));
        let last_probe = Instant::now();

        let (drop_events, changed) = split_events(normalize_notify_event(write), &display);

        assert!(drop_events.is_empty());
        assert_eq!(changed, vec![0]);
        assert!(!probe_due(
            &drop_events,
            last_probe,
            last_probe + Duration::from_millis(10)
        ));
    }

    #[test]
    fn an_image_event_in_the_display_dir_is_no_change_and_no_drop_event() {
        let display = display_path(Path::new("/display"), &["event"]);
        let image = notify::Event::new(EventKind::Create(CreateKind::File))
            .add_path(PathBuf::from("/display/abc.jpg"));

        let (drop_events, changed) = split_events(normalize_notify_event(image), &display);

        assert!(drop_events.is_empty());
        assert!(changed.is_empty());
    }

    #[test]
    fn a_drop_file_event_stays_a_drop_file_event_with_a_display_path() {
        let display = display_path(Path::new("/display"), &["event"]);
        let drop = notify::Event::new(EventKind::Create(CreateKind::File))
            .add_path(PathBuf::from("/drop/default.json"));

        let (drop_events, changed) = split_events(normalize_notify_event(drop), &display);

        assert_eq!(drop_events.len(), 1);
        assert!(changed.is_empty());
    }

    #[test]
    fn a_producer_that_becomes_missing_gives_the_dead_block_and_the_display_null_at_once()
    -> Result<()> {
        let watch = tempfile::TempDir::new()?;
        let mut processor = DropWatcher::new_targets(
            vec![musicindex_live_publisher::WatchTarget {
                name: "default".to_owned(),
                event_guid: "event".to_owned(),
            }],
            Duration::ZERO,
        );
        let mut display = display_path(Path::new("/display"), &["event"]);
        let mut pending_missing = HashMap::new();

        // No lock file: the probe gives `Missing`. No delay runs before the
        // dead block and the display null come back: this is the only call.
        let (state, items) = update_producer_state(
            watch.path(),
            &mut processor,
            &mut display,
            None,
            &mut pending_missing,
            ProducerState::Running,
        )?;

        assert_eq!(state, ProducerState::Missing);
        assert_eq!(items.len(), 2);
        assert!(matches!(&items[0], EmitItem::Payload(_)));
        assert_eq!(
            items[1],
            EmitItem::Display(DisplayEntry {
                event_id: "event".to_owned(),
                state: DisplayState::null(),
            })
        );
        assert_eq!(pending_missing.len(), 1);
        Ok(())
    }

    #[test]
    fn a_drop_file_change_gives_its_payload_at_once() -> Result<()> {
        let watch = tempfile::TempDir::new()?;
        let path = watch.path().join("default.json");
        std::fs::write(
            &path,
            serde_json::json!({
                "schema": "musicindex.nowplaying/2",
                "target": "default",
                "artist": "Alice",
                "title": "Track One",
                "duration_secs": 187.326,
                "image": null,
                "feed_guid": "feed-guid",
                "track_guid": "track-1",
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
                "value_routes_source": "embedded-id3"
            })
            .to_string(),
        )?;
        let mut processor = DropWatcher::new_targets(
            vec![musicindex_live_publisher::WatchTarget {
                name: "default".to_owned(),
                event_guid: "event-default".to_owned(),
            }],
            Duration::ZERO,
        );

        // One `Instant`, used once: nothing here waits for a second one.
        let now = Instant::now();
        let items: Vec<EmitItem> = processor
            .process_event(
                DropEvent {
                    kind: DropEventKind::Upsert,
                    path,
                },
                now,
            )?
            .into_iter()
            .map(EmitItem::Payload)
            .collect();

        assert_eq!(items.len(), 1);
        let EmitItem::Payload(payload) = &items[0] else {
            panic!("expected a payload item");
        };
        assert_eq!(payload.title, "Track One");
        Ok(())
    }

    #[test]
    fn a_display_json_change_gives_its_state_at_once() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        std::fs::write(
            dir.path().join(DISPLAY_FILE_NAME),
            serde_json::json!({
                "schema": musicindex_live_publisher::DISPLAY_SCHEMA,
                "track": {
                    "artist": "Alice",
                    "title": "Track One",
                    "artwork": null,
                    "song_line": "Alice - Track One",
                    "play_id": "1"
                }
            })
            .to_string(),
        )?;
        let mut display = display_path(dir.path(), &["event"]);

        let items = display.items_for_dir(0);

        assert_eq!(
            items,
            vec![EmitItem::Display(DisplayEntry {
                event_id: "event".to_owned(),
                state: DisplayState {
                    track: Some(musicindex_live_publisher::DisplayTrack {
                        artist: "Alice".to_owned(),
                        title: "Track One".to_owned(),
                        artwork: None,
                        song_line: "Alice - Track One".to_owned(),
                        play_id: Some("1".to_owned()),
                    }),
                },
            })]
        );
        Ok(())
    }

    #[test]
    fn an_unchanged_display_state_is_not_scheduled_again() {
        let mut display = display_path(Path::new("/display"), &["event-a", "event-b"]);

        assert_eq!(display.changed_entries(0, &DisplayState::null()).len(), 2);
        assert!(display.changed_entries(0, &DisplayState::null()).is_empty());
    }

    #[test]
    fn startup_with_a_missing_producer_gives_the_display_state_null() {
        let mut display = display_path(Path::new("/nonexistent/display"), &["event"]);

        let items = display.startup_items(ProducerState::Missing);

        assert_eq!(
            items,
            vec![EmitItem::Display(DisplayEntry {
                event_id: "event".to_owned(),
                state: DisplayState::null(),
            })]
        );
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
            display_dir: None,
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
            display_dir: None,
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
                display_dir: None,
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
            vec![
                "display_dir",
                "event_id",
                "name",
                "stream_delay_secs",
                "token_file"
            ]
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
