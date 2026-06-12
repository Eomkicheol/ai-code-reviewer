use crate::error::{Result, ReviewerError};

pub struct GithubClient {
    // pub(super): github 모듈 내 comment.rs에서만 접근, 외부 노출 불필요
    pub(super) token: String,
    pub(super) base_url: String,
    pub(super) client: reqwest::Client,
}

impl GithubClient {
    /// base_url 유효성 검증 후 클라이언트를 생성한다.
    ///
    /// 허용 규칙:
    /// - `https` scheme: 운영자가 env로 설정하는 신뢰 값이므로 GitHub Enterprise 커스텀 도메인 허용
    /// - `http` scheme: loopback(127.0.0.1 / localhost / ::1) 호스트만 허용 (테스트 mock용)
    /// - 그 외: 거부 (SSRF 방지)
    pub fn new(token: impl Into<String>, base_url: &str) -> Result<Self> {
        let parsed = reqwest::Url::parse(base_url)
            .map_err(|e| ReviewerError::GithubApi(format!("invalid base_url: {e}")))?;

        match parsed.scheme() {
            "https" => {}
            "http" => {
                // loopback 호스트만 허용 (테스트 mock 서버용)
                let host = parsed.host_str().unwrap_or("");
                let is_loopback = host == "127.0.0.1" || host == "localhost" || host == "::1";
                if !is_loopback {
                    return Err(ReviewerError::GithubApi(
                        "http scheme은 loopback 호스트(127.0.0.1/localhost/::1)만 허용 (SSRF 방지)"
                            .into(),
                    ));
                }
            }
            scheme => {
                return Err(ReviewerError::GithubApi(format!(
                    "허용되지 않는 URL scheme '{scheme}' (SSRF 방지)"
                )));
            }
        }

        Ok(Self {
            token: token.into(),
            base_url: base_url.to_string(),
            client: reqwest::Client::new(),
        })
    }

    pub fn from_env() -> Self {
        Self::new(
            std::env::var("GITHUB_TOKEN").expect("GITHUB_TOKEN not set"),
            "https://api.github.com",
        )
        .expect("from_env base_url은 항상 유효한 https URL")
    }

    pub async fn get_pr_diff(&self, owner: &str, repo: &str, pr_number: u64) -> Result<String> {
        let url = format!("{}/repos/{owner}/{repo}/pulls/{pr_number}", self.base_url);

        let resp = self
            .client
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

        resp.text()
            .await
            .map_err(|e| ReviewerError::GithubApi(e.to_string()))
    }

    /// PR의 head commit SHA를 조회한다.
    /// HTTP 오류 응답은 JSON 파싱 전에 명시적으로 처리한다.
    pub async fn get_pr_head_sha(&self, owner: &str, repo: &str, pr_number: u64) -> Result<String> {
        use serde::Deserialize;
        #[derive(Deserialize)]
        struct PrInfo {
            head: HeadInfo,
        }
        #[derive(Deserialize)]
        struct HeadInfo {
            sha: String,
        }

        let url = format!("{}/repos/{owner}/{repo}/pulls/{pr_number}", self.base_url);
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "ai-code-reviewer/0.1")
            .send()
            .await
            .map_err(|e| ReviewerError::GithubApi(e.to_string()))?;

        // HTTP 오류 응답은 JSON 파싱 전에 명시적으로 처리
        if !resp.status().is_success() {
            let status = resp.status();
            return Err(ReviewerError::GithubApi(format!(
                "PR head SHA 조회 실패 HTTP {status}"
            )));
        }

        let info: PrInfo = resp
            .json()
            .await
            .map_err(|e| ReviewerError::GithubApi(e.to_string()))?;
        Ok(info.head.sha)
    }

    /// 저장소의 .reviewbot.yml 원문을 조회한다.
    /// 파일이 없으면 빈 문자열을 반환한다 (기본값 적용은 호출부 책임).
    /// SSRF 검증은 생성자에서 이미 완료됐으므로 별도 검사 불필요.
    pub async fn get_repo_config(&self, owner: &str, repo: &str) -> Result<String> {
        let url = format!(
            "{}/repos/{owner}/{repo}/contents/.reviewbot.yml",
            self.base_url
        );

        let resp = self
            .client
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

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(ReviewerError::GithubApi(format!(
                "GET .reviewbot.yml {status}"
            )));
        }

        resp.text()
            .await
            .map_err(|e| ReviewerError::GithubApi(e.to_string()))
    }

    pub async fn get_file_content(&self, owner: &str, repo: &str, path: &str) -> Result<String> {
        // 경로 트래버설 차단: "../" 또는 절대경로 포함 금지
        if path.contains("..") || path.starts_with('/') {
            return Err(ReviewerError::GithubApi("path traversal rejected".into()));
        }

        let url = format!("{}/repos/{owner}/{repo}/contents/{path}", self.base_url);

        let resp = self
            .client
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

        resp.text()
            .await
            .map_err(|e| ReviewerError::GithubApi(e.to_string()))
    }
}
