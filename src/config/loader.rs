use crate::{config::ReviewConfig, error::{Result, ReviewerError}};

pub fn parse_config(yaml: &str) -> Result<ReviewConfig> {
    if yaml.trim().is_empty() {
        return Ok(ReviewConfig {
            provider: crate::config::ProviderConfig {
                name: "claude".to_string(),
                model: "claude-sonnet-4-6".to_string(),
            },
            reviewers: Default::default(),
            ignore: Default::default(),
        });
    }
    serde_yaml::from_str(yaml)
        .map_err(|e| ReviewerError::Config(e.to_string()))
}

pub async fn load_config_from_repo(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    token: &str,
) -> Result<ReviewConfig> {
    let url = format!(
        "https://api.github.com/repos/{owner}/{repo}/contents/.reviewbot.yml"
    );
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github.raw+json")
        .header("User-Agent", "ai-code-reviewer/0.1")
        .send()
        .await
        .map_err(|e| ReviewerError::GithubApi(e.to_string()))?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return parse_config("");
    }

    let text = resp.text().await
        .map_err(|e| ReviewerError::GithubApi(e.to_string()))?;
    parse_config(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_config() {
        let yaml = r#"
provider:
  name: claude
  model: claude-sonnet-4-6
reviewers:
  security:
    enabled: true
    severity_threshold: warning
    owasp_categories:
      - injection
  quality:
    enabled: false
    checks: []
ignore:
  paths: []
  max_file_size_kb: 500
"#;
        let config = parse_config(yaml).unwrap();
        assert_eq!(config.provider.name, "claude");
        assert!(config.reviewers.security.enabled);
        assert!(!config.reviewers.quality.enabled);
    }

    #[test]
    fn test_defaults_applied_when_missing() {
        let yaml = r#"
provider:
  name: openai
  model: gpt-4o
"#;
        let config = parse_config(yaml).unwrap();
        assert!(config.reviewers.security.enabled);
        assert!(config.reviewers.quality.enabled);
        assert_eq!(config.ignore.max_file_size_kb, 500);
    }

    #[test]
    fn test_empty_yaml_returns_defaults() {
        let config = parse_config("").unwrap();
        assert_eq!(config.provider.name, "claude");
    }

    #[test]
    fn test_invalid_yaml_returns_error() {
        let yaml = "invalid: [unclosed";
        let result = parse_config(yaml);
        assert!(result.is_err());
    }
}
