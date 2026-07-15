use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use semver::Version;
use serde::{Deserialize, Serialize};

pub const SETTINGS_RELATIVE_PATH: &str = ".mcpify/settings.yaml";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PublicationMetadata {
    #[serde(default = "default_license")]
    pub license: Option<String>,
    pub repository: Option<String>,
    #[serde(default = "default_readme")]
    pub readme: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
}

impl Default for PublicationMetadata {
    fn default() -> Self {
        Self {
            license: default_license(),
            repository: None,
            readme: default_readme(),
            authors: Vec::new(),
            keywords: Vec::new(),
            categories: Vec::new(),
            exclude: Vec::new(),
        }
    }
}

fn default_readme() -> String {
    "README.md".to_string()
}

fn default_license() -> Option<String> {
    Some("MIT".to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HeaderSetting {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectSettings {
    #[serde(default)]
    pub publication: PublicationMetadata,
    #[serde(default)]
    pub default_headers: Vec<HeaderSetting>,
    pub package_size_limit_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AuthOverrideKind {
    Basic,
    ApiKey,
    Pat,
    OAuth1,
    OAuth2,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AuthOverrideLocation {
    Header,
    Query,
    Cookie,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthOverride {
    pub name: String,
    pub kind: AuthOverrideKind,
    pub location: Option<AuthOverrideLocation>,
    pub parameter_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreprocessCommand {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionSpec {
    pub version: String,
    pub source: String,
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub preprocess: Vec<PreprocessCommand>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum VersionPolicy {
    #[default]
    All,
    LatestPerMinor,
    Allowlist {
        versions: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectManifest {
    pub language: String,
    pub output: PathBuf,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub publish_registry: bool,
    #[serde(default)]
    pub publication: PublicationMetadata,
    #[serde(default)]
    pub default_headers: BTreeMap<String, String>,
    #[serde(default)]
    pub auth: Vec<AuthOverride>,
    pub versions: Vec<VersionSpec>,
    #[serde(default)]
    pub version_policy: VersionPolicy,
    pub package_size_limit_mb: Option<u64>,
}

impl ProjectManifest {
    pub fn from_yaml(yaml: &str) -> Result<Self> {
        serde_yaml::from_str(yaml).context("failed to parse mcpify project manifest")
    }

    pub async fn read(path: &Path) -> Result<Self> {
        Ok(Self::read_portable_and_resolved(path).await?.1)
    }

    pub async fn read_portable_and_resolved(path: &Path) -> Result<(Self, Self)> {
        let yaml = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("failed to read manifest '{}'", path.display()))?;
        let portable = Self::from_yaml(&yaml)?;
        portable.validate()?;
        let mut resolved = portable.clone();
        if resolved.output.is_relative() {
            let parent = path.parent().unwrap_or_else(|| Path::new("."));
            resolved.output = parent.join(&resolved.output);
        }
        for version in &mut resolved.versions {
            if !version.source.starts_with("http://") && !version.source.starts_with("https://") {
                let source = Path::new(&version.source);
                if source.is_relative() {
                    let parent = path.parent().unwrap_or_else(|| Path::new("."));
                    version.source = parent.join(source).to_string_lossy().into_owned();
                }
            }
            for hook in &mut version.preprocess {
                let command = Path::new(&hook.command);
                if command.is_relative()
                    && (hook.command.contains('/') || hook.command.contains('\\'))
                {
                    let parent = path.parent().unwrap_or_else(|| Path::new("."));
                    hook.command = parent.join(command).to_string_lossy().into_owned();
                }
            }
        }
        Ok((portable, resolved))
    }

    pub fn validate(&self) -> Result<()> {
        if !crate::cli::SUPPORTED_LANGUAGES.contains(&self.language.as_str()) {
            bail!("unsupported manifest language '{}'", self.language);
        }
        if self.versions.is_empty() {
            bail!("manifest must contain at least one version");
        }
        let defaults = self
            .versions
            .iter()
            .filter(|version| version.default)
            .count();
        if defaults != 1 {
            bail!("manifest must mark exactly one version as default; found {defaults}");
        }
        let mut seen = std::collections::HashSet::new();
        for version in &self.versions {
            if version.version.trim().is_empty() || version.source.trim().is_empty() {
                bail!("manifest versions require non-empty version and source values");
            }
            if !seen.insert(&version.version) {
                bail!("manifest contains duplicate version '{}'", version.version);
            }
        }
        if self.publish_registry {
            if self
                .publication
                .license
                .as_deref()
                .is_none_or(str::is_empty)
            {
                bail!("publish_registry requires publication.license");
            }
            if self
                .publication
                .repository
                .as_deref()
                .is_none_or(str::is_empty)
            {
                bail!("publish_registry requires publication.repository");
            }
            if self.publication.authors.is_empty() {
                bail!("publish_registry requires at least one publication.authors entry");
            }
        }
        for auth in &self.auth {
            if auth.location.is_some() != auth.parameter_name.is_some() {
                bail!(
                    "auth override '{}' must set both location and parameter_name, or neither",
                    auth.name
                );
            }
        }
        Ok(())
    }

    pub fn settings(&self) -> ProjectSettings {
        let mut headers = self.default_headers.clone();
        if !headers
            .keys()
            .any(|name| name.eq_ignore_ascii_case("user-agent"))
        {
            let project = self
                .output
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("mcpify-client");
            headers.insert(
                "User-Agent".to_string(),
                format!("{project}/0.1.0 (generated by mcpify)"),
            );
        }
        ProjectSettings {
            publication: self.publication.clone(),
            default_headers: headers
                .into_iter()
                .map(|(name, value)| HeaderSetting { name, value })
                .collect(),
            package_size_limit_bytes: Some(
                self.package_size_limit_mb
                    .unwrap_or(10)
                    .saturating_mul(1024 * 1024),
            ),
        }
    }
}

pub fn select_versions<'a>(
    versions: &'a [VersionSpec],
    policy: &VersionPolicy,
) -> Result<Vec<&'a VersionSpec>> {
    let selected: Vec<&VersionSpec> = match policy {
        VersionPolicy::All => versions.iter().collect(),
        VersionPolicy::Allowlist {
            versions: allowlist,
        } => versions
            .iter()
            .filter(|version| allowlist.contains(&version.version))
            .collect(),
        VersionPolicy::LatestPerMinor => {
            let mut best: HashMap<(u64, u64), (&VersionSpec, Version)> = HashMap::new();
            for item in versions {
                let parsed = Version::parse(item.version.trim_start_matches('v')).with_context(|| {
                    format!(
                        "latest-per-minor requires semantic version labels; '{}' is not valid semver",
                        item.version
                    )
                })?;
                let key = (parsed.major, parsed.minor);
                if best.get(&key).is_none_or(|(_, current)| parsed > *current) {
                    best.insert(key, (item, parsed));
                }
            }
            versions
                .iter()
                .filter(|item| {
                    best.values()
                        .any(|(selected, _)| selected.version == item.version)
                })
                .collect()
        }
    };
    if selected.is_empty() {
        bail!("version policy selected no versions");
    }
    if !selected.iter().any(|version| version.default) {
        bail!("version policy excluded the default version");
    }
    Ok(selected)
}

pub async fn write_settings(project_dir: &Path, settings: &ProjectSettings) -> Result<()> {
    let path = project_dir.join(SETTINGS_RELATIVE_PATH);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let yaml = serde_yaml::to_string(settings).context("failed to serialize project settings")?;
    tokio::fs::write(&path, yaml)
        .await
        .with_context(|| format!("failed to write '{}'", path.display()))
}

pub fn read_settings(project_dir: &Path) -> ProjectSettings {
    let path = project_dir.join(SETTINGS_RELATIVE_PATH);
    std::fs::read_to_string(path)
        .ok()
        .and_then(|yaml| serde_yaml::from_str(&yaml).ok())
        .unwrap_or_default()
}
