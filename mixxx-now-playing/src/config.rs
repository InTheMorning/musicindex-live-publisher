use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use directories::{BaseDirs, ProjectDirs};
use serde::Deserialize;

use mixxx_now_playing::display::same_directory;

use crate::cli::Cli;

const DEFAULT_BASE_URL: &str = "https://api.musicindex.org";
const DEFAULT_TXT_FILE_NAME: &str = "now-playing.txt";
const DEFAULT_ID3_FILE_NAME: &str = "metadata.txt";

#[derive(Debug, Clone)]
pub(crate) struct ResolvedConfig {
    pub(crate) db_file: PathBuf,
    pub(crate) txt_file: PathBuf,
    pub(crate) id3_file: PathBuf,
    pub(crate) v4v_root: PathBuf,
    pub(crate) musicindex_endpoint: String,
    /// The target name. With `--instance`, it is the target of the instance
    /// configuration (ADR 0021). Else it is `--target`.
    pub(crate) target: String,
    /// The display directory of ADR 0008 §The Producer Output. `None` turns
    /// the display output off.
    pub(crate) display_dir: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
struct HititConfig {
    music_dir: Option<String>,
    musicindex_endpoint: Option<String>,
}

#[derive(Debug, Clone)]
struct ResolutionEnv {
    v4v_music_dir: Option<String>,
    home_dir: PathBuf,
    runtime_dir: Option<PathBuf>,
    /// `$XDG_CONFIG_HOME`, or `<home>/.config` when it is not set.
    config_home: PathBuf,
    config_path: PathBuf,
}

impl ResolutionEnv {
    fn current() -> Result<Self> {
        let home_dir = home_dir()?;
        let config_home = env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map_or_else(|| home_dir.join(".config"), PathBuf::from);
        Ok(Self {
            v4v_music_dir: env::var("V4V_MUSIC_DIR").ok(),
            home_dir,
            runtime_dir: env::var_os("XDG_RUNTIME_DIR")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
            config_home,
            config_path: config_path()?,
        })
    }
}

impl ResolvedConfig {
    pub(crate) fn resolve(cli: &Cli) -> Result<Self> {
        Self::resolve_with_env(cli, &ResolutionEnv::current()?)
    }

