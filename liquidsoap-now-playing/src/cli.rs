//! Parse the command line for the Liquidsoap producer.

use std::ffi::OsString;
use std::path::PathBuf;

use anyhow::{Result, anyhow};
use now_playing_drop::instance::validate_instance_name;

/// Command line values for the producer.
#[derive(Debug, PartialEq, Eq)]
pub struct Cli {
    /// The instance name.
    pub instance: String,
    /// The Liquidsoap state file path.
    pub state_file: PathBuf,
}

/// Parse producer command line arguments.
pub fn parse<I, S>(args: I) -> Result<Cli>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let mut args = args.into_iter().map(Into::into);
    let _program = args.next();
    let mut instance = None;
    let mut state_file = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--instance") => {
                let name = next_string(&mut args, "--instance")?;
                validate_instance_name(&name)?;
                instance = Some(name);
            }
            Some("--state-file") => {
                state_file = Some(PathBuf::from(next_string(&mut args, "--state-file")?))
            }
            Some(flag) if flag.starts_with("--") => return Err(anyhow!("unknown flag {flag}")),
            Some(value) => return Err(anyhow!("unexpected argument {value}")),
            None => return Err(anyhow!("argument is not valid UTF-8")),
        }
    }
    Ok(Cli {
        instance: instance.ok_or_else(|| anyhow!("--instance is required"))?,
        state_file: state_file.ok_or_else(|| anyhow!("--state-file is required"))?,
    })
}

fn next_string(args: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<String> {
    let value = args
        .next()
        .ok_or_else(|| anyhow!("{flag} requires a value"))?;
    if value.is_empty() {
        return Err(anyhow!("{flag} requires a non-empty value"));
    }
    value
        .into_string()
        .map_err(|_| anyhow!("{flag} value is not valid UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_required_options() -> Result<()> {
        let cli = parse([
            "liquidsoap-now-playing",
            "--instance",
            "test",
            "--state-file",
            "/tmp/state.json",
        ])?;
        assert_eq!(cli.instance, "test");
        assert_eq!(cli.state_file, PathBuf::from("/tmp/state.json"));
        Ok(())
    }

    #[test]
    fn rejects_missing_and_unknown_options() {
        assert!(parse(["liquidsoap-now-playing", "--instance", "test"]).is_err());
        assert!(parse(["liquidsoap-now-playing", "--other"]).is_err());
        // ADR 0021 §The Unit Names: an AzuraCast short name with `_` is not
        // an instance name (ADR 0022 §The Template Unit).
        let error = parse([
            "liquidsoap-now-playing",
            "--instance",
            "heycitizens_test_radio",
            "--state-file",
            "/tmp/state.json",
        ])
        .expect_err("an instance name with `_` must fail");
        assert!(error.to_string().contains("ADR 0021"));
    }
}
