use crate::{
    config::ReviewConfig,
    error::{Result, ReviewerError},
    github::GithubClient,
};

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
    serde_yaml::from_str(yaml).map_err(|e| ReviewerError::Config(e.to_string()))
}

/// 저장소의 .reviewbot.yml을 읽어 ReviewConfig를 반환한다.
/// 파일이 없으면 기본값을 반환한다.
/// GithubClient를 재사용하므로 별도 reqwest::Client 생성 및 SSRF 우회가 없다.
pub async fn load_config_from_repo(
    github_client: &GithubClient,
    owner: &str,
    repo: &str,
) -> Result<ReviewConfig> {
    let yaml = github_client.get_repo_config(owner, repo).await?;
    parse_config(&yaml)
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
  quality:
    enabled: false
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