    fn resolve_with_env(cli: &Cli, env: &ResolutionEnv) -> Result<Self> {
        let file_config = load_hitit_config(&env.config_path);
        let db_file = resolve_path_override(&cli.db_file, &env.home_dir)
            .unwrap_or_else(|| env.home_dir.join(".mixxx").join("mixxxdb.sqlite"));
        let instance = cli
            .instance
            .as_deref()
            .map(|name| InstanceSettings::load(name, env))
            .transpose()?;
        let default_output_dir = default_output_dir(env);
        let (txt_file, id3_file, target) = match instance.as_ref() {
            Some(instance) => (
                instance.drop_dir.join(DEFAULT_TXT_FILE_NAME),
                instance.drop_dir.join(format!("{}.json", instance.target)),
                instance.target.clone(),
            ),
            None => (
                resolve_path_override(&cli.txt_file, &env.home_dir)
                    .unwrap_or_else(|| default_output_dir.join(DEFAULT_TXT_FILE_NAME)),
                resolve_path_override(&cli.id3_file, &env.home_dir)
                    .unwrap_or_else(|| default_output_dir.join(DEFAULT_ID3_FILE_NAME)),
                cli.target.clone(),
            ),
        };
        let v4v_root = resolve_v4v_root(cli, env, file_config.as_ref())?;
        let musicindex_endpoint = file_config
            .as_ref()
            .and_then(|config| config.musicindex_endpoint.as_deref())
            .map(normalize_musicindex_endpoint)
            .transpose()?
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        let display_dir = match instance.as_ref() {
            Some(instance) => instance.display_dir.clone(),
            None => resolve_path_override(&cli.display_dir, &env.home_dir),
        };
        if let Some(display_dir) = display_dir.as_deref() {
            check_display_dir(display_dir, &id3_file)?;
        }

        Ok(Self {
            db_file,
            txt_file,
            id3_file,
            v4v_root,
            musicindex_endpoint,
            target,
            display_dir,
        })
    }
}

/// The values that `--instance` takes from the instance configuration
/// (ADR 0021 §The Producer Reads Its Instance).
#[derive(Debug)]
struct InstanceSettings {
    /// `$XDG_RUNTIME_DIR/musicindex-live-publisher/NAME/nowplaying`.
    drop_dir: PathBuf,
    target: String,
    display_dir: Option<PathBuf>,
}

/// The part of the instance configuration that the producer reads. The
/// producer ignores each other key. It never writes the file.
#[derive(Debug, Deserialize)]
struct InstanceFile {
    #[serde(default, rename = "target")]
    targets: Vec<InstanceTarget>,
}

#[derive(Debug, Deserialize)]
struct InstanceTarget {
    name: String,
    display_dir: Option<PathBuf>,
}

impl InstanceSettings {
    /// Reads the instance configuration of `name`.
    ///
    /// # Errors
    ///
    /// Returns an error when `XDG_RUNTIME_DIR` is not set, when the file is
    /// missing or not valid TOML, when it has not exactly one target, when
    /// the target name is not valid, or when `display_dir` is not the
    /// display directory of the instance. Each error names ADR 0021.
    fn load(name: &str, env: &ResolutionEnv) -> Result<Self> {
        let runtime_dir = env.runtime_dir.as_deref().ok_or_else(|| {
            anyhow!("ADR 0021 §The Producer Reads Its Instance: --instance needs XDG_RUNTIME_DIR")
        })?;
        let config_file = env
            .config_home
            .join("musicindex-live-publisher")
            .join(name)
            .join("config.toml");
        let instance_run = runtime_dir.join("musicindex-live-publisher").join(name);
        let raw = fs::read_to_string(&config_file).map_err(|source| {
            anyhow!(
                "ADR 0021 §The Producer Reads Its Instance: cannot read instance configuration {}: {source}",
                config_file.display()
            )
        })?;
        let file: InstanceFile = toml::from_str(&raw).map_err(|source| {
            anyhow!(
                "ADR 0021 §The Producer Reads Its Instance: cannot parse instance configuration {}: {source}",
                config_file.display()
            )
        })?;
        let mut targets = file.targets;
        if targets.len() != 1 {
            return Err(anyhow!(
                "ADR 0021 §The Producer Reads Its Instance: the instance configuration {} must have exactly one target, but it has {}",
                config_file.display(),
                targets.len()
            ));
        }
        let target = targets.remove(0);
        validate_target_name(&target.name).map_err(|source| {
            anyhow!(
                "ADR 0021 §The Producer Reads Its Instance: target in {}: {source}",
                config_file.display()
            )
        })?;
        let expected_display = instance_run.join("display");
        if let Some(display_dir) = target.display_dir.as_deref()
            && !same_directory(display_dir, &expected_display)
        {
            return Err(anyhow!(
                "ADR 0021 §The Producer Reads Its Instance: display_dir {} in {} must be {}",
                display_dir.display(),
                config_file.display(),
                expected_display.display()
            ));
        }
        Ok(Self {
            drop_dir: instance_run.join("nowplaying"),
            target: target.name,
            // The check above allows only the expected directory.
            display_dir: target.display_dir.map(|_| expected_display),
        })
    }
}

/// Applies the target name rule of the publisher (`src/config.rs`): the name
/// is not empty and has no control character. The producer also refuses a
/// path separator and a dot name, because the name is part of a file name.
fn validate_target_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(anyhow!("target name must not be empty"));
    }
    if name.chars().any(char::is_control) {
        return Err(anyhow!("target name must not contain control characters"));
    }
    // The drop file is `<target>.json` in the drop directory. A path
    // separator or a dot name can put that file outside the drop directory.
    if name.contains(['/', '\\']) || name == "." || name == ".." {
        return Err(anyhow!(
            "target name must not contain a path separator or be a dot name"
        ));
    }
    Ok(())
}

/// Refuses a display directory that is the drop directory. The publisher
/// reads each JSON file in the drop directory as a drop file.
///
/// # Errors
///
/// Returns an error that names ADR 0008 when the two directories are the
/// same.
fn check_display_dir(display_dir: &Path, id3_file: &Path) -> Result<()> {
    let drop_dir = match id3_file.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    if same_directory(display_dir, drop_dir) {
        return Err(anyhow!(
            "--display-dir {} is the drop directory. ADR 0008 §The Producer Output: use a different directory, because the publisher reads each JSON file in the drop directory as a drop file",
            display_dir.display()
        ));
    }
    Ok(())
}

