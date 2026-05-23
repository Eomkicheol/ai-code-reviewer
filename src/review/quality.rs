use async_trait::async_trait;
use serde::Deserialize;
use crate::{
    error::{Result, ReviewerError},
    llm::LlmProvider,
    review::{
        context::{Category, ReviewComment, ReviewContext, Severity},
        security::Reviewer,
    },
};

pub struct QualityReviewer<P: LlmProvider> {
    llm: P,
}

impl<P: LlmProvider> QualityReviewer<P> {
    pub fn new(llm: P) -> Self {
        Self { llm }
    }

    fn build_prompt(&self, ctx: &ReviewContext) -> String {
        let code = ctx
            .diff_hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .map(|l| l.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"You are a code quality reviewer. Analyze ONLY the code inside the <code> tags.
Focus on: naming conventions, function complexity, code duplication, architecture patterns.

File: {path}
Language: {lang:?}

<code>
{code}
</code>

Respond with a JSON array of issues. Each issue: {{"line": <number>, "severity": "critical"|"warning"|"info", "category": "quality", "body": "<explanation>"}}.
If no issues, respond with [].
Do NOT include any text outside the JSON array."#,
            path = ctx.file_path,
            lang = ctx.language,
            code = code,
        )
    }
}

#[derive(Deserialize)]
struct LlmIssue {
    line: u32,
    severity: String,
    category: String,
    body: String,
}

#[async_trait]
impl<P: LlmProvider> Reviewer for QualityReviewer<P> {
    fn name(&self) -> &str { "quality" }

    async fn review(&self, ctx: &ReviewContext) -> Result<Vec<ReviewComment>> {
        let prompt = self.build_prompt(ctx);
        let raw = self.llm.complete(&prompt).await?;

        let issues: Vec<LlmIssue> = serde_json::from_str(raw.trim())
            .map_err(|e| ReviewerError::Llm(format!("failed to parse LLM response: {e}")))?;

        Ok(issues.into_iter().map(|issue| ReviewComment {
            path: ctx.file_path.clone(),
            line: issue.line,
            severity: match issue.severity.as_str() {
                "critical" => Severity::Critical,
                "warning" => Severity::Warning,
                _ => Severity::Info,
            },
            category: match issue.category.as_str() {
                "security" => Category::Security,
                "bug" => Category::Bug,
                _ => Category::Quality,
            },
            body: issue.body,
        }).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        llm::MockLlmProvider,
        review::context::{DiffHunk, DiffLine, DiffLineKind, Language, RepoInfo, ReviewContext},
    };

    fn make_context() -> ReviewContext {
        ReviewContext {
            repo: RepoInfo {
                owner: "test".into(), name: "repo".into(),
                pr_number: 1, commit_sha: "abc".into(),
            },
            file_path: "src/lib.rs".into(),
            language: Language::Rust,
            diff_hunks: vec![DiffHunk {
                start_line: 5,
                lines: vec![DiffLine {
                    number: 5, kind: DiffLineKind::Added,
                    content: "fn a() { let x = 1; let y = 2; }".into(),
                }],
            }],
        }
    }

    #[tokio::test]
    async fn test_quality_reviewer_returns_comments() {
        let llm_response = r#"[
            {"line": 5, "severity": "warning", "category": "quality", "body": "함수명이 너무 짧습니다"}
        ]"#;
        let mock = MockLlmProvider::new(llm_response);
        let reviewer = QualityReviewer::new(mock);
        let comments = reviewer.review(&make_context()).await.unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].category, Category::Quality);
    }

    #[tokio::test]
    async fn test_quality_reviewer_no_issues() {
        let mock = MockLlmProvider::new("[]");
        let reviewer = QualityReviewer::new(mock);
        let comments = reviewer.review(&make_context()).await.unwrap();
        assert!(comments.is_empty());
    }
}
