mod cli;
mod config;

use anyhow::{Context, Result, anyhow};
use mixxx_now_playing::classify::is_v4v_track;
use mixxx_now_playing::connector::{
    Action, Coordinator, DeviceEvent, DeviceLocation, DisplayState, DisplayTrack, FADE_NOW,
    Outcome, Row, STATE_REQUEST, open_device, pump, send_command, spawn_reader,
};
use mixxx_now_playing::expiry::Expiry;
use mixxx_now_playing::history::{HistoryWatcher, TrackRow};
use mixxx_now_playing::lock::ProducerLock;
use mixxx_now_playing::musicindex::{
    ResolvedRouteResult, RouteRequestStatus, ValueRouteResolver, ValueRoutesSource,
    apply_resolution_to_tags, result_matches_current,
};
use mixxx_now_playing::render::{
    TrackDisplay, render_metadata_json_with_routes, render_metadata_text_with_routes,
    render_now_playing_line,
};
use mixxx_now_playing::sink::{OutputFile, Presence, ensure_empty_file, remove_file_if_exists};
use mixxx_now_playing::tags::read_tags;
use signal_hook::consts::signal::{SIGINT, SIGTERM};
use signal_hook::iterator::Signals;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    let args: Vec<OsString> = std::env::args_os().collect();
    // ADR 0007: the command needs no producer options, no configuration and
    // no log subscriber. Its exit code is its result.
    if args.get(1).is_some_and(|arg| arg == "command") {
        process::exit(command_main(&args[2..]));
    }

    let cli = cli::Cli::parse(args)?;
    init_tracing()?;
    let config = config::ResolvedConfig::resolve(&cli)?;

    if cli.verbose {
        println!("Mixxx DB: {}", config.db_file.display());
        println!("Now-playing file: {}", config.txt_file.display());
        println!("Metadata file: {}", config.id3_file.display());
        println!("V4V root: {}", config.v4v_root.display());
        println!("MusicIndex endpoint: {}", config.musicindex_endpoint);
        println!("MusicIndex API enabled: {}", !cli.no_api);
    }

    run(&cli, &config)?;

    Ok(())
}

/// The exit code for a command line that is not correct (ADR 0007).
const EXIT_USAGE: i32 = 2;

/// Runs `mixxx-now-playing command` and gives its exit code (ADR 0007 §The
/// Command Line).
///
/// The command writes one line to stderr for each result other than success.
/// It does not take the producer lock, and it does not change a drop file.
fn command_main(args: &[OsString]) -> i32 {
    let command = match cli::CommandCli::parse(args) {
        Ok(command) => command,
        Err(error) => {
            eprintln!(
                "mixxx-now-playing command: {error:#}. {}",
                cli::COMMAND_USAGE
            );
            return EXIT_USAGE;
        }
    };
    // The timeout is one limit for the full command, the device open too.
    let Some(deadline) = Instant::now().checked_add(command.timeout) else {
        eprintln!(
            "mixxx-now-playing command: --timeout is too large. {}",
            cli::COMMAND_USAGE
        );
        return EXIT_USAGE;
    };
    let code = match command.name {
        cli::CommandName::FadeNow => FADE_NOW,
    };
    let outcome = match open_command_device(&command.connector_card) {
        Ok((events, mut writer)) => send_command(&events, &mut writer, code, deadline),
        Err(error) => {
            eprintln!("mixxx-now-playing command: not sent: {error:#}");
            return Outcome::NotSent.exit_code();
        }
    };
    match outcome {
        Outcome::Done => {}
        Outcome::Refused => eprintln!("mixxx-now-playing command: the mapping refused the command"),
        Outcome::NotSent => eprintln!(
            "mixxx-now-playing command: not sent: no heartbeat of the connector mapping arrived"
        ),
        Outcome::Unknown => eprintln!(
            "mixxx-now-playing command: no answer arrived. The command can have run. Check the deck state before you repeat it."
        ),
    }
    outcome.exit_code()
}

/// Opens the raw MIDI device of the connector card for the command.
///
/// One thread reads the device with `pump`. It stays blocked in its read
/// until the process exits, because the process exits at once after the
/// result.
///
/// # Errors
///
/// Returns an error when the card lookup, the open or the thread start fails.
fn open_command_device(card_id: &str) -> Result<(mpsc::Receiver<DeviceEvent>, File)> {
    let (reader, writer) = open_device(&DeviceLocation::system(card_id))?;
    let (sender, receiver) = mpsc::channel();
    thread::Builder::new()
        .name("connector-command".to_owned())
        .spawn(move || {
            // The command needs no wake-up. The receiver of `wake` is gone,
            // and `pump` ignores that.
            let (wake, _) = mpsc::channel();
            let _ = pump(reader, &sender, &wake);
        })
        .context("spawn connector-command thread")?;
    Ok((receiver, writer))
}

