use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use directories::{BaseDirs, ProjectDirs};
use serde::Deserialize;

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
}

#[derive(Debug, Default, Deserialize)]
struct V4vmmConfig {
    music_dir: Option<String>,
    musicindex_endpoint: Option<String>,
}

#[derive(Debug, Clone)]
struct ResolutionEnv {
    v4v_music_dir: Option<String>,
    home_dir: PathBuf,
    runtime_dir: Option<PathBuf>,
    config_path: PathBuf,
}

impl ResolutionEnv {
    fn current() -> Result<Self> {
        Ok(Self {
            v4v_music_dir: env::var("V4V_MUSIC_DIR").ok(),
            home_dir: home_dir()?,
            runtime_dir: env::var_os("XDG_RUNTIME_DIR")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
            config_path: config_path()?,
        })
    }
}

impl ResolvedConfig {
    pub(crate) fn resolve(cli: &Cli) -> Result<Self> {
        Self::resolve_with_env(cli, &ResolutionEnv::current()?)
    }

    fn resolve_with_env(cli: &Cli, env: &ResolutionEnv) -> Result<Self> {
        let file_config = load_v4vmm_config(&env.config_path);
        let db_file = resolve_path_override(&cli.db_file, &env.home_dir)
            .unwrap_or_else(|| env.home_dir.join(".mixxx").join("mixxxdb.sqlite"));
        let default_output_dir = default_output_dir(env);
        let txt_file = resolve_path_override(&cli.txt_file, &env.home_dir)
            .unwrap_or_else(|| default_output_dir.join(DEFAULT_TXT_FILE_NAME));
        let id3_file = resolve_path_override(&cli.id3_file, &env.home_dir)
            .unwrap_or_else(|| default_output_dir.join(DEFAULT_ID3_FILE_NAME));
        let v4v_root = resolve_v4v_root(cli, env, file_config.as_ref())?;
        let musicindex_endpoint = file_config
            .as_ref()
            .and_then(|config| config.musicindex_endpoint.as_deref())
            .map(normalize_musicindex_endpoint)
            .transpose()?
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

        Ok(Self {
            db_file,
            txt_file,
            id3_file,
            v4v_root,
            musicindex_endpoint,
        })
    }
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

fn load_v4vmm_config(path: &Path) -> Option<V4vmmConfig> {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| toml::from_str::<V4vmmConfig>(&raw).ok())
}

fn resolve_v4v_root(
    cli: &Cli,
    env: &ResolutionEnv,
    file_config: Option<&V4vmmConfig>,
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

fn config_path() -> Result<PathBuf> {
    let proj = ProjectDirs::from("xyz", "HeyCitizen", "v4vmm")
        .ok_or_else(|| anyhow!("could not determine user config directory"))?;
    Ok(proj.config_dir().join("config.toml"))
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
    fn config_expands_home_in_paths() -> Result<()> {
        let temp = TempDir::new()?;
        let env = test_env(&temp);
        let expanded = expand_home("~/V4Vmusic", &env.home_dir)?;

        assert_eq!(expanded, env.home_dir.join("V4Vmusic"));
        Ok(())
    }
}
