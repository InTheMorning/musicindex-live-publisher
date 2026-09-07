//! Service configuration and token loading.

use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{env, fs};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::{FallbackConfig, LiveValue, LiveValueDestination, LiveValueModel, WatchTarget};

/// Default service configuration path.
pub const DEFAULT_CONFIG_PATH: &str = "/etc/musicindex-live-publisher/config.toml";

/// Ceiling for a configured stream delay.
///
/// Chosen well above any real broadcast buffer. Its job is to catch a units
/// mistake — milliseconds typed into a seconds field — at startup rather than
/// on air, where it would park every payload past the end of the show.
const MAX_STREAM_DELAY_SECS: f64 = 300.0;

const DEFAULT_DEAD_FALLBACK_TITLE: &str = "No V4V track playing";
const DEFAULT_DEAD_FALLBACK_RECIPIENT_NAME: &str = "No V4V payment route";
const DEFAULT_DEAD_FALLBACK_ADDRESS: &str = "no-v4v-track@example.invalid";

const EVENT_ID_PLACEHOLDERS: &[&str] = &[
    "replace-with-provisioned-event-guid",
    "the-provisioned-event-guid",
    "<event_id>",
];
const DESTINATION_ADDRESS_PLACEHOLDERS: &[&str] = &[
    "YOUR_LIGHTNING_DESTINATION",
    "YOUR_LIGHTNING_NODE_PUBKEY",
    "replace-with-lightning-destination",
    "03your-node-pubkey",
    "03...",
];

/// Runtime service configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct PublisherConfig {
    pub watch_dir: PathBuf,
    pub endpoint: String,
    pub targets: Vec<PublisherTarget>,
}

/// A configured publishing target with its loaded broadcaster token.
#[derive(Clone, PartialEq)]
pub struct PublisherTarget {
    pub name: String,
    pub event_id: String,
    pub token_file: PathBuf,
    pub token: String,
    pub stream_delay: Duration,
    pub fallback: FallbackConfig,
}

impl fmt::Debug for PublisherTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PublisherTarget")
            .field("name", &self.name)
            .field("event_id", &self.event_id)
            .field("token_file", &self.token_file)
            .field("token", &"<redacted>")
            .field("stream_delay", &self.stream_delay)
            .field("fallback", &self.fallback)
            .finish()
    }
}

impl PublisherTarget {
    /// Returns the watcher-facing target data.
    pub fn watch_target(&self) -> WatchTarget {
        WatchTarget {
            name: self.name.clone(),
            event_guid: self.event_id.clone(),
            fallback: self.fallback.clone(),
        }
    }
}

/// CLI overrides that take precedence over config fields.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConfigOverrides {
    pub watch_dir: Option<PathBuf>,
    pub endpoint: Option<String>,
}

/// A target stanza to add to the publisher configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetConfigEdit {
    pub name: String,
    pub event_id: String,
    pub token_file: PathBuf,
    pub stream_delay_secs: Option<f64>,
}

/// A redacted target summary read from the publisher configuration.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TargetConfigSummary {
    pub name: String,
    pub event_id: String,
    pub token_file: PathBuf,
    pub stream_delay_secs: f64,
}

/// A config edit failure with a stable command-line meaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigEditError {
    TargetExists(String),
    TargetNotFound(String),
}

impl fmt::Display for ConfigEditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TargetExists(name) => write!(formatter, "target {name} already exists"),
            Self::TargetNotFound(name) => write!(formatter, "target {name} not found"),
        }
    }
}

impl std::error::Error for ConfigEditError {}

#[derive(Debug, Deserialize)]
struct RawConfig {
    watch_dir: PathBuf,
    endpoint: String,
    #[serde(rename = "target")]
    targets: Vec<RawTarget>,
}

#[derive(Debug, Deserialize)]
struct RawTarget {
    name: String,
    event_id: String,
    token_file: PathBuf,
    stream_delay_secs: Option<f64>,
    fallback: Option<RawFallback>,
}