/// Starts the log subscriber. It writes to stderr and reads `RUST_LOG`.
///
/// # Errors
///
/// Returns an error when a subscriber is already set.
fn init_tracing() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .try_init()
        .map_err(|error| anyhow!("initialize tracing subscriber: {error}"))
}

fn run(cli: &cli::Cli, config: &config::ResolvedConfig) -> Result<()> {
    if cli.once {
        // `--once` never starts the connector.
        let mut runtime = Runtime::new(cli, config, false)?;
        runtime.process_pass(Instant::now())?;
        runtime.wait_for_once_route_resolution()?;
        return Ok(());
    }

    // `Runtime::new` writes to the drop directory. The producer must hold
    // the lock first. The `_lock` binding keeps the lock alive for the rest
    // of this function.
    let drop_dir = drop_directory(&config.id3_file)?;
    let _lock = ProducerLock::acquire(&drop_dir)
        .with_context(|| format!("acquire producer lock in {}", drop_dir.display()))?;

    let mut runtime = Runtime::new(cli, config, !cli.no_connector)?;

    let _cleanup = ShutdownCleanup::new(config.id3_file.clone(), config.txt_file.clone());
    let (terminated, wakeup, wake_sender) = install_signal_flags()?;
    let connector_events = runtime.start_connector(&wake_sender);
    drop(wake_sender);
    let poll_interval = Duration::from_secs_f64(cli.poll_secs);
    let mut liveness = MixxxLiveness::new(LIVENESS_CHECK_INTERVAL);
    loop {
        if terminated.load(Ordering::Relaxed) || !liveness.is_running(Instant::now()) {
            break;
        }
        if let Some(events) = connector_events.as_ref() {
            runtime.drain_connector_events(events);
        }
        runtime.process_pass(Instant::now())?;
        sleep_interruptibly(poll_interval, &wakeup);
    }

    // ADR 0008 §The Song File For `butt`: the song file holds no text at
    // exit. It is never deleted. This call covers the ordinary exit path.
    // `ShutdownCleanup` covers an early return above it.
    runtime.now_playing.set(Presence::Present(String::new()))?;
    runtime.metadata.set(Presence::Absent)?;
    Ok(())
}

/// Returns the drop directory, the parent directory of the metadata output.
///
/// # Errors
///
/// Returns an error when `id3_file` has no parent directory.
fn drop_directory(id3_file: &Path) -> Result<PathBuf> {
    match id3_file.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => Ok(parent.to_path_buf()),
        Some(_) => Ok(PathBuf::from(".")),
        None => Err(anyhow!(
            "id3 file {} has no parent directory",
            id3_file.display()
        )),
    }
}

fn render_metadata_content(
    cli: &cli::Cli,
    artist: &str,
    title: &str,
    tags: &mixxx_now_playing::tags::TrackTags,
    value_routes_source: ValueRoutesSource,
) -> Result<String> {
    let display = TrackDisplay {
        artist,
        title,
        tags,
    };
    match cli.format {
        cli::OutputFormat::Text => Ok(render_metadata_text_with_routes(
            display,
            value_routes_source,
        )),
        cli::OutputFormat::Json => {
            render_metadata_json_with_routes(display, &cli.target, value_routes_source)
        }
    }
}

#[derive(Debug, Default)]
struct RuntimeState {
    current: Option<CurrentTrack>,
    pending_route_lookup: bool,
}

impl RuntimeState {
    fn clear(&mut self) {
        self.current = None;
        self.pending_route_lookup = false;
    }
}

/// The rendered source of the present track. A resume writes the drop file
/// again from it.
#[derive(Debug)]
struct CurrentTrack {
    hist_id: i64,
    artist: String,
    title: String,
    tags: mixxx_now_playing::tags::TrackTags,
    routes_source: ValueRoutesSource,
}

#[derive(Debug)]
struct Runtime<'a> {
    cli: &'a cli::Cli,
    config: &'a config::ResolvedConfig,
    watcher: HistoryWatcher,
    now_playing: OutputFile,
    metadata: OutputFile,
    state: RuntimeState,
    resolver: ValueRouteResolver,
    coordinator: Coordinator,
    connector_writer: Option<File>,
}

