mod cli;
mod config;

use anyhow::{Context, Result};
use mixxx_now_playing::classify::is_v4v_track;
use mixxx_now_playing::expiry::Expiry;
use mixxx_now_playing::history::{HistoryWatcher, TrackRow};
use mixxx_now_playing::musicindex::{
    ResolvedRouteResult, RouteRequestStatus, ValueRouteResolver, ValueRoutesSource,
    apply_resolution_to_tags, result_matches_current,
};
use mixxx_now_playing::render::{
    TrackDisplay, render_metadata_json_with_routes, render_metadata_text_with_routes,
    render_now_playing_line,
};
use mixxx_now_playing::sink::{OutputFile, Presence, remove_file_if_exists};
use mixxx_now_playing::tags::read_tags;
use signal_hook::consts::signal::{SIGINT, SIGTERM};
use signal_hook::iterator::Signals;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

fn main() -> Result<()> {
    let cli = cli::Cli::parse(std::env::args_os())?;
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

fn run(cli: &cli::Cli, config: &config::ResolvedConfig) -> Result<()> {
    let mut runtime = Runtime::new(cli, config)?;

    if cli.once {
        runtime.process_latest(Instant::now())?;
        runtime.wait_for_once_route_resolution()?;
        return Ok(());
    }

    let _cleanup = MetadataCleanup::new(config.id3_file.clone());
    let (terminated, wakeup) = install_signal_flags()?;
    let poll_interval = Duration::from_secs_f64(cli.poll_secs);
    let mut liveness = MixxxLiveness::new(LIVENESS_CHECK_INTERVAL);
    loop {
        let now = Instant::now();
        if terminated.load(Ordering::Relaxed) || !liveness.is_running(now) {
            break;
        }
        runtime.process_latest(now)?;
        sleep_interruptibly(poll_interval, &wakeup);
    }

    runtime.metadata.set(Presence::Absent)?;
    Ok(())
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

#[derive(Debug)]
struct RuntimeState {
    current: Option<CurrentTrack>,
    expiry: Expiry,
    pending_route_lookup: bool,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            current: None,
            expiry: Expiry::none(),
            pending_route_lookup: false,
        }
    }
}

impl RuntimeState {
    fn clear(&mut self) {
        self.current = None;
        self.expiry = Expiry::none();
        self.pending_route_lookup = false;
    }
}

#[derive(Debug)]
struct CurrentTrack {
    hist_id: i64,
    artist: String,
    title: String,
    tags: mixxx_now_playing::tags::TrackTags,
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
}

impl<'a> Runtime<'a> {
    fn new(cli: &'a cli::Cli, config: &'a config::ResolvedConfig) -> Result<Self> {
        ensure_output_parent(&config.txt_file)?;
        ensure_output_parent(&config.id3_file)?;
        let mut now_playing = OutputFile::new(&config.txt_file);
        let mut metadata = OutputFile::new(&config.id3_file);
        now_playing.set(Presence::Absent)?;
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
        })
    }

    fn process_latest(&mut self, now: Instant) -> Result<()> {
        self.expire_current_metadata(now)?;
        self.apply_completed_value_routes()?;

        let Some(row) = self.watcher.poll()? else {
            return Ok(());
        };

        self.process_track(&row, now)
    }

    fn process_track(&mut self, row: &TrackRow, now: Instant) -> Result<()> {
        let line = render_now_playing_line(&row.artist, &row.title, self.cli.strip_hyphens);
        self.now_playing.set(Presence::Present(line))?;

        if !is_v4v_track(&row.path, &self.config.v4v_root) {
            self.metadata.set(Presence::Absent)?;
            self.state.clear();
            return Ok(());
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
                self.metadata.set(Presence::Absent)?;
                self.state.clear();
                return Ok(());
            }
        };
        let content = render_metadata_content(
            self.cli,
            &row.artist,
            &row.title,
            &tags,
            ValueRoutesSource::EmbeddedId3,
        )?;
        self.metadata.set(Presence::Present(content))?;
        self.state.expiry = match self.cli.expiry {
            cli::ExpiryMode::Duration => Expiry::duration(
                now,
                tags.duration,
                self.cli.expiry_slack,
                self.cli.expiry_fallback,
            ),
            cli::ExpiryMode::None => Expiry::none(),
        };
        self.state.current = Some(CurrentTrack {
            hist_id: row.hist_id,
            artist: row.artist.clone(),
            title: row.title.clone(),
            tags,
        });
        if let Some(current) = self.state.current.as_ref() {
            match self.resolver.request(row.hist_id, &current.tags) {
                RouteRequestStatus::Spawned => self.state.pending_route_lookup = true,
                RouteRequestStatus::Cached(result) => self.apply_value_route_result(result)?,
                RouteRequestStatus::Disabled | RouteRequestStatus::NoLookupKey => {}
            }
        }
        Ok(())
    }

    fn expire_current_metadata(&mut self, now: Instant) -> Result<()> {
        if self.state.expiry.expired_at(now) {
            self.metadata.set(Presence::Absent)?;
            self.state.clear();
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

        let updated_tags = apply_resolution_to_tags(&current.tags, &result.resolution);
        let content = render_metadata_content(
            self.cli,
            &current.artist,
            &current.title,
            &updated_tags,
            result.resolution.source,
        )?;
        self.metadata.set(Presence::Present(content))?;
        current.tags = updated_tags;
        Ok(())
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
/// Two mechanisms with distinct jobs. `flag::register` records that a signal
/// arrived, from an async-signal-safe handler. The forwarding thread exists
/// purely so the poll loop's sleep can be cut short: `thread::sleep` restarts
/// itself after `EINTR`, so a signal alone does not shorten it, and sleeping in
/// short slices to compensate meant tens of pointless wake-ups a second.
fn install_signal_flags() -> Result<(Arc<AtomicBool>, mpsc::Receiver<()>)> {
    let terminated = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(SIGTERM, Arc::clone(&terminated))?;
    signal_hook::flag::register(SIGINT, Arc::clone(&terminated))?;

    let (sender, receiver) = mpsc::channel();
    let mut signals = Signals::new([SIGTERM, SIGINT]).context("watch shutdown signals")?;
    thread::Builder::new()
        .name("shutdown-signal".to_owned())
        .spawn(move || {
            for _signal in signals.forever() {
                if sender.send(()).is_err() {
                    break;
                }
            }
        })
        .context("spawn shutdown signal thread")?;

    Ok((terminated, receiver))
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

#[derive(Debug)]
struct MetadataCleanup {
    path: PathBuf,
}

impl MetadataCleanup {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for MetadataCleanup {
    fn drop(&mut self) {
        let _ = remove_file_if_exists(&self.path);
    }
}
