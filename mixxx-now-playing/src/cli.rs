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
    pub(crate) expiry_max: Duration,
    pub(crate) no_api: bool,
    pub(crate) api_timeout: Duration,
    pub(crate) strip_hyphens: bool,
    pub(crate) verbose: bool,
    pub(crate) connector_card: String,
    pub(crate) no_connector: bool,
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
            expiry_max: Duration::from_secs(600),
            no_api: false,
            api_timeout: Duration::from_secs(5),
            strip_hyphens: true,
            verbose: false,
            connector_card: "V4V".to_owned(),
            no_connector: false,
        }
    }
}

/// The usage line of the `command` subcommand (ADR 0007 §The Command Line).
pub(crate) const COMMAND_USAGE: &str =
    "usage: mixxx-now-playing command fade-now [--connector-card ID] [--timeout SECS]";

/// A command to Mixxx.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandName {
    /// AutoDJ fade now.
    FadeNow,
}

/// The options of `mixxx-now-playing command`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CommandCli {
    pub(crate) name: CommandName,
    pub(crate) connector_card: String,
    pub(crate) timeout: Duration,
}

impl CommandCli {
    /// Parses the arguments that follow `command`.
    ///
    /// # Errors
    ///
    /// Returns an error when the command name is missing or unknown, when an
    /// option is unknown or has no value, or when `--timeout` is not a
    /// positive number of seconds.
    pub(crate) fn parse<I, S>(args: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        let mut args = args.into_iter().map(Into::into);
        let name = match args.next() {
            None => return Err(anyhow!("no command name")),
            Some(name) => match name.to_str() {
                Some("fade-now") => CommandName::FadeNow,
                _ => {
                    return Err(anyhow!("unknown command {}", name.to_string_lossy()));
                }
            },
        };
        let mut command = Self {
            name,
            connector_card: "V4V".to_owned(),
            timeout: Duration::from_secs(2),
        };
        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("--connector-card") => {
                    command.connector_card = next_string(&mut args, "--connector-card")?;
                }
                Some("--timeout") => {
                    let secs = next_f64(&mut args, "--timeout")?;
                    command.timeout = Duration::try_from_secs_f64(secs)
                        .map_err(|source| anyhow!("parse --timeout value {secs:?}: {source}"))?;
                }
                _ => return Err(anyhow!("unexpected argument {}", arg.to_string_lossy())),
            }
        }
        Ok(command)
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
                Some("--expiry-max") => {
                    cli.expiry_max = next_duration(&mut args, "--expiry-max")?;
                }
                Some("--expiry-fallback") => {
                    return Err(anyhow!(
                        "--expiry-fallback was removed; use --expiry-max instead"
                    ));
                }
                Some("--no-api") => cli.no_api = true,
                Some("--api-timeout") => {
                    cli.api_timeout = next_duration(&mut args, "--api-timeout")?;
                }
                Some("--strip-hyphens") => cli.strip_hyphens = true,
                Some("--no-strip-hyphens") => cli.strip_hyphens = false,
                Some("--verbose") => cli.verbose = true,
                Some("--connector-card") => {
                    cli.connector_card = next_string(&mut args, "--connector-card")?;
                }
                Some("--no-connector") => cli.no_connector = true,
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn expiry_max_sets_the_expiry_max_field() -> Result<()> {
        let cli = Cli::parse(["mixxx-now-playing", "--expiry-max", "120"])?;

        assert_eq!(cli.expiry_max, Duration::from_secs(120));
        Ok(())
    }

    #[test]
    fn expiry_fallback_is_rejected_and_names_expiry_max() {
        let error = Cli::parse(["mixxx-now-playing", "--expiry-fallback", "120"])
            .expect_err("--expiry-fallback must fail");

        let message = error.to_string();
        assert!(
            message.contains("--expiry-max"),
            "error message {message:?} does not name --expiry-max"
        );
    }

    #[test]
    fn connector_defaults_to_the_v4v_card() -> Result<()> {
        let cli = Cli::parse(["mixxx-now-playing"])?;

        assert_eq!(cli.connector_card, "V4V");
        assert!(!cli.no_connector);
        Ok(())
    }

    #[test]
    fn connector_card_sets_the_card_id() -> Result<()> {
        let cli = Cli::parse(["mixxx-now-playing", "--connector-card", "Other"])?;

        assert_eq!(cli.connector_card, "Other");
        Ok(())
    }

    #[test]
    fn connector_card_requires_a_value() {
        assert!(Cli::parse(["mixxx-now-playing", "--connector-card"]).is_err());
        assert!(Cli::parse(["mixxx-now-playing", "--connector-card", ""]).is_err());
    }

    #[test]
    fn command_defaults() -> Result<()> {
        let command = CommandCli::parse(["fade-now"])?;

        assert_eq!(command.name, CommandName::FadeNow);
        assert_eq!(command.connector_card, "V4V");
        assert_eq!(command.timeout, Duration::from_secs(2));
        Ok(())
    }

    #[test]
    fn command_options() -> Result<()> {
        let command =
            CommandCli::parse(["fade-now", "--timeout", "0.5", "--connector-card", "Other"])?;

        assert_eq!(command.connector_card, "Other");
        assert_eq!(command.timeout, Duration::from_millis(500));
        Ok(())
    }

    #[test]
    fn command_errors() {
        let cases: [&[&str]; 9] = [
            &[],
            &["skip"],
            &["--timeout", "1"],
            &["fade-now", "--timeout"],
            &["fade-now", "--timeout", "0"],
            &["fade-now", "--timeout", "-1"],
            &["fade-now", "--timeout", "abc"],
            &["fade-now", "--timeout", "inf"],
            &["fade-now", "extra"],
        ];
        for args in cases {
            assert!(CommandCli::parse(args).is_err(), "{args:?} must fail");
        }
    }

    #[test]
    fn no_connector_turns_the_connector_off() -> Result<()> {
        let cli = Cli::parse(["mixxx-now-playing", "--no-connector"])?;

        assert!(cli.no_connector);
        Ok(())
    }
}
