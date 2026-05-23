use crate::error::{Result, ReviewerError};

pub struct GithubClient {
    pub(crate) token: String,
    pub(crate) base_url: String,
    pub(crate) client: reqwest::Client,
}

impl GithubClient {
    pub fn new(token: impl Into<String>, base_url: &str) -> Self {
        Self {
            token: token.into(),
            base_url: base_url.to_string(),
            client: reqwest::Client::new(),
        }
    }

    pub fn from_env() -> Self {
        Self::new(
            std::env::var("GITHUB_TOKEN").expect("GITHUB_TOKEN not set"),
            "https://api.github.com",
        )
    }

    pub async fn get_pr_diff(&self, owner: &str, repo: &str, pr_number: u64) -> Result<String> {
        let url = format!("{}/repos/{owner}/{repo}/pulls/{pr_number}", self.base_url);

        let resp = self.client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github.v3.diff")
            .header("User-Agent", "ai-code-reviewer/0.1")
            .send()
            .await
            .map_err(|e| ReviewerError::GithubApi(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(ReviewerError::GithubApi(format!("GET PR diff {status}")));
        }

        resp.text().await.map_err(|e| ReviewerError::GithubApi(e.to_string()))
    }

    pub async fn get_file_content(&self, owner: &str, repo: &str, path: &str) -> Result<String> {
        // SSRF 방지: github.com 또는 localhost만 허용
        if !self.base_url.contains("github.com")
            && !self.base_url.starts_with("http://127.0.0.1")
            && !self.base_url.starts_with("http://localhost")
        {
            return Err(ReviewerError::GithubApi(
                "non-github URL rejected (SSRF prevention)".into()
            ));
        }

        let url = format!("{}/repos/{owner}/{repo}/contents/{path}", self.base_url);

        let resp = self.client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github.raw+json")
            .header("User-Agent", "ai-code-reviewer/0.1")
            .send()
            .await
            .map_err(|e| ReviewerError::GithubApi(e.to_string()))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(String::new());
        }

        resp.text().await.map_err(|e| ReviewerError::GithubApi(e.to_string()))
    }
}
