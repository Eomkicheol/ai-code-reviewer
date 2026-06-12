pub mod common;
pub mod context;
pub mod quality;
pub mod security;
pub mod summary;

pub use context::{
    Category, DiffHunk, DiffLine, DiffLineKind, Language, RepoInfo, ReviewComment, ReviewContext,
    Severity,
};
pub use quality::QualityReviewer;
pub use security::SecurityReviewer;

use async_trait::async_trait;

/// 코드 리뷰어 추상화 trait — 의존 방향: review::mod (상위) → security/quality (하위)
#[async_trait]
pub trait Reviewer: Send + Sync {
    fn name(&self) -> &str;
    async fn review(
        &self,
        ctx: &context::ReviewContext,
    ) -> crate::error::Result<Vec<context::ReviewComment>>;
}

use crate::error::Result;

pub struct ReviewEngine {
    security: Box<dyn Reviewer>,
    quality: Box<dyn Reviewer>,
}

impl ReviewEngine {
    pub fn new(security: Box<dyn Reviewer>, quality: Box<dyn Reviewer>) -> Self {
        Self { security, quality }
    }

    pub async fn run(&self, ctx: &ReviewContext) -> Result<Vec<ReviewComment>> {
        let mut all_comments = Vec::new();

        match self.security.review(ctx).await {
            Ok(comments) => all_comments.extend(comments),
            Err(e) => tracing::warn!("security reviewer failed: {e}"),
        }

        match self.quality.review(ctx).await {
            Ok(comments) => all_comments.extend(comments),
            Err(e) => tracing::warn!("quality reviewer failed: {e}"),
        }

        Ok(all_comments)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        llm::MockLlmProvider,
        review::context::{DiffHunk, DiffLine, DiffLineKind, Language, RepoInfo, ReviewContext},
    };

    fn make_ctx() -> ReviewContext {
        ReviewContext {
            repo: RepoInfo {
                owner: "test".into(),
                name: "repo".into(),
                pr_number: 1,
                commit_sha: "abc".into(),
            },
            file_path: "src/main.rs".into(),
            language: Language::Rust,
            diff_hunks: vec![DiffHunk {
                start_line: 1,
                lines: vec![DiffLine {
                    number: 1,
                    kind: DiffLineKind::Added,
                    content: "let x = 1;".into(),
                }],
            }],
            dep_snippets: vec![],
        }
    }

    #[tokio::test]
    async fn test_engine_combines_security_and_quality() {
        let engine = ReviewEngine::new(
            Box::new(SecurityReviewer::new(MockLlmProvider::new(
                r#"[{"line":1,"severity":"critical","category":"security","body":"issue"}]"#,
            ))),
            Box::new(QualityReviewer::new(MockLlmProvider::new(
                r#"[{"line":1,"severity":"warning","category":"quality","body":"naming issue"}]"#,
            ))),
        );
        let comments = engine.run(&make_ctx()).await.unwrap();
        assert_eq!(comments.len(), 2);
    }

    #[tokio::test]
    async fn test_engine_continues_on_partial_failure() {
        let engine = ReviewEngine::new(
            Box::new(SecurityReviewer::new(MockLlmProvider::new(
                r#"[{"line":1,"severity":"critical","category":"security","body":"issue"}]"#,
            ))),
            Box::new(QualityReviewer::new(MockLlmProvider::new(
                "not valid json {{{{",
            ))),
        );
        let comments = engine.run(&make_ctx()).await.unwrap();
        assert_eq!(comments.len(), 1);
    }
}