impl<'a> Runtime<'a> {
    fn new(cli: &'a cli::Cli, config: &'a config::ResolvedConfig, connector: bool) -> Result<Self> {
        ensure_output_parent(&config.txt_file)?;
        ensure_output_parent(&config.id3_file)?;
        let mut now_playing = OutputFile::new(&config.txt_file);
        let mut metadata = OutputFile::new(&config.id3_file);
        // ADR 0008 §The Song File For `butt`: the song file holds no text at
        // startup. It is never deleted. The metadata (drop) file keeps its
        // present behavior.
        now_playing.set(Presence::Present(String::new()))?;
        metadata.set(Presence::Absent)?;
        let resolver = ValueRouteResolver::new(
            !cli.no_api,
            &config.musicindex_endpoint,
            cli.api_timeout,
            cli.verbose,
        )?;
        let watcher = HistoryWatcher::open(&config.db_file)?;

        Ok(Self {
            cli,
            config,
            watcher,
            now_playing,
            metadata,
            state: RuntimeState::default(),
            resolver,
            coordinator: Coordinator::new(connector, Instant::now()),
            connector_writer: None,
        })
    }

    /// Starts the connector reader when the connector is on.
    ///
    /// The reader gets a clone of the wake-up sender, so a MIDI message ends
    /// the sleep of the poll loop at once. A reader that cannot start gives
    /// the history-only mode.
    fn start_connector(&mut self, wake: &mpsc::Sender<()>) -> Option<mpsc::Receiver<DeviceEvent>> {
        if !self.coordinator.connector_enabled() {
            return None;
        }
        let (sender, receiver) = mpsc::channel();
        let location = DeviceLocation::system(self.cli.connector_card.clone());
        match spawn_reader(location, sender, wake.clone()) {
            Ok(_handle) => Some(receiver),
            Err(error) => {
                tracing::warn!(error = %format!("{error:#}"), "connector reader did not start");
                self.coordinator.device_unavailable();
                None
            }
        }
    }

    /// Gives each waiting connector event to the `Coordinator`.
    fn drain_connector_events(&mut self, events: &mpsc::Receiver<DeviceEvent>) {
        while let Ok(event) = events.try_recv() {
            match event {
                DeviceEvent::DeviceOpened(writer) => {
                    tracing::debug!(card = %self.cli.connector_card, "connector device opened");
                    self.connector_writer = Some(writer);
                    self.coordinator.device_opened();
                }
                DeviceEvent::ControlChange { cc, time } => {
                    self.coordinator.control_change(cc, time);
                }
                DeviceEvent::DeviceClosed => {
                    tracing::debug!(card = %self.cli.connector_card, "connector device closed");
                    self.connector_writer = None;
                    self.coordinator.device_closed();
                }
                DeviceEvent::DeviceUnavailable(error) => {
                    tracing::debug!(card = %self.cli.connector_card, %error, "connector device unavailable");
                    self.connector_writer = None;
                    self.coordinator.device_unavailable();
                }
            }
        }
    }

    /// Does one pass after the connector events: the mode change, the deck
    /// changes and the expiry, then the API results, then the history poll.
    fn process_pass(&mut self, now: Instant) -> Result<()> {
        let actions = self.coordinator.update(now);
        self.perform(&actions)?;
        self.apply_display_change()?;
        self.apply_completed_value_routes()?;

        if !self.coordinator.history_allowed() {
            return Ok(());
        }
        let Some(row) = self.watcher.poll()? else {
            return Ok(());
        };

        self.process_track(&row, now)
    }

    fn perform(&mut self, actions: &[Action]) -> Result<()> {
        for action in actions {
            match *action {
                Action::SendStateRequest => self.send_state_request(),
                Action::RemoveFile => self.metadata.set(Presence::Absent)?,
                Action::WriteFile { duration } => self.write_current(duration)?,
            }
        }
        Ok(())
    }

    /// Writes the song file when the display state changed (ADR 0008 §The
    /// Song File For `butt`). A track gives its title line. `Null` gives a
    /// file with no text.
    ///
    /// # Errors
    ///
    /// Returns an error when the file write fails.
    fn apply_display_change(&mut self) -> Result<()> {
        let Some(state) = self.coordinator.take_display_change() else {
            return Ok(());
        };
        let line = match state {
            DisplayState::Track { artist, title, .. } => {
                render_now_playing_line(&artist, &title, self.cli.strip_hyphens)
            }
            DisplayState::Null => String::new(),
        };
        self.now_playing.set(Presence::Present(line))
    }

