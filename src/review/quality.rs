use async_trait::async_trait;

use crate::{
    error::Result,
    llm::LlmProvider,
    review::{
        common::{format_context_for_prompt, llm_review},
        context::{ReviewComment, ReviewContext},
        Reviewer,
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
        let code = format_context_for_prompt(ctx);
        format!(
            r#"You are a code quality reviewer. Analyze ONLY the code inside the <code> tags.
Focus on: naming conventions, function complexity, code duplication, architecture patterns.
Consider how the changed code fits within the related files shown for context.

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

#[async_trait]
impl<P: LlmProvider> Reviewer for QualityReviewer<P> {
    fn name(&self) -> &str {
        "quality"
    }

    async fn review(&self, ctx: &ReviewContext) -> Result<Vec<ReviewComment>> {
        llm_review(&self.llm, self.build_prompt(ctx), &ctx.file_path).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        llm::MockLlmProvider,
        review::context::{
            Category, DiffHunk, DiffLine, DiffLineKind, Language, RepoInfo, ReviewContext,
        },
    };

    fn make_context() -> ReviewContext {
        ReviewContext {
            repo: RepoInfo {
                owner: "test".into(),
                name: "repo".into(),
                pr_number: 1,
                commit_sha: "abc".into(),
            },
            file_path: "src/lib.rs".into(),
            language: Language::Rust,
            diff_hunks: vec![DiffHunk {
                start_line: 5,
                lines: vec![DiffLine {
                    number: 5,
                    kind: DiffLineKind::Added,
                    content: "fn a() { let x = 1; let y = 2; }".into(),
                }],
            }],
            dep_snippets: vec![],
        }
    }

    #[tokio::test]
    async fn test_quality_reviewer_returns_comments() {
        let llm_response = r#"[
            {"line": 5, "severity": "warning", "category": "quality", "body": "함수명이 너무 짧습니다"}
        ]"#;
        let reviewer = QualityReviewer::new(MockLlmProvider::new(llm_response));
        let comments = reviewer.review(&make_context()).await.unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].category, Category::Quality);
    }

    #[tokio::test]
    async fn test_quality_reviewer_no_issues() {
        let reviewer = QualityReviewer::new(MockLlmProvider::new("[]"));
        assert!(reviewer.review(&make_context()).await.unwrap().is_empty());
    }
}
