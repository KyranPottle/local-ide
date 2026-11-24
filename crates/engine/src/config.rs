use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use inference::InferenceSettings;
use protocol::{ModelProfile, RULES_CHARACTER_LIMIT, rules_text};
use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read the configuration file {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("the configuration does not decode: {0}")]
    Decode(#[from] toml::de::Error),
    #[error("invalid configuration: {0}")]
    Invalid(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    listen: SocketAddr,
    public_origin: String,
    allowed_peers: Vec<IpAddr>,
    data_dir: PathBuf,
    assets_dir: PathBuf,
    step_limit: u32,
    tool_output_limit: usize,
    tool_timeout_seconds: u64,
    #[serde(default)]
    system_rules: Option<String>,
    inference: InferenceFile,
    #[serde(default)]
    model: Vec<ModelProfile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InferenceFile {
    base_url: String,
    api_key_env: String,
    idle_timeout_seconds: u64,
}

#[derive(Debug)]
pub struct Config {
    pub listen: SocketAddr,
    // Exact value that an Origin header must carry
    pub public_origin: String,
    pub allowed_peers: Vec<IpAddr>,
    pub data_dir: PathBuf,
    pub assets_dir: PathBuf,
    pub step_limit: u32,
    pub tool_output_limit: usize,
    pub tool_timeout: Duration,
    pub system_rules: Option<String>,
    pub inference: InferenceSettings,
    pub models: Vec<ModelProfile>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_owned(),
            source,
        })?;
        Self::parse(&text, |name| std::env::var(name).ok())
    }

    pub fn parse(
        text: &str,
        environment: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, ConfigError> {
        let file: ConfigFile = toml::from_str(text)?;
        let invalid = |message: &str| Err(ConfigError::Invalid(message.to_owned()));
        if !is_origin(&file.public_origin) {
            return invalid(
                "public_origin must be scheme://host[:port] with http or https and no path",
            );
        }
        if file.allowed_peers.is_empty() {
            return invalid("allowed_peers must list at least one address");
        }
        if !file.assets_dir.join("index.html").is_file() {
            return invalid("assets_dir must be a directory that holds index.html");
        }
        if file.step_limit == 0 || file.tool_output_limit == 0 {
            return invalid("step_limit and tool_output_limit must be at least 1");
        }
        if file.tool_timeout_seconds == 0 || file.inference.idle_timeout_seconds == 0 {
            return invalid(
                "tool_timeout_seconds and inference.idle_timeout_seconds must be at least 1",
            );
        }
        let system_rules = match file.system_rules.as_deref() {
            None => None,
            Some(text) => rules_text(text).map_err(|_| {
                ConfigError::Invalid(format!(
                    "system_rules must hold at most {RULES_CHARACTER_LIMIT} characters"
                ))
            })?,
        };
        validate_models(&file.model)?;
        let api_key = environment(&file.inference.api_key_env)
            .filter(|key| !key.is_empty())
            .ok_or_else(|| {
                ConfigError::Invalid(format!(
                    "the environment variable {} named by inference.api_key_env is unset or empty",
                    file.inference.api_key_env
                ))
            })?;
        Ok(Self {
            listen: file.listen,
            public_origin: file.public_origin,
            allowed_peers: file.allowed_peers,
            data_dir: file.data_dir,
            assets_dir: file.assets_dir,
            step_limit: file.step_limit,
            tool_output_limit: file.tool_output_limit,
            tool_timeout: Duration::from_secs(file.tool_timeout_seconds),
            system_rules,
            inference: InferenceSettings {
                base_url: file.inference.base_url,
                api_key: Some(api_key),
                idle_timeout: Duration::from_secs(file.inference.idle_timeout_seconds),
            },
            models: file.model,
        })
    }
}

fn is_origin(origin: &str) -> bool {
    let Some((scheme, authority)) = origin.split_once("://") else {
        return false;
    };
    matches!(scheme, "http" | "https")
        && !authority.is_empty()
        && !authority.contains(['/', '?', '#', '@', ' '])
}

fn validate_models(models: &[ModelProfile]) -> Result<(), ConfigError> {
    let mut ids = HashSet::new();
    for profile in models {
        let defect = |reason: &str| {
            Err(ConfigError::Invalid(format!(
                "model {:?}: {reason}",
                profile.id
            )))
        };
        if profile.id.trim().is_empty() {
            return defect("id must not be empty");
        }
        if !ids.insert(profile.id.as_str()) {
            return defect("id appears twice");
        }
        if profile.reply_reserve == 0 || profile.reply_reserve >= profile.context_budget {
            return defect("reply_reserve must be at least 1 and less than context_budget");
        }
        if !(profile.chars_per_token.is_finite() && profile.chars_per_token > 0.0) {
            return defect("chars_per_token must be a positive number");
        }
        let sampling_valid = [profile.temperature, profile.top_p]
            .into_iter()
            .flatten()
            .all(|value| value.is_finite() && value >= 0.0);
        if !sampling_valid {
            return defect("temperature and top_p must be non-negative numbers");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Assets(PathBuf);

    impl Assets {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!(
                "local-ide-config-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("the clock is after the epoch")
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("index.html"), "ok").unwrap();
            Self(dir)
        }
    }

    impl Drop for Assets {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn parse(system_rules_line: &str) -> Result<Config, ConfigError> {
        let assets = Assets::new();
        let text = format!(
            r#"
listen = "127.0.0.1:9"
public_origin = "http://127.0.0.1:5173"
allowed_peers = ["127.0.0.1"]
data_dir = "."
assets_dir = "{assets}"
step_limit = 1
tool_output_limit = 1
tool_timeout_seconds = 1
{system_rules_line}
[inference]
base_url = "http://127.0.0.1:9"
api_key_env = "TEST_KEY"
idle_timeout_seconds = 1
"#,
            assets = assets.0.display(),
        );
        Config::parse(&text, |name| (name == "TEST_KEY").then(|| "key".to_owned()))
    }

    #[test]
    fn absent_or_blank_system_rules_add_none() {
        assert_eq!(parse("").unwrap().system_rules, None);
        assert_eq!(parse("system_rules = \"  \"").unwrap().system_rules, None);
    }

    #[test]
    fn system_rules_keep_the_trimmed_text() {
        let config = parse("system_rules = \"\"\"\nUse short names.\n\"\"\"").unwrap();
        assert_eq!(config.system_rules.as_deref(), Some("Use short names."));
    }

    #[test]
    fn system_rules_past_the_cap_stop_the_start() {
        let text = "x".repeat(RULES_CHARACTER_LIMIT + 1);
        let error = parse(&format!("system_rules = \"{text}\"")).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "invalid configuration: system_rules must hold at most {RULES_CHARACTER_LIMIT} characters"
            )
        );
    }
}