#[derive(Debug, Deserialize)]
struct RawFallback {
    title: Option<String>,
    image: Option<String>,
    model: Option<LiveValueModel>,
    #[serde(default)]
    destinations: Vec<LiveValueDestination>,
    value: Option<RawFallbackValue>,
}

#[derive(Debug, Deserialize)]
struct RawFallbackValue {
    model: Option<LiveValueModel>,
    #[serde(default)]
    destinations: Vec<LiveValueDestination>,
}

#[derive(Debug, Deserialize)]
struct RawTargetSummaryConfig {
    #[serde(default, rename = "target")]
    targets: Vec<RawTargetSummary>,
}

#[derive(Debug, Deserialize)]
struct RawTargetSummary {
    name: String,
    event_id: String,
    token_file: PathBuf,
    stream_delay_secs: Option<f64>,
}

/// Loads, validates, and resolves token files for a service config.
///
/// # Errors
///
/// Returns an error when the config file cannot be read, TOML is invalid, a
/// validation rule fails, or a token file is missing or empty.
pub fn load_config(path: &Path, overrides: ConfigOverrides) -> Result<PublisherConfig> {
    let bytes = fs::read(path).with_context(|| format!("read config file {}", path.display()))?;
    load_config_bytes(&bytes, Some(path), overrides)
}

/// Loads config from bytes, resolving token files from disk.
///
/// This helper is public for integration tests and small embedding tools.
///
/// # Errors
///
/// Returns an error when TOML parsing, validation, or token loading fails.
pub fn load_config_bytes(
    bytes: &[u8],
    source_path: Option<&Path>,
    overrides: ConfigOverrides,
) -> Result<PublisherConfig> {
    let text = std::str::from_utf8(bytes).with_context(|| {
        source_path
            .map(|path| format!("read config TOML as UTF-8 {}", path.display()))
            .unwrap_or_else(|| "read config TOML as UTF-8".to_owned())
    })?;
    let raw: RawConfig = toml::from_str(text).with_context(|| {
        source_path
            .map(|path| format!("parse config TOML {}", path.display()))
            .unwrap_or_else(|| "parse config TOML".to_owned())
    })?;
    resolve_config(raw, overrides)
}

/// Lists target stanzas without reading broadcaster tokens.
///
/// # Errors
///
/// Returns an error when the config file cannot be read, TOML is invalid, or a
/// target summary has an invalid stream delay.
pub fn list_config_targets(path: &Path) -> Result<Vec<TargetConfigSummary>> {
    let text =
        fs::read_to_string(path).with_context(|| format!("read config file {}", path.display()))?;
    list_config_targets_from_str(&text)
        .with_context(|| format!("parse config targets {}", path.display()))
}

/// Adds or replaces one target stanza in the config file.
///
/// # Errors
///
/// Returns an error when the existing config cannot be read or parsed, the
/// target is invalid, the token file cannot be read, or the write fails.
pub fn add_target_to_config(path: &Path, edit: &TargetConfigEdit, replace: bool) -> Result<()> {
    validate_target_config_edit(edit)?;
    let text =
        fs::read_to_string(path).with_context(|| format!("read config file {}", path.display()))?;
    let edited = add_target_to_config_text(&text, edit, replace)?;
    write_config_text_atomic(path, &edited)
}

/// Removes one target stanza from the config file.
///
/// # Errors
///
/// Returns an error when the existing config cannot be read or parsed, the
/// target is missing, or the write fails.
pub fn remove_target_from_config(path: &Path, name: &str) -> Result<()> {
    validate_target_name(name)?;
    let text =
        fs::read_to_string(path).with_context(|| format!("read config file {}", path.display()))?;
    let edited = remove_target_from_config_text(&text, name)?;
    write_config_text_atomic(path, &edited)
}