fn default_output_dir(env: &ResolutionEnv) -> PathBuf {
    env.runtime_dir
        .clone()
        .unwrap_or_else(|| env.home_dir.join(".cache"))
        .join("musicindex-live-publisher")
        .join("mixxx")
        .join("nowplaying")
}

fn resolve_path_override(path: &Option<PathBuf>, home_dir: &Path) -> Option<PathBuf> {
    path.as_ref()
        .map(|path| expand_home_path(path, home_dir).unwrap_or_else(|| path.to_path_buf()))
}

fn load_hitit_config(path: &Path) -> Option<HititConfig> {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| toml::from_str::<HititConfig>(&raw).ok())
}

fn resolve_v4v_root(
    cli: &Cli,
    env: &ResolutionEnv,
    file_config: Option<&HititConfig>,
) -> Result<PathBuf> {
    let candidate = cli
        .v4v_root
        .as_ref()
        .map(|path| path.display().to_string())
        .or_else(|| env.v4v_music_dir.clone())
        .or_else(|| file_config.and_then(|config| config.music_dir.clone()))
        .map(|path| expand_home(&path, &env.home_dir))
        .transpose()?
        .unwrap_or_else(|| env.home_dir.join("V4Vmusic"));

    match candidate.canonicalize() {
        Ok(path) => Ok(path),
        Err(source) => {
            eprintln!(
                "warning: could not canonicalize V4V root {}: {source}",
                candidate.display()
            );
            Ok(candidate)
        }
    }
}

/// Returns the HitIt configuration file (ADR 0018). The `v4vmm` file is the
/// fallback until HitIt removes its move code (HitIt ADR 0096 Decision 5).
fn config_path() -> Result<PathBuf> {
    let hitit = app_config_path("hitit")?;
    let v4vmm = app_config_path("v4vmm")?;
    if let Some(path) = select_config_path(&hitit, &v4vmm) {
        return Ok(path);
    }
    eprintln!(
        "warning: no HitIt configuration at {} or {}. ADR 0018: the producer uses the default V4V root and MusicIndex endpoint",
        hitit.display(),
        v4vmm.display()
    );
    Ok(hitit)
}

fn app_config_path(app: &str) -> Result<PathBuf> {
    let proj = ProjectDirs::from("xyz", "HeyCitizen", app)
        .ok_or_else(|| anyhow!("could not determine user config directory"))?;
    Ok(proj.config_dir().join("config.toml"))
}

/// Selects the first configuration file that exists: `hitit`, then `v4vmm`.
fn select_config_path(hitit: &Path, v4vmm: &Path) -> Option<PathBuf> {
    [hitit, v4vmm]
        .into_iter()
        .find(|path| path.is_file())
        .map(Path::to_path_buf)
}

fn home_dir() -> Result<PathBuf> {
    let base_dirs =
        BaseDirs::new().ok_or_else(|| anyhow!("could not determine user home directory"))?;
    Ok(base_dirs.home_dir().to_path_buf())
}

fn expand_home(path: &str, home_dir: &Path) -> Result<PathBuf> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("path is empty"));
    }

    if trimmed == "~" {
        return Ok(home_dir.to_path_buf());
    }

    if let Some(rest) = trimmed.strip_prefix("~/") {
        return Ok(home_dir.join(rest));
    }

    Ok(PathBuf::from(trimmed))
}

fn expand_home_path(path: &Path, home_dir: &Path) -> Option<PathBuf> {
    let path = path.to_str()?;
    expand_home(path, home_dir).ok()
}