    /// Gives a history row to the `Coordinator`, then does its actions and
    /// writes the song file from the display state.
    ///
    /// # Errors
    ///
    /// Returns an error when a file write fails.
    fn apply_row(&mut self, row: Row, track: &TrackRow) -> Result<()> {
        let display = DisplayTrack {
            artist: track.artist.clone(),
            title: track.title.clone(),
        };
        let actions = self.coordinator.history_row(row, display);
        self.perform(&actions)?;
        self.apply_display_change()
    }

    fn send_state_request(&mut self) {
        let Some(writer) = self.connector_writer.as_mut() else {
            tracing::warn!("no connector device for the state request");
            return;
        };
        if let Err(error) = writer.write_all(&STATE_REQUEST) {
            tracing::warn!(%error, "state request failed");
        }
    }

    /// Writes the drop file for the present track, with `duration` as
    /// `duration_secs`.
    ///
    /// # Errors
    ///
    /// Returns an error when the render or the file write fails.
    fn write_current(&mut self, duration: Option<Duration>) -> Result<()> {
        let Some(current) = self.state.current.as_ref() else {
            return Ok(());
        };
        let tags = mixxx_now_playing::tags::TrackTags {
            duration,
            ..current.tags.clone()
        };
        let content = render_metadata_content(
            self.cli,
            &current.artist,
            &current.title,
            &tags,
            current.routes_source,
        )?;
        self.metadata.set(Presence::Present(content))
    }

    fn process_track(&mut self, row: &TrackRow, now: Instant) -> Result<()> {
        if !is_v4v_track(&row.path, &self.config.v4v_root) {
            self.state.clear();
            return self.apply_row(Row::Other, row);
        }

        let tags = match read_tags(&row.path) {
            Ok(tags) => tags,
            Err(error) => {
                if self.cli.verbose {
                    eprintln!(
                        "warning: could not read tags from {}: {error:#}",
                        row.path.display()
                    );
                }
                self.state.clear();
                return self.apply_row(Row::Other, row);
            }
        };
        // The expiry keeps its source in both modes. The `Coordinator`
        // enforces it only in the history-only mode.
        let expiry = match self.cli.expiry {
            cli::ExpiryMode::Duration => Expiry::duration(
                now,
                tags.duration,
                self.cli.expiry_slack,
                self.cli.expiry_max,
            ),
            cli::ExpiryMode::None => Expiry::none(),
        };
        let header_duration = tags.duration;
        self.state.current = Some(CurrentTrack {
            hist_id: row.hist_id,
            artist: row.artist.clone(),
            title: row.title.clone(),
            tags,
            routes_source: ValueRoutesSource::EmbeddedId3,
        });
        self.apply_row(
            Row::V4v {
                header_duration,
                expiry,
            },
            row,
        )?;
        if let Some(current) = self.state.current.as_ref() {
            match self.resolver.request(row.hist_id, &current.tags) {
                RouteRequestStatus::Spawned => self.state.pending_route_lookup = true,
                RouteRequestStatus::Cached(result) => self.apply_value_route_result(result)?,
                RouteRequestStatus::Disabled | RouteRequestStatus::NoLookupKey => {}
            }
        }
        Ok(())
    }

    fn apply_completed_value_routes(&mut self) -> Result<()> {
        for result in self.resolver.drain() {
            self.apply_value_route_result(result)?;
        }
        Ok(())
    }

    fn wait_for_once_route_resolution(&mut self) -> Result<()> {
        if !self.state.pending_route_lookup {
            return Ok(());
        }

        let deadline = Instant::now()
            .checked_add(self.cli.api_timeout)
            .unwrap_or_else(Instant::now);
        while Instant::now() < deadline {
            let results = self.resolver.drain();
            if !results.is_empty() {
                for result in results {
                    self.apply_value_route_result(result)?;
                }
                return Ok(());
            }
            thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }

    fn apply_value_route_result(&mut self, result: ResolvedRouteResult) -> Result<()> {
        let Some(current) = self.state.current.as_mut() else {
            return Ok(());
        };
        if !result_matches_current(&result, current.hist_id) {
            return Ok(());
        }
        self.state.pending_route_lookup = false;
        if result.resolution.source != ValueRoutesSource::MusicIndexApi {
            return Ok(());
        }

        // Keep the result, so a resume writes it. The `Coordinator` allows
        // the write only while the drop file is present.
        current.tags = apply_resolution_to_tags(&current.tags, &result.resolution);
        current.routes_source = result.resolution.source;
        match self.coordinator.api_result() {
            Some(action) => self.perform(&[action]),
            None => Ok(()),
        }
    }
}

fn ensure_output_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    Ok(())
}