/// Lists target stanzas from TOML text without reading broadcaster tokens.
///
/// # Errors
///
/// Returns an error when TOML is invalid or a target summary has an invalid
/// stream delay.
pub fn list_config_targets_from_str(text: &str) -> Result<Vec<TargetConfigSummary>> {
    let raw: RawTargetSummaryConfig =
        toml::from_str(text).context("parse config TOML for target list")?;
    let mut seen = HashSet::new();

    raw.targets
        .into_iter()
        .map(|target| {
            if !seen.insert(target.name.clone()) {
                return Err(anyhow!("duplicate target name {}", target.name));
            }
            validate_stream_delay_secs(&target.name, target.stream_delay_secs)?;
            Ok(TargetConfigSummary {
                name: target.name,
                event_id: target.event_id,
                token_file: target.token_file,
                stream_delay_secs: target.stream_delay_secs.unwrap_or(0.0),
            })
        })
        .collect()
}

/// Adds or replaces one target stanza in TOML text.
///
/// # Errors
///
/// Returns an error when the existing TOML is invalid or the target exists
/// without `replace`.
pub fn add_target_to_config_text(
    text: &str,
    edit: &TargetConfigEdit,
    replace: bool,
) -> Result<String> {
    let existing = list_config_targets_from_str(text)?;
    let exists = existing.iter().any(|target| target.name == edit.name);
    if exists && !replace {
        return Err(ConfigEditError::TargetExists(edit.name.clone()).into());
    }

    let stanza = render_target_config_stanza(edit);
    if exists {
        let lines = split_preserving_newlines(text);
        let span = target_stanza_named(text, &lines, &edit.name)?
            .ok_or_else(|| ConfigEditError::TargetNotFound(edit.name.clone()))?;
        let mut edited = lines[..span.start].concat();
        edited.push_str(&stanza);
        edited.push_str(&lines[span.end..].concat());
        return Ok(edited);
    }

    Ok(append_target_config_stanza(text, &stanza))
}

/// Removes one target stanza from TOML text.
///
/// # Errors
///
/// Returns an error when the existing TOML is invalid or the target is missing.
pub fn remove_target_from_config_text(text: &str, name: &str) -> Result<String> {
    list_config_targets_from_str(text)?;
    let lines = split_preserving_newlines(text);
    let span = target_stanza_named(text, &lines, name)?
        .ok_or_else(|| ConfigEditError::TargetNotFound(name.to_owned()))?;
    let mut edited = lines[..span.start].concat();
    edited.push_str(&lines[span.end..].concat());
    Ok(edited)
}

fn resolve_config(raw: RawConfig, overrides: ConfigOverrides) -> Result<PublisherConfig> {
    if raw.targets.is_empty() {
        return Err(anyhow!("config must define at least one target"));
    }

    let mut seen = HashSet::new();
    let targets = raw
        .targets
        .into_iter()
        .map(|target| resolve_target(target, &mut seen))
        .collect::<Result<Vec<_>>>()?;

    Ok(PublisherConfig {
        watch_dir: overrides.watch_dir.unwrap_or(raw.watch_dir),
        endpoint: overrides.endpoint.unwrap_or(raw.endpoint),
        targets,
    })
}

fn resolve_target(target: RawTarget, seen: &mut HashSet<String>) -> Result<PublisherTarget> {
    if target.name.trim().is_empty() {
        return Err(anyhow!("target name must not be empty"));
    }
    if !seen.insert(target.name.clone()) {
        return Err(anyhow!("duplicate target name {}", target.name));
    }
    validate_event_id(&target.name, &target.event_id)?;
    let stream_delay = resolve_stream_delay(&target.name, target.stream_delay_secs)?;

    let fallback = target.fallback.map_or_else(
        || {
            Ok(default_dead_fallback(
                &target.name,
                None,
                None,
                "target has no fallback configuration",
            ))
        },
        |fallback| resolve_fallback(&target.name, fallback),
    )?;

    let token_file = resolve_token_file_path(
        &target.token_file,
        env::var_os("CREDENTIALS_DIRECTORY").as_ref().map(Path::new),
        env::var_os("HOME").as_ref().map(Path::new),
    )?;
    let token = load_token_file(&token_file)?;

    Ok(PublisherTarget {
        name: target.name,
        event_id: target.event_id,
        token_file,
        token,
        stream_delay,
        fallback,
    })
}

