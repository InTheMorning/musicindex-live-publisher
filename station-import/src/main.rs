//! Command-line interface for `station-import`.

use anyhow::{Result, anyhow};
use musicindex_convert::{DEFAULT_MP3_BIT_RATE_KBPS, MP3_BIT_RATE_RANGE_KBPS};
use musicindex_fetch::{DEFAULT_MUSICINDEX_BASE_URL, RSS_HOST_INTERVAL, ReaderConfig};
use station_import::{Options, OutputFormat, run};
use std::path::PathBuf;

const USAGE: &str = "usage: station-import --out DIR [--songs URL_OR_PATH] [--episode-feed URL --episode SELECTOR ...] [--format keep|mp3] [--mp3-bitrate KBPS] [--dry-run] [--musicindex-url URL]";

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
    let mut format = None;
    let mut bit_rate = None;
    let mut index = 1;
    while index < args.len() {
        let flag = args[index].as_str();
        match flag {
            "--out" => out = Some(PathBuf::from(next_value(args, &mut index, flag)?)),
            "--songs" => songs = Some(next_value(args, &mut index, flag)?),
            "--episode-feed" => episode_feed = Some(next_value(args, &mut index, flag)?),
            "--episode" => episodes.push(next_value(args, &mut index, flag)?),
            "--musicindex-url" => musicindex_url = next_value(args, &mut index, flag)?,
            "--format" => format = Some(next_value(args, &mut index, flag)?),
            "--mp3-bitrate" => bit_rate = Some(next_value(args, &mut index, flag)?),
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
        format: output_format(format.as_deref(), bit_rate.as_deref())?,
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

/// The output format of `--format` and `--mp3-bitrate` (musicindex ADR 0095
/// R-MP3-1 and R-MP3-3 item 6). The default is `keep`. `--mp3-bitrate` is
/// valid only with `--format mp3`.
fn output_format(format: Option<&str>, bit_rate: Option<&str>) -> Result<OutputFormat> {
    match (format.unwrap_or("keep"), bit_rate) {
        ("keep", None) => Ok(OutputFormat::Keep),
        ("keep", Some(_)) => Err(anyhow!("--mp3-bitrate requires --format mp3")),
        ("mp3", bit_rate) => {
            let bit_rate_kbps = match bit_rate {
                None => DEFAULT_MP3_BIT_RATE_KBPS,
                Some(value) => value
                    .parse()
                    .ok()
                    .filter(|kbps| MP3_BIT_RATE_RANGE_KBPS.contains(kbps))
                    .ok_or_else(|| {
                        anyhow!(
                            "--mp3-bitrate must be a number from {} to {}",
                            MP3_BIT_RATE_RANGE_KBPS.start(),
                            MP3_BIT_RATE_RANGE_KBPS.end()
                        )
                    })?,
            };
            Ok(OutputFormat::Mp3 { bit_rate_kbps })
        }
        (other, _) => Err(anyhow!("--format must be keep or mp3, not {other}")),
    }
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
            &["--out", "out", "--songs", "s", "--mp3-bitrate", "192"][..],
            &[
                "--out",
                "out",
                "--songs",
                "s",
                "--format",
                "keep",
                "--mp3-bitrate",
                "192",
            ][..],
            &[
                "--out",
                "out",
                "--songs",
                "s",
                "--format",
                "mp3",
                "--mp3-bitrate",
                "31",
            ][..],
            &[
                "--out",
                "out",
                "--songs",
                "s",
                "--format",
                "mp3",
                "--mp3-bitrate",
                "321",
            ][..],
            &[
                "--out",
                "out",
                "--songs",
                "s",
                "--format",
                "mp3",
                "--mp3-bitrate",
                "x",
            ][..],
            &["--out", "out", "--songs", "s", "--format", "ogg"][..],
        ] {
            assert!(parse_args(&args(values)).is_err());
        }
    }
    // musicindex ADR 0095 R-MP3-1 item 2.
    #[test]
    fn format_keep_is_the_default() {
        let parsed = parse_args(&args(&["--out", "out", "--songs", "s"])).expect("arguments parse");
        assert_eq!(parsed.0.format, OutputFormat::Keep);
        let parsed = parse_args(&args(&["--out", "out", "--songs", "s", "--format", "keep"]))
            .expect("arguments parse");
        assert_eq!(parsed.0.format, OutputFormat::Keep);
    }
    // musicindex ADR 0095 R-MP3-3 item 6.
    #[test]
    fn mp3_format_has_256_kbps_else_the_flag_value() {
        let parsed = parse_args(&args(&["--out", "out", "--songs", "s", "--format", "mp3"]))
            .expect("arguments parse");
        assert_eq!(parsed.0.format, OutputFormat::Mp3 { bit_rate_kbps: 256 });
        let parsed = parse_args(&args(&[
            "--out",
            "out",
            "--songs",
            "s",
            "--format",
            "mp3",
            "--mp3-bitrate",
            "192",
        ]))
        .expect("arguments parse");
        assert_eq!(parsed.0.format, OutputFormat::Mp3 { bit_rate_kbps: 192 });
    }
}
