use serde::Deserialize;

use crate::{
    error::{Result, ReviewerError},
    review::context::{Category, ReviewComment, ReviewContext, Severity},
};

#[derive(Deserialize)]
pub(crate) struct LlmIssue {
    pub line: u32,
    pub severity: String,
    pub category: String,
    pub body: String,
}

pub(crate) fn extract_code(ctx: &ReviewContext) -> String {
    ctx.diff_hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .map(|l| l.content.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn parse_llm_response(raw: &str, file_path: &str) -> Result<Vec<ReviewComment>> {
    let issues: Vec<LlmIssue> = serde_json::from_str(raw.trim())
        .map_err(|e| ReviewerError::Llm(format!("failed to parse LLM response: {e}")))?;

    Ok(issues
        .into_iter()
        .map(|issue| ReviewComment {
            path: file_path.to_string(),
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
        })
        .collect())
}
