//! Command-line interface for `station-import`.

use anyhow::{Result, anyhow};
use musicindex_fetch::{DEFAULT_MUSICINDEX_BASE_URL, RSS_HOST_INTERVAL, ReaderConfig};
use station_import::{Options, run};
use std::path::PathBuf;

const USAGE: &str = "usage: station-import --out DIR [--songs URL_OR_PATH] [--episode-feed URL --episode SELECTOR ...] [--dry-run] [--musicindex-url URL]";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (options, musicindex_url) = match parse_args(&args) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error}\n{USAGE}");
            std::process::exit(2);
        }
    };
    let manifest = run(
        &options,
        ReaderConfig {
            min_interval: RSS_HOST_INTERVAL,
            musicindex_base_url: musicindex_url,
            user_agent: format!("station-import/{}", env!("CARGO_PKG_VERSION")),
        },
    )?;
    eprintln!(
        "songs: {}, episodes: {}, errors: {}, unresolved splits: {}",
        manifest.song_count(),
        manifest.episode_count(),
        manifest.error_count(),
        manifest.unresolved_count()
    );
    if manifest.error_count() > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn parse_args(args: &[String]) -> Result<(Options, String)> {
    let mut out = None;
    let mut songs = None;
    let mut episode_feed = None;
    let mut episodes = Vec::new();
    let mut dry_run = false;
    let mut musicindex_url = DEFAULT_MUSICINDEX_BASE_URL.to_owned();
    let mut index = 1;
    while index < args.len() {
        let flag = args[index].as_str();
        match flag {
            "--out" => out = Some(PathBuf::from(next_value(args, &mut index, flag)?)),
            "--songs" => songs = Some(next_value(args, &mut index, flag)?),
            "--episode-feed" => episode_feed = Some(next_value(args, &mut index, flag)?),
            "--episode" => episodes.push(next_value(args, &mut index, flag)?),
            "--musicindex-url" => musicindex_url = next_value(args, &mut index, flag)?,
            "--dry-run" => {
                dry_run = true;
                index += 1;
            }
            "--help" | "-h" => return Err(anyhow!(USAGE)),
            other => return Err(anyhow!("unknown argument {other}")),
        }
    }
    let options = Options {
        songs,
        episode_feed,
        episodes,
        out: out.ok_or_else(|| anyhow!("--out is required"))?,
        dry_run,
    };
    if options.songs.is_none() && options.episode_feed.is_none() {
        return Err(anyhow!("at least one input is required"));
    }
    if options.episode_feed.is_none() && !options.episodes.is_empty() {
        return Err(anyhow!("--episode requires --episode-feed"));
    }
    if options.episode_feed.is_some() && options.episodes.is_empty() {
        return Err(anyhow!("--episode-feed requires --episode"));
    }
    Ok((options, musicindex_url))
}

fn next_value(args: &[String], index: &mut usize, flag: &str) -> Result<String> {
    *index += 1;
    let value = args
        .get(*index)
        .ok_or_else(|| anyhow!("{flag} requires a value"))?
        .clone();
    if value.is_empty() || value.starts_with("--") {
        return Err(anyhow!("{flag} requires a value"));
    }
    *index += 1;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<String> {
        std::iter::once("station-import")
            .chain(values.iter().copied())
            .map(str::to_owned)
            .collect()
    }
    #[test]
    fn parses_episode_number_and_guid_selectors() {
        let parsed = parse_args(&args(&[
            "--out",
            "out",
            "--episode-feed",
            "http://feed",
            "--episode",
            "7",
            "--episode",
            "song-guid",
        ]))
        .expect("arguments parse");
        assert_eq!(parsed.0.episodes, ["7", "song-guid"]);
    }
    #[test]
    fn rejects_usage_errors() {
        for values in [
            &["--out", "out"][..],
            &["--out", "out", "--episode-feed", "http://feed"][..],
            &["--out", "out", "--songs"][..],
            &["--out", "out", "--episode", "7"][..],
        ] {
            assert!(parse_args(&args(values)).is_err());
        }
    }
}