/// Installs shutdown handling and returns the flag plus a wake-up channel.
///
/// The returned sender lets another thread end the sleep of the poll loop.
///
/// Two mechanisms with distinct jobs. `flag::register` records that a signal
/// arrived, from an async-signal-safe handler. The forwarding thread exists
/// purely so the poll loop's sleep can be cut short: `thread::sleep` restarts
/// itself after `EINTR`, so a signal alone does not shorten it, and sleeping in
/// short slices to compensate meant tens of pointless wake-ups a second.
fn install_signal_flags() -> Result<(Arc<AtomicBool>, mpsc::Receiver<()>, mpsc::Sender<()>)> {
    let terminated = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(SIGTERM, Arc::clone(&terminated))?;
    signal_hook::flag::register(SIGINT, Arc::clone(&terminated))?;

    let (sender, receiver) = mpsc::channel();
    let signal_sender = sender.clone();
    let mut signals = Signals::new([SIGTERM, SIGINT]).context("watch shutdown signals")?;
    thread::Builder::new()
        .name("shutdown-signal".to_owned())
        .spawn(move || {
            for _signal in signals.forever() {
                if signal_sender.send(()).is_err() {
                    break;
                }
            }
        })
        .context("spawn shutdown signal thread")?;

    Ok((terminated, receiver, sender))
}

/// How long a liveness result stays good before /proc is scanned again.
///
/// The poll interval governs how quickly a *track* change is noticed; noticing
/// that Mixxx itself exited a fraction of a second later costs nothing, and the
/// scan is by far the most expensive thing in an otherwise idle loop.
const LIVENESS_CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// Caches the answer to "is Mixxx still running" for a short interval.
#[derive(Debug)]
struct MixxxLiveness {
    interval: Duration,
    last_checked: Option<Instant>,
    last_result: bool,
}

impl MixxxLiveness {
    fn new(interval: Duration) -> Self {
        Self {
            interval,
            last_checked: None,
            last_result: true,
        }
    }

    fn is_running(&mut self, now: Instant) -> bool {
        let due = match self.last_checked {
            Some(last) => now.duration_since(last) >= self.interval,
            None => true,
        };
        if due {
            self.last_result = mixxx_process_running();
            self.last_checked = Some(now);
        }
        self.last_result
    }
}

/// Reports whether any process other than this one looks like Mixxx.
///
/// Reads `/proc` directly rather than shelling out to `pgrep`. The matching
/// rule is deliberately the same as the bash script's `pgrep -i mixxx`: a
/// case-insensitive substring match against the process name. Spawning `pgrep`
/// twice a second cost more than everything else in the loop combined.
fn mixxx_process_running() -> bool {
    let Ok(entries) = fs::read_dir("/proc") else {
        // If /proc cannot be read we cannot prove Mixxx is gone, and exiting
        // would tear down the now-playing files under a live show.
        return true;
    };

    let current_pid = process::id();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|name| name.parse::<u32>().ok()) else {
            continue;
        };
        if pid == current_pid {
            continue;
        }
        let Ok(comm) = fs::read_to_string(entry.path().join("comm")) else {
            continue;
        };
        if comm.trim().to_ascii_lowercase().contains("mixxx") {
            return true;
        }
    }
    false
}

/// Waits out one poll interval, returning immediately once a signal arrives.
///
/// One blocking wait per poll rather than a slice loop, and shutdown is prompt
/// because the signal thread sends on this channel rather than relying on the
/// sleep being interrupted.
fn sleep_interruptibly(duration: Duration, wakeup: &mpsc::Receiver<()>) {
    match wakeup.recv_timeout(duration) {
        // Signalled, or the sender is gone: either way stop waiting. The caller
        // re-checks the shutdown flag.
        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {}
        Err(mpsc::RecvTimeoutError::Timeout) => {}
    }
}

/// Guards the metadata (drop) file and the song file at every exit from the
/// main loop. An exit can be a normal end, a signal, or an error that `?`
/// propagates before `run` reaches its own cleanup lines.
///
/// `drop` removes the metadata file. That behavior does not change. `drop`
/// also writes the song file with no text, unless the file already holds no
/// text (AGENTS.md §6). See ADR 0008 §The Song File For `butt`.
#[derive(Debug)]
struct ShutdownCleanup {
    metadata_path: PathBuf,
    song_path: PathBuf,
}

impl ShutdownCleanup {
    fn new(metadata_path: PathBuf, song_path: PathBuf) -> Self {
        Self {
            metadata_path,
            song_path,
        }
    }
}

impl Drop for ShutdownCleanup {
    fn drop(&mut self) {
        let _ = remove_file_if_exists(&self.metadata_path);
        let _ = ensure_empty_file(&self.song_path);
    }
}
