pub mod loader;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ReviewConfig {
    pub provider: ProviderConfig,
    #[serde(default)]
    pub reviewers: ReviewersConfig,
    #[serde(default)]
    pub ignore: IgnoreConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub model: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ReviewersConfig {
    #[serde(default)]
    pub security: SecurityReviewerConfig,
    #[serde(default)]
    pub quality: QualityReviewerConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SecurityReviewerConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_warning")]
    pub severity_threshold: String,
    #[serde(default = "default_owasp")]
    pub owasp_categories: Vec<String>,
}

impl Default for SecurityReviewerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            severity_threshold: "warning".to_string(),
            owasp_categories: default_owasp(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct QualityReviewerConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_checks")]
    pub checks: Vec<String>,
}

impl Default for QualityReviewerConfig {
    fn default() -> Self {
        Self { enabled: true, checks: default_checks() }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct IgnoreConfig {
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default = "default_max_file_size")]
    pub max_file_size_kb: u64,
}

impl Default for IgnoreConfig {
    fn default() -> Self {
        Self { paths: vec![], max_file_size_kb: 500 }
    }
}

fn default_true() -> bool { true }
fn default_warning() -> String { "warning".to_string() }
fn default_owasp() -> Vec<String> {
    vec!["injection".into(), "auth".into(), "crypto".into(), "secrets".into()]
}
fn default_checks() -> Vec<String> {
    vec!["naming".into(), "complexity".into(), "duplication".into()]
}
fn default_max_file_size() -> u64 { 500 }
