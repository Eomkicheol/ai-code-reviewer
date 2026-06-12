use crate::{
    error::{Result, ReviewerError},
    github::client::GithubClient,
    review::context::{ReviewComment, Severity},
};
use serde::Serialize;

pub struct PostedComment {
    pub path: String,
    pub line: u32,
    pub body: String,
    pub commit_sha: String,
}

#[derive(Serialize)]
struct CreateReviewCommentBody<'a> {
    body: &'a str,
    commit_id: &'a str,
    path: &'a str,
    line: u32,
    side: &'a str,
}

impl GithubClient {
    pub async fn post_review_comment(
        &self,
        owner: &str,
        repo: &str,
        pr_number: u64,
        comment: &PostedComment,
    ) -> Result<()> {
        let url = format!(
            "{}/repos/{owner}/{repo}/pulls/{pr_number}/comments",
            self.base_url
        );

        let body = CreateReviewCommentBody {
            body: &comment.body,
            commit_id: &comment.commit_sha,
            path: &comment.path,
            line: comment.line,
            side: "RIGHT",
        };

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "ai-code-reviewer/0.1")
            .json(&body)
            .send()
            .await
            .map_err(|e| ReviewerError::GithubApi(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ReviewerError::GithubApi(format!(
                "POST comment {status}: {text}"
            )));
        }

        Ok(())
    }

    pub async fn post_review_comments_bulk(
        &self,
        owner: &str,
        repo: &str,
        pr_number: u64,
        commit_sha: &str,
        comments: &[ReviewComment],
    ) -> Result<()> {
        for comment in comments {
            let emoji = match comment.severity {
                Severity::Critical => "🚨",
                Severity::Warning => "⚠️",
                Severity::Info => "ℹ️",
            };
            let formatted_body = format!(
                "{emoji} **[{:?}]** {}\n\n*AI Code Reviewer*",
                comment.category, comment.body
            );
            let posted = PostedComment {
                path: comment.path.clone(),
                line: comment.line,
                body: formatted_body,
                commit_sha: commit_sha.to_string(),
            };
            // 개별 댓글 실패 시 경고만 로깅하고 나머지 댓글 계속 게시
            if let Err(e) = self
                .post_review_comment(owner, repo, pr_number, &posted)
                .await
            {
                tracing::warn!("댓글 게시 실패 ({}:{}): {e}", comment.path, comment.line);
            }
        }
        Ok(())
    }
}

impl GithubClient {
    pub async fn create_pr_review(
        &self,
        owner: &str,
        repo: &str,
        pr_number: u64,
        commit_sha: &str,
        body: &str,
    ) -> Result<()> {
        let url = format!(
            "{}/repos/{owner}/{repo}/pulls/{pr_number}/reviews",
            self.base_url
        );

        let payload = serde_json::json!({
            "commit_id": commit_sha,
            "body": body,
            "event": "COMMENT"
        });

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "ai-code-reviewer/0.1")
            .json(&payload)
            .send()
            .await
            .map_err(|e| ReviewerError::GithubApi(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ReviewerError::GithubApi(format!(
                "POST review {status}: {text}"
            )));
        }

        Ok(())
    }

    pub async fn post_issue_comment(
        &self,
        owner: &str,
        repo: &str,
        issue_number: u64,
        body: &str,
    ) -> Result<()> {
        let url = format!(
            "{}/repos/{owner}/{repo}/issues/{issue_number}/comments",
            self.base_url
        );

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "ai-code-reviewer/0.1")
            .json(&serde_json::json!({"body": body}))
            .send()
            .await
            .map_err(|e| ReviewerError::GithubApi(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ReviewerError::GithubApi(format!(
                "POST issue comment {status}: {text}"
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{header, method, path_regex},
        Mock, MockServer, ResponseTemplate,
    };

    #[tokio::test]
    async fn test_post_comment_calls_github_api() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex("/repos/owner/repo/pulls/1/comments"))
            .and(header("Authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({"id": 1})))
            .mount(&mock_server)
            .await;

        let client = GithubClient::new("test-token", &mock_server.uri()).unwrap();
        let comment = PostedComment {
            path: "src/auth.rs".into(),
            line: 5,
            body: "SQL injection risk".into(),
            commit_sha: "abc123".into(),
        };

        let result = client
            .post_review_comment("owner", "repo", 1, &comment)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_post_comment_handles_non_success() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(422))
            .mount(&mock_server)
            .await;

        let client = GithubClient::new("test-token", &mock_server.uri()).unwrap();
        let comment = PostedComment {
            path: "src/auth.rs".into(),
            line: 5,
            body: "issue".into(),
            commit_sha: "abc123".into(),
        };

        let result = client
            .post_review_comment("owner", "repo", 1, &comment)
            .await;
        assert!(result.is_err());
    }
}
