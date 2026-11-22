mod cli;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use directories::BaseDirs;
use liquidsoap_now_playing::Producer;
use now_playing_drop::InstanceSettings;
use signal_hook::consts::signal::{SIGINT, SIGTERM};
use signal_hook::flag;
use tracing_subscriber::EnvFilter;

const HELP: &str = "usage: liquidsoap-now-playing --instance NAME --state-file PATH";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{HELP}");
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("liquidsoap-now-playing {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .try_init()
        .map_err(|error| anyhow::anyhow!("initialize tracing subscriber: {error}"))?;
    let cli = cli::parse(args)?;
    let dirs = BaseDirs::new().context("find user configuration directory")?;
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    let settings = InstanceSettings::load(&cli.instance, dirs.config_dir(), runtime.as_deref())?;
    let mut producer = Producer::new(cli.state_file, settings);
    let terminated = Arc::new(AtomicBool::new(false));
    flag::register(SIGTERM, Arc::clone(&terminated))?;
    flag::register(SIGINT, Arc::clone(&terminated))?;
    while !terminated.load(Ordering::Relaxed) {
        producer.poll(Instant::now())?;
        for _ in 0..50 {
            if terminated.load(Ordering::Relaxed) {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    producer.shutdown()
}
