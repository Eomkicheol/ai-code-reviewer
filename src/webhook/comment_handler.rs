use crate::{error::Result, github::GithubClient, llm::LlmProvider};
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct IssueCommentPayload {
    pub action: String,
    pub comment: IssueComment,
    pub issue: Issue,
    pub repository: crate::webhook::handler::Repository,
}

#[derive(Deserialize, Debug)]
pub struct IssueComment {
    pub body: String,
    pub user: CommentUser,
}

#[derive(Deserialize, Debug)]
pub struct CommentUser {
    // user_type만 봇 여부 판별에 사용 (login, id는 불필요)
    #[serde(rename = "type")]
    pub user_type: String,
}

#[derive(Deserialize, Debug)]
pub struct Issue {
    pub number: u64,
    pub pull_request: Option<serde_json::Value>,
}

#[derive(Debug, PartialEq)]
pub enum CommentCommand {
    FullReview,
    TargetedReview(String),
    Question(String),
    Unknown,
}

pub fn parse_command(body: &str, bot_name: &str) -> CommentCommand {
    let trimmed = body.trim();

    if let Some(rest) = trimmed.strip_prefix("/review") {
        let target = rest.trim().to_string();
        if target.is_empty() {
            return CommentCommand::FullReview;
        }
        return CommentCommand::TargetedReview(target);
    }

    let mention = format!("@{bot_name}");
    if let Some(question) = trimmed.strip_prefix(&mention) {
        let q = question.trim().to_string();
        if !q.is_empty() {
            return CommentCommand::Question(q);
        }
    }

    CommentCommand::Unknown
}

pub async fn handle_question<P: LlmProvider>(
    llm: &P,
    question: &str,
    owner: &str,
    repo: &str,
    pr_number: u64,
    github_client: &GithubClient,
) -> Result<String> {
    let raw_diff = github_client.get_pr_diff(owner, repo, pr_number).await?;
    // diff가 너무 크면 앞부분만 사용 (토큰 비용/프롬프트 인젝션 방지)
    // chars()로 순회해 멀티바이트 경계에서 패닉하지 않도록 한다.
    const MAX_DIFF_CHARS: usize = 8_000;
    let diff = if raw_diff.chars().count() > MAX_DIFF_CHARS {
        let truncated: String = raw_diff.chars().take(MAX_DIFF_CHARS).collect();
        format!("{truncated}\n... (diff truncated)")
    } else {
        raw_diff
    };

    let prompt = format!(
        r#"You are an AI code reviewer assistant. A developer asked the following question about PR #{pr_number} in {owner}/{repo}.

Question: {question}

The PR diff (for context):
<diff>
{diff}
</diff>

Answer the question in Korean, directly and helpfully. Focus on the specific question. Keep it concise (under 150 words)."#
    );

    llm.complete(&prompt).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_full_review_command() {
        assert_eq!(
            parse_command("/review", "reviewer"),
            CommentCommand::FullReview
        );
        assert_eq!(
            parse_command("/review ", "reviewer"),
            CommentCommand::FullReview
        );
    }

    #[test]
    fn test_parse_targeted_review_command() {
        assert_eq!(
            parse_command("/review security", "reviewer"),
            CommentCommand::TargetedReview("security".to_string())
        );
    }

    #[test]
    fn test_parse_question_command() {
        assert_eq!(
            parse_command("@reviewer 왜 이 코드가 위험한가요?", "reviewer"),
            CommentCommand::Question("왜 이 코드가 위험한가요?".to_string())
        );
    }

    #[test]
    fn test_parse_unknown_command() {
        assert_eq!(
            parse_command("일반 댓글입니다", "reviewer"),
            CommentCommand::Unknown
        );
        assert_eq!(parse_command("", "reviewer"), CommentCommand::Unknown);
    }

    #[test]
    fn test_bot_mention_without_question_is_unknown() {
        assert_eq!(
            parse_command("@reviewer", "reviewer"),
            CommentCommand::Unknown
        );
    }
}