fn validate_target_config_edit(edit: &TargetConfigEdit) -> Result<()> {
    validate_target_name(&edit.name)?;
    validate_no_control_chars("target event_id", &edit.event_id)?;
    validate_event_id(&edit.name, &edit.event_id)?;
    validate_stream_delay_secs(&edit.name, edit.stream_delay_secs)?;
    validate_token_file_readable(&edit.token_file)
}

fn validate_target_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(anyhow!("target name must not be empty"));
    }
    validate_no_control_chars("target name", name)
}

fn validate_no_control_chars(label: &str, value: &str) -> Result<()> {
    if value.chars().any(char::is_control) {
        return Err(anyhow!("{label} must not contain control characters"));
    }
    Ok(())
}

fn validate_stream_delay_secs(target_name: &str, stream_delay_secs: Option<f64>) -> Result<()> {
    resolve_stream_delay(target_name, stream_delay_secs).map(|_| ())
}

fn validate_token_file_readable(path: &Path) -> Result<()> {
    let metadata =
        fs::metadata(path).with_context(|| format!("inspect token file {}", path.display()))?;
    if !metadata.is_file() {
        return Err(anyhow!("token file {} is not a file", path.display()));
    }
    fs::File::open(path)
        .with_context(|| format!("read token file {}", path.display()))
        .map(|_| ())
}

/// Resolves a target's broadcast stream delay.
///
/// The delay compensates for the buffering between the publisher and a
/// listener's ears. Zero means publish on sight, which is the behavior every
/// config had before this field existed.
fn resolve_stream_delay(target_name: &str, stream_delay_secs: Option<f64>) -> Result<Duration> {
    let Some(seconds) = stream_delay_secs else {
        return Ok(Duration::ZERO);
    };
    if !seconds.is_finite() {
        return Err(anyhow!(
            "target {target_name} stream_delay_secs must be a finite number"
        ));
    }
    if seconds < 0.0 {
        return Err(anyhow!(
            "target {target_name} stream_delay_secs must not be negative"
        ));
    }
    if seconds > MAX_STREAM_DELAY_SECS {
        return Err(anyhow!(
            "target {target_name} stream_delay_secs {seconds} exceeds the {MAX_STREAM_DELAY_SECS} second maximum"
        ));
    }

    Duration::try_from_secs_f64(seconds).with_context(|| {
        format!("target {target_name} stream_delay_secs {seconds} is not a usable duration")
    })
}