fn normalize_musicindex_endpoint(endpoint: &str) -> Result<String> {
    let trimmed = endpoint.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(anyhow!("musicindex_endpoint is empty"));
    }

    let candidate = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    let url = reqwest::Url::parse(&candidate)
        .with_context(|| format!("parse musicindex_endpoint {candidate:?}"))?;
    match url.scheme() {
        "http" | "https" => {}
        scheme => return Err(anyhow!("unsupported musicindex_endpoint scheme: {scheme}")),
    }
    if url.host_str().is_none() {
        return Err(anyhow!("musicindex_endpoint must include a host"));
    }

    Ok(url.as_str().trim_end_matches('/').to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use anyhow::Result;
    use tempfile::TempDir;

    use super::*;

    fn cli_with_v4v_root(path: &Path) -> Cli {
        Cli {
            v4v_root: Some(path.to_path_buf()),
            ..Cli::default()
        }
    }

    fn test_env(temp: &TempDir) -> ResolutionEnv {
        ResolutionEnv {
            v4v_music_dir: None,
            home_dir: temp.path().join("home"),
            runtime_dir: Some(temp.path().join("run")),
            config_home: temp.path().join("config-home"),
            config_path: temp.path().join("config.toml"),
        }
    }

    fn write_config(path: &Path, contents: &str) -> Result<()> {
        fs::write(path, contents).with_context(|| format!("write {}", path.display()))
    }

    #[test]
    fn config_v4v_root_prefers_flag() -> Result<()> {
        let temp = TempDir::new()?;
        let flag_root = temp.path().join("flag-root");
        let env_root = temp.path().join("env-root");
        let config_root = temp.path().join("config-root");
        fs::create_dir_all(&flag_root)?;
        fs::create_dir_all(&env_root)?;
        fs::create_dir_all(&config_root)?;
        let mut env = test_env(&temp);
        env.v4v_music_dir = Some(env_root.display().to_string());
        write_config(
            &env.config_path,
            &format!("music_dir = {:?}\n", config_root.display().to_string()),
        )?;

        let resolved = ResolvedConfig::resolve_with_env(&cli_with_v4v_root(&flag_root), &env)?;

        assert_eq!(resolved.v4v_root, flag_root.canonicalize()?);
        Ok(())
    }

    #[test]
    fn config_v4v_root_prefers_env_over_config() -> Result<()> {
        let temp = TempDir::new()?;
        let env_root = temp.path().join("env-root");
        let config_root = temp.path().join("config-root");
        fs::create_dir_all(&env_root)?;
        fs::create_dir_all(&config_root)?;
        let mut env = test_env(&temp);
        env.v4v_music_dir = Some(env_root.display().to_string());
        write_config(
            &env.config_path,
            &format!("music_dir = {:?}\n", config_root.display().to_string()),
        )?;

        let resolved = ResolvedConfig::resolve_with_env(&Cli::default(), &env)?;

        assert_eq!(resolved.v4v_root, env_root.canonicalize()?);
        Ok(())
    }

    #[test]
    fn config_v4v_root_uses_config_when_no_override() -> Result<()> {
        let temp = TempDir::new()?;
        let config_root = temp.path().join("config-root");
        fs::create_dir_all(&config_root)?;
        let env = test_env(&temp);
        write_config(
            &env.config_path,
            &format!(
                "music_dir = {:?}\nmusicindex_endpoint = \"music.example.test/\"\n",
                config_root.display().to_string()
            ),
        )?;

        let resolved = ResolvedConfig::resolve_with_env(&Cli::default(), &env)?;

        assert_eq!(resolved.v4v_root, config_root.canonicalize()?);
        assert_eq!(resolved.musicindex_endpoint, "https://music.example.test");
        Ok(())
    }

    #[test]
    fn config_v4v_root_uses_default_when_unconfigured() -> Result<()> {
        let temp = TempDir::new()?;
        let env = test_env(&temp);

        let resolved = ResolvedConfig::resolve_with_env(&Cli::default(), &env)?;

        assert_eq!(resolved.v4v_root, env.home_dir.join("V4Vmusic"));
        assert_eq!(resolved.musicindex_endpoint, DEFAULT_BASE_URL);
        Ok(())
    }

    #[test]
    fn config_malformed_toml_falls_back_to_defaults() -> Result<()> {
        let temp = TempDir::new()?;
        let env = test_env(&temp);
        write_config(&env.config_path, "music_dir = [")?;

        let resolved = ResolvedConfig::resolve_with_env(&Cli::default(), &env)?;

        assert_eq!(resolved.v4v_root, env.home_dir.join("V4Vmusic"));
        assert_eq!(resolved.musicindex_endpoint, DEFAULT_BASE_URL);
        Ok(())
    }

    #[test]
    fn config_resolves_runtime_path_defaults_and_home_overrides() -> Result<()> {
        let temp = TempDir::new()?;
        let env = test_env(&temp);
        let cli = Cli {
            db_file: Some(PathBuf::from("~/mixxx.sqlite")),
            txt_file: Some(PathBuf::from("~/now-playing.txt")),
            id3_file: Some(PathBuf::from("~/metadata.txt")),
            ..Cli::default()
        };

        let resolved = ResolvedConfig::resolve_with_env(&cli, &env)?;

        assert_eq!(resolved.db_file, env.home_dir.join("mixxx.sqlite"));
        assert_eq!(resolved.txt_file, env.home_dir.join("now-playing.txt"));
        assert_eq!(resolved.id3_file, env.home_dir.join("metadata.txt"));
        Ok(())
    }

    #[test]
    fn config_uses_runtime_dir_for_default_outputs() -> Result<()> {
        let temp = TempDir::new()?;
        let env = test_env(&temp);

        let resolved = ResolvedConfig::resolve_with_env(&Cli::default(), &env)?;
        let output_dir = default_output_dir(&env);

        assert_eq!(resolved.txt_file, output_dir.join(DEFAULT_TXT_FILE_NAME));
        assert_eq!(resolved.id3_file, output_dir.join(DEFAULT_ID3_FILE_NAME));
        Ok(())
    }

    #[test]
    fn config_uses_home_cache_for_default_outputs_without_runtime_dir() -> Result<()> {
        let temp = TempDir::new()?;
        let mut env = test_env(&temp);
        env.runtime_dir = None;

        let resolved = ResolvedConfig::resolve_with_env(&Cli::default(), &env)?;
        let output_dir = env
            .home_dir
            .join(".cache")
            .join("musicindex-live-publisher")
            .join("mixxx")
            .join("nowplaying");

        assert_eq!(resolved.txt_file, output_dir.join(DEFAULT_TXT_FILE_NAME));
        assert_eq!(resolved.id3_file, output_dir.join(DEFAULT_ID3_FILE_NAME));
        Ok(())
    }

    #[test]
    fn config_refuses_display_dir_equal_to_drop_dir() -> Result<()> {
        let temp = TempDir::new()?;
        let env = test_env(&temp);
        let drop_dir = temp.path().join("drop");
        let cli = Cli {
            id3_file: Some(drop_dir.join("metadata.txt")),
            display_dir: Some(drop_dir.join(".").join("sub").join("..")),
            ..Cli::default()
        };

        let error = ResolvedConfig::resolve_with_env(&cli, &env)
            .expect_err("a display directory equal to the drop directory must fail");

        assert!(error.to_string().contains("ADR 0008"), "{error}");
        Ok(())
    }

    #[test]
    fn config_accepts_a_separate_display_dir() -> Result<()> {
        let temp = TempDir::new()?;
        let env = test_env(&temp);
        let cli = Cli {
            id3_file: Some(temp.path().join("drop").join("metadata.txt")),
            display_dir: Some(temp.path().join("display")),
            ..Cli::default()
        };

        let resolved = ResolvedConfig::resolve_with_env(&cli, &env)?;

        assert_eq!(resolved.display_dir, Some(temp.path().join("display")));
        Ok(())
    }

    fn instance_cli(name: &str) -> Cli {
        Cli {
            instance: Some(name.to_owned()),
            ..Cli::default()
        }
    }

    fn write_instance_config(env: &ResolutionEnv, name: &str, contents: &str) -> Result<PathBuf> {
        let dir = env.config_home.join("musicindex-live-publisher").join(name);
        fs::create_dir_all(&dir)?;
        let path = dir.join("config.toml");
        write_config(&path, contents)?;
        Ok(path)
    }

    fn instance_run(env: &ResolutionEnv, name: &str) -> PathBuf {
        env.runtime_dir
            .clone()
            .expect("the test environment has a runtime directory")
            .join("musicindex-live-publisher")
            .join(name)
    }

    #[test]
    fn instance_resolves_drop_file_and_song_file() -> Result<()> {
        let temp = TempDir::new()?;
        let env = test_env(&temp);
        write_instance_config(
            &env,
            "show",
            "watch_dir = \"ignored\"\nendpoint = \"x\"\n[[target]]\nname = \"show\"\nevent_id = \"e\"\ntoken_file = \"t\"\n",
        )?;

        let resolved = ResolvedConfig::resolve_with_env(&instance_cli("show"), &env)?;

        let drop_dir = instance_run(&env, "show").join("nowplaying");
        assert_eq!(resolved.target, "show");
        assert_eq!(resolved.id3_file, drop_dir.join("show.json"));
        assert_eq!(resolved.txt_file, drop_dir.join("now-playing.txt"));
        Ok(())
    }

    #[test]
    fn instance_display_dir_is_the_instance_display_directory_or_none() -> Result<()> {
        let temp = TempDir::new()?;
        let env = test_env(&temp);
        let display = instance_run(&env, "show").join("display");
        write_instance_config(
            &env,
            "show",
            &format!(
                "[[target]]\nname = \"show\"\ndisplay_dir = {:?}\n",
                display.display().to_string()
            ),
        )?;
        let resolved = ResolvedConfig::resolve_with_env(&instance_cli("show"), &env)?;
        assert_eq!(resolved.display_dir, Some(display));

        write_instance_config(&env, "plain", "[[target]]\nname = \"plain\"\n")?;
        let resolved = ResolvedConfig::resolve_with_env(&instance_cli("plain"), &env)?;
        assert_eq!(resolved.display_dir, None);
        Ok(())
    }

    #[test]
    fn instance_errors_name_adr_0021_and_the_configuration_path() -> Result<()> {
        let temp = TempDir::new()?;
        let env = test_env(&temp);
        let other_display = temp.path().join("elsewhere");
        let cases = [
            ("zero", "watch_dir = \"x\"\n".to_owned()),
            (
                "two",
                "[[target]]\nname = \"a\"\n[[target]]\nname = \"b\"\n".to_owned(),
            ),
            (
                "wrongdisplay",
                format!(
                    "[[target]]\nname = \"a\"\ndisplay_dir = {:?}\n",
                    other_display.display().to_string()
                ),
            ),
            ("badname", "[[target]]\nname = \" \"\n".to_owned()),
            ("slashname", "[[target]]\nname = \"a/b\"\n".to_owned()),
            ("dotname", "[[target]]\nname = \"..\"\n".to_owned()),
            ("badtoml", "[[target".to_owned()),
        ];
        for (name, contents) in cases {
            let path = write_instance_config(&env, name, &contents)?;
            let error = ResolvedConfig::resolve_with_env(&instance_cli(name), &env)
                .expect_err("a bad instance configuration must fail");
            let message = format!("{error:#}");
            assert!(message.contains("ADR 0021"), "{name}: {message}");
            assert!(
                message.contains(&path.display().to_string()),
                "{name}: {message}"
            );
        }

        let missing = ResolvedConfig::resolve_with_env(&instance_cli("missing"), &env)
            .expect_err("a missing configuration file must fail");
        let message = format!("{missing:#}");
        assert!(message.contains("ADR 0021"), "{message}");
        assert!(message.contains("config.toml"), "{message}");
        Ok(())
    }

    #[test]
    fn instance_without_a_runtime_dir_names_adr_0021() -> Result<()> {
        let temp = TempDir::new()?;
        let mut env = test_env(&temp);
        env.runtime_dir = None;

        let error = ResolvedConfig::resolve_with_env(&instance_cli("show"), &env)
            .expect_err("--instance without XDG_RUNTIME_DIR must fail");

        assert!(error.to_string().contains("ADR 0021"), "{error}");
        Ok(())
    }

    #[test]
    fn config_path_prefers_hitit_over_v4vmm() -> Result<()> {
        let temp = TempDir::new()?;
        let hitit = temp.path().join("hitit.toml");
        let v4vmm = temp.path().join("v4vmm.toml");
        write_config(&hitit, "")?;
        write_config(&v4vmm, "")?;

        assert_eq!(select_config_path(&hitit, &v4vmm), Some(hitit));
        Ok(())
    }

    #[test]
    fn config_path_falls_back_to_v4vmm() -> Result<()> {
        let temp = TempDir::new()?;
        let hitit = temp.path().join("hitit.toml");
        let v4vmm = temp.path().join("v4vmm.toml");
        write_config(&v4vmm, "")?;

        assert_eq!(select_config_path(&hitit, &v4vmm), Some(v4vmm));
        Ok(())
    }

    #[test]
    fn config_path_is_none_without_either_file() -> Result<()> {
        let temp = TempDir::new()?;
        let hitit = temp.path().join("hitit.toml");
        let v4vmm = temp.path().join("v4vmm.toml");

        assert_eq!(select_config_path(&hitit, &v4vmm), None);
        Ok(())
    }

    #[test]
    fn config_expands_home_in_paths() -> Result<()> {
        let temp = TempDir::new()?;
        let env = test_env(&temp);
        let expanded = expand_home("~/V4Vmusic", &env.home_dir)?;

        assert_eq!(expanded, env.home_dir.join("V4Vmusic"));
        Ok(())
    }
}
