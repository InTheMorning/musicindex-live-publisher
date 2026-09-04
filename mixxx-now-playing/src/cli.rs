use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Result, anyhow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExpiryMode {
    Duration,
    None,
}

#[derive(Debug, Clone)]
pub(crate) struct Cli {
    pub(crate) db_file: Option<PathBuf>,
    pub(crate) txt_file: Option<PathBuf>,
    pub(crate) id3_file: Option<PathBuf>,
    pub(crate) v4v_root: Option<PathBuf>,
    pub(crate) poll_secs: f64,
    pub(crate) once: bool,
    pub(crate) format: OutputFormat,
    pub(crate) target: String,
    pub(crate) expiry: ExpiryMode,
    pub(crate) expiry_slack: Duration,
    pub(crate) expiry_fallback: Duration,
    pub(crate) no_api: bool,
    pub(crate) api_timeout: Duration,
    pub(crate) strip_hyphens: bool,
    pub(crate) verbose: bool,
}

impl Default for Cli {
    fn default() -> Self {
        Self {
            db_file: None,
            txt_file: None,
            id3_file: None,
            v4v_root: None,
            poll_secs: 0.5,
            once: false,
            format: OutputFormat::Text,
            target: "default".to_owned(),
            expiry: ExpiryMode::Duration,
            expiry_slack: Duration::from_secs(5),
            expiry_fallback: Duration::from_secs(600),
            no_api: false,
            api_timeout: Duration::from_secs(5),
            strip_hyphens: true,
            verbose: false,
        }
    }
}

impl Cli {
    pub(crate) fn parse<I, S>(args: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        let mut cli = Self::default();
        let mut args = args.into_iter().map(Into::into);
        let _program = args.next();

        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("--db-file") => cli.db_file = Some(next_path(&mut args, "--db-file")?),
                Some("--txt-file") => cli.txt_file = Some(next_path(&mut args, "--txt-file")?),
                Some("--id3-file") => cli.id3_file = Some(next_path(&mut args, "--id3-file")?),
                Some("--v4v-root") => cli.v4v_root = Some(next_path(&mut args, "--v4v-root")?),
                Some("--poll-secs") => cli.poll_secs = next_f64(&mut args, "--poll-secs")?,
                Some("--once") => cli.once = true,
                Some("--format") => cli.format = next_format(&mut args)?,
                Some("--target") => cli.target = next_string(&mut args, "--target")?,
                Some("--expiry") => cli.expiry = next_expiry_mode(&mut args)?,
                Some("--expiry-slack") => {
                    cli.expiry_slack = next_duration(&mut args, "--expiry-slack")?;
                }
                Some("--expiry-fallback") => {
                    cli.expiry_fallback = next_duration(&mut args, "--expiry-fallback")?;
                }
                Some("--no-api") => cli.no_api = true,
                Some("--api-timeout") => {
                    cli.api_timeout = next_duration(&mut args, "--api-timeout")?;
                }
                Some("--strip-hyphens") => cli.strip_hyphens = true,
                Some("--no-strip-hyphens") => cli.strip_hyphens = false,
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

fn next_value(args: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<OsString> {
    args.next()
        .ok_or_else(|| anyhow!("{flag} requires a value"))
        .and_then(|value| {
            if value == OsStr::new("") {
                Err(anyhow!("{flag} requires a non-empty value"))
            } else {
                Ok(value)
            }
        })
}

fn next_path(args: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<PathBuf> {
    Ok(PathBuf::from(next_value(args, flag)?))
}

fn next_string(args: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<String> {
    next_value(args, flag)?.into_string().map_err(|value| {
        anyhow!(
            "{flag} value is not valid UTF-8: {}",
            value.to_string_lossy()
        )
    })
}

fn next_f64(args: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<f64> {
    let value = next_value(args, flag)?;
    let Some(value) = value.to_str() else {
        return Err(anyhow!("{flag} value is not valid UTF-8"));
    };
    let parsed = value
        .parse::<f64>()
        .map_err(|source| anyhow!("parse {flag} value {value:?}: {source}"))?;
    if parsed <= 0.0 {
        return Err(anyhow!("{flag} must be greater than zero"));
    }
    Ok(parsed)
}

fn next_duration(args: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<Duration> {
    let value = next_value(args, flag)?;
    let Some(value) = value.to_str() else {
        return Err(anyhow!("{flag} value is not valid UTF-8"));
    };
    let secs = value
        .parse::<f64>()
        .map_err(|source| anyhow!("parse {flag} value {value:?}: {source}"))?;
    if secs < 0.0 {
        return Err(anyhow!("{flag} must be greater than or equal to zero"));
    }
    Duration::try_from_secs_f64(secs)
        .map_err(|source| anyhow!("parse {flag} value {secs:?}: {source}"))
}

fn next_format(args: &mut impl Iterator<Item = OsString>) -> Result<OutputFormat> {
    let value = next_value(args, "--format")?;
    match value.to_str() {
        Some("text") => Ok(OutputFormat::Text),
        Some("json") => Ok(OutputFormat::Json),
        Some(other) => Err(anyhow!("unsupported --format {other:?}; use text or json")),
        None => Err(anyhow!("--format value is not valid UTF-8")),
    }
}

fn next_expiry_mode(args: &mut impl Iterator<Item = OsString>) -> Result<ExpiryMode> {
    let value = next_value(args, "--expiry")?;
    match value.to_str() {
        Some("duration") => Ok(ExpiryMode::Duration),
        Some("none") => Ok(ExpiryMode::None),
        Some(other) => Err(anyhow!(
            "unsupported --expiry {other:?}; use duration or none"
        )),
        None => Err(anyhow!("--expiry value is not valid UTF-8")),
    }
}