fn validate_event_id(target_name: &str, event_id: &str) -> Result<()> {
    let event_id = event_id.trim();
    if event_id.is_empty() {
        return Err(anyhow!("target {target_name} event_id must not be empty"));
    }
    if EVENT_ID_PLACEHOLDERS.contains(&event_id)
        || event_id.contains("replace-with")
        || event_id.contains("provisioned")
    {
        return Err(anyhow!(
            "target {target_name} event_id is still an example placeholder; run provision and paste the returned event_id"
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TargetStanzaSpan {
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TableHeaderKind {
    Target,
    TargetChild,
    Other,
}

fn target_stanza_named(text: &str, lines: &[&str], name: &str) -> Result<Option<TargetStanzaSpan>> {
    for span in target_stanza_spans(lines) {
        let stanza = lines[span.start..span.end].concat();
        if target_name_from_stanza(&stanza)?.as_deref() == Some(name) {
            return Ok(Some(span));
        }
    }

    let summaries = list_config_targets_from_str(text)?;
    if summaries.iter().any(|target| target.name == name) {
        return Err(anyhow!("could not locate target stanza {name}"));
    }
    Ok(None)
}

fn target_name_from_stanza(stanza: &str) -> Result<Option<String>> {
    let value: toml::Value = toml::from_str(stanza).context("parse target stanza")?;
    Ok(value
        .get("target")
        .and_then(toml::Value::as_array)
        .and_then(|targets| targets.first())
        .and_then(|target| target.get("name"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned))
}

fn target_stanza_spans(lines: &[&str]) -> Vec<TargetStanzaSpan> {
    let mut spans = Vec::new();
    let mut current_start = None;

    for (index, line) in lines.iter().enumerate() {
        match table_header_kind(line) {
            Some(TableHeaderKind::Target) => {
                if let Some(start) = current_start {
                    spans.push(TargetStanzaSpan {
                        start,
                        end: trim_span_end(lines, start, index),
                    });
                }
                current_start = Some(index);
            }
            Some(TableHeaderKind::Other) => {
                if let Some(start) = current_start.take() {
                    spans.push(TargetStanzaSpan {
                        start,
                        end: trim_span_end(lines, start, index),
                    });
                }
            }
            Some(TableHeaderKind::TargetChild) | None => {}
        }
    }

    if let Some(start) = current_start {
        spans.push(TargetStanzaSpan {
            start,
            end: trim_span_end(lines, start, lines.len()),
        });
    }

    spans
}

fn trim_span_end(lines: &[&str], start: usize, mut end: usize) -> usize {
    while end > start && is_blank_or_comment(lines[end - 1]) {
        end -= 1;
    }
    end
}

fn is_blank_or_comment(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.is_empty() || trimmed.starts_with('#')
}

fn table_header_kind(line: &str) -> Option<TableHeaderKind> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('[') {
        return None;
    }

    let header = trimmed.split('#').next()?.trim();
    if header == "[[target]]" {
        return Some(TableHeaderKind::Target);
    }
    if (header.starts_with("[target.") && header.ends_with(']'))
        || (header.starts_with("[[target.") && header.ends_with("]]"))
    {
        return Some(TableHeaderKind::TargetChild);
    }
    Some(TableHeaderKind::Other)
}

fn split_preserving_newlines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

fn append_target_config_stanza(text: &str, stanza: &str) -> String {
    if text.is_empty() {
        return stanza.to_owned();
    }

    let mut edited = text.to_owned();
    if !edited.ends_with('\n') {
        edited.push('\n');
    }
    edited.push('\n');
    edited.push_str(stanza);
    edited
}

fn render_target_config_stanza(edit: &TargetConfigEdit) -> String {
    let mut stanza = String::new();
    stanza.push_str("[[target]]\n");
    stanza.push_str(&format!("name = {}\n", toml_string(&edit.name)));
    stanza.push_str(&format!("event_id = {}\n", toml_string(&edit.event_id)));
    stanza.push_str(&format!(
        "token_file = {}\n",
        toml_string(&edit.token_file.display().to_string())
    ));
    if let Some(stream_delay_secs) = edit.stream_delay_secs {
        stanza.push_str(&format!("stream_delay_secs = {stream_delay_secs}\n"));
    }
    stanza
}

fn write_config_text_atomic(path: &Path, text: &str) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .ok_or_else(|| anyhow!("config path {} has no file name", path.display()))?;
    let temp_name = format!(
        ".{}.tmp-{}",
        file_name.to_string_lossy(),
        std::process::id()
    );
    let temp_path = parent.join(temp_name);
    fs::write(&temp_path, text)
        .with_context(|| format!("write temporary config file {}", temp_path.display()))?;
    fs::rename(&temp_path, path).with_context(|| {
        format!(
            "rename temporary config file {} to {}",
            temp_path.display(),
            path.display()
        )
    })
}

fn toml_string(value: &str) -> String {
    toml::Value::String(value.to_owned()).to_string()
}

fn resolve_token_file_path(
    token_file: &Path,
    credentials_directory: Option<&Path>,
    home_directory: Option<&Path>,
) -> Result<PathBuf> {
    let Some(raw) = token_file.to_str() else {
        return Ok(token_file.to_path_buf());
    };

    if let Some(rest) = raw.strip_prefix("%d/") {
        let credentials_directory = credentials_directory
            .ok_or_else(|| anyhow!("token_file {raw:?} requires CREDENTIALS_DIRECTORY"))?;
        if rest.is_empty() {
            return Err(anyhow!("token_file {raw:?} must name a credential"));
        }
        return Ok(credentials_directory.join(rest));
    }

    if raw == "~" || raw.starts_with("~/") {
        let home_directory =
            home_directory.ok_or_else(|| anyhow!("token_file {raw:?} requires HOME"))?;
        let rest = raw.strip_prefix("~/").unwrap_or_default();
        return Ok(home_directory.join(rest));
    }

    Ok(token_file.to_path_buf())
}

fn resolve_fallback(target_name: &str, fallback: RawFallback) -> Result<FallbackConfig> {
    let RawFallback {
        title,
        image,
        model,
        destinations: direct_destinations,
        value,
    } = fallback;
    let title = title.unwrap_or_else(|| DEFAULT_DEAD_FALLBACK_TITLE.to_owned());
    let (value_model, value_destinations) = match value {
        Some(value) => (value.model, value.destinations),
        None => (None, Vec::new()),
    };

    if model.is_some() && value_model.is_some() {
        return Err(anyhow!(
            "target {target_name} fallback must define model either directly or under value, not both"
        ));
    }

    let has_direct_destinations = !direct_destinations.is_empty();
    let has_value_destinations = !value_destinations.is_empty();
    if has_direct_destinations && has_value_destinations {
        return Err(anyhow!(
            "target {target_name} fallback must define destinations either directly or under value, not both"
        ));
    }

    let destinations = if has_direct_destinations {
        direct_destinations
    } else {
        value_destinations
    };

    if destinations.is_empty() {
        return Ok(default_dead_fallback(
            target_name,
            Some(title),
            image,
            "target fallback has no payment destinations",
        ));
    }

    let model = model.or(value_model).unwrap_or_else(default_fallback_model);
    validate_fallback_model(target_name, &model)?;
    validate_fallback_destinations(target_name, &destinations)?;

    Ok(FallbackConfig {
        title,
        image,
        value: LiveValue {
            model,
            destinations,
        },
    })
}

fn default_dead_fallback(
    target_name: &str,
    title: Option<String>,
    image: Option<String>,
    reason: &str,
) -> FallbackConfig {
    tracing::warn!(
        target = target_name,
        reason,
        fallback_type = "lnaddress",
        fallback_address = DEFAULT_DEAD_FALLBACK_ADDRESS,
        "FALLBACK PAYMENT ROUTE MISSING; publishing default dead fallback route during idle/non-V4V playback; configure target.fallback.value.destinations to receive station payments"
    );
    FallbackConfig {
        title: title.unwrap_or_else(|| DEFAULT_DEAD_FALLBACK_TITLE.to_owned()),
        image,
        value: default_dead_fallback_value(),
    }
}

fn default_dead_fallback_value() -> LiveValue {
    LiveValue {
        model: LiveValueModel {
            kind: "lightning".to_owned(),
            method: "lnaddress".to_owned(),
            suggested: None,
        },
        destinations: vec![LiveValueDestination {
            kind: Some("lnaddress".to_owned()),
            name: Some(DEFAULT_DEAD_FALLBACK_RECIPIENT_NAME.to_owned()),
            address: Some(DEFAULT_DEAD_FALLBACK_ADDRESS.to_owned()),
            split: Some("100".to_owned()),
            custom_key: None,
            custom_value: None,
            fee: None,
        }],
    }
}

fn default_fallback_model() -> LiveValueModel {
    LiveValueModel {
        kind: "lightning".to_owned(),
        method: "keysend".to_owned(),
        suggested: None,
    }
}

fn validate_fallback_model(target_name: &str, model: &LiveValueModel) -> Result<()> {
    if model.kind.trim().is_empty() {
        return Err(anyhow!(
            "target {target_name} fallback value model type must not be empty"
        ));
    }
    if model.method.trim().is_empty() {
        return Err(anyhow!(
            "target {target_name} fallback value model method must not be empty"
        ));
    }
    Ok(())
}

fn validate_fallback_destinations(
    target_name: &str,
    destinations: &[LiveValueDestination],
) -> Result<()> {
    if destinations.is_empty() {
        return Err(anyhow!(
            "target {target_name} fallback destinations must not be empty"
        ));
    }

    for (index, destination) in destinations.iter().enumerate() {
        validate_required_destination_field(
            target_name,
            index,
            "name",
            destination.name.as_deref(),
        )?;
        validate_required_destination_field(
            target_name,
            index,
            "type",
            destination.kind.as_deref(),
        )?;
        let address = validate_required_destination_field(
            target_name,
            index,
            "address",
            destination.address.as_deref(),
        )?;
        validate_destination_address(target_name, index, address)?;
        let split = validate_required_destination_field(
            target_name,
            index,
            "split",
            destination.split.as_deref(),
        )?;
        let parsed = split.parse::<f64>().with_context(|| {
            format!("target {target_name} fallback destination {index} split must be decimal")
        })?;
        if !parsed.is_finite() {
            return Err(anyhow!(
                "target {target_name} fallback destination {index} split must be finite"
            ));
        }
    }

    Ok(())
}

fn validate_destination_address(target_name: &str, index: usize, address: &str) -> Result<()> {
    let address = address.trim();
    if DESTINATION_ADDRESS_PLACEHOLDERS.contains(&address)
        || address.contains("YOUR_")
        || address.contains("your-")
        || address.contains("replace-with")
    {
        return Err(anyhow!(
            "target {target_name} fallback destination {index} address is still an example placeholder"
        ));
    }
    Ok(())
}

fn validate_required_destination_field<'a>(
    target_name: &str,
    index: usize,
    field: &str,
    value: Option<&'a str>,
) -> Result<&'a str> {
    let Some(value) = value else {
        return Err(anyhow!(
            "target {target_name} fallback destination {index} missing {field}"
        ));
    };
    if value.trim().is_empty() {
        return Err(anyhow!(
            "target {target_name} fallback destination {index} {field} must not be empty"
        ));
    }
    Ok(value)
}

fn load_token_file(path: &Path) -> Result<String> {
    warn_if_token_file_permissive(path);
    let token = fs::read_to_string(path)
        .with_context(|| format!("read token file {}", path.display()))?
        .trim_end_matches(char::is_whitespace)
        .to_owned();

    if token.is_empty() {
        return Err(anyhow!("token file {} is empty", path.display()));
    }

    Ok(token)
}

#[cfg(unix)]
fn warn_if_token_file_permissive(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    match fs::metadata(path) {
        Ok(metadata) => {
            let mode = metadata.permissions().mode() & 0o777;
            if mode & 0o077 != 0 {
                tracing::warn!(
                    path = %path.display(),
                    mode = format_args!("{mode:03o}"),
                    "token file permissions are more permissive than 0600"
                );
            }
        }
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                %error,
                "could not inspect token file permissions"
            );
        }
    }
}

#[cfg(not(unix))]
fn warn_if_token_file_permissive(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_token_path_expands_from_credentials_directory() -> Result<()> {
        let path = resolve_token_file_path(
            Path::new("%d/default.token"),
            Some(Path::new(
                "/run/credentials/musicindex-live-publisher.service",
            )),
            None,
        )?;

        assert_eq!(
            path,
            Path::new("/run/credentials/musicindex-live-publisher.service/default.token")
        );
        Ok(())
    }

    #[test]
    fn credential_token_path_requires_credentials_directory() {
        let error = resolve_token_file_path(Path::new("%d/default.token"), None, None);

        assert!(
            error.is_err_and(|error| {
                error.to_string().contains("requires CREDENTIALS_DIRECTORY")
            })
        );
    }

    #[test]
    fn home_token_path_expands_from_home_directory() -> Result<()> {
        let path = resolve_token_file_path(
            Path::new("~/.config/musicindex-live-publisher/tokens/default.token"),
            None,
            Some(Path::new("/home/tester")),
        )?;

        assert_eq!(
            path,
            Path::new("/home/tester/.config/musicindex-live-publisher/tokens/default.token")
        );
        Ok(())
    }

    #[test]
    fn home_token_path_requires_home_directory() {
        let error = resolve_token_file_path(Path::new("~/default.token"), None, None);

        assert!(error.is_err_and(|error| error.to_string().contains("requires HOME")));
    }
}
