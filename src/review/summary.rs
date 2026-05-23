use crate::{
    error::Result,
    llm::LlmProvider,
    review::context::{Category, ReviewComment, Severity},
};

pub async fn generate_pr_summary<P: LlmProvider>(
    llm: &P,
    comments: &[ReviewComment],
    _repo: &str,
    pr_number: u64,
    pattern_hint: &str,
) -> Result<String> {
    if comments.is_empty() {
        return Ok(format!(
            "## AI Code Review — PR #{pr_number}\n\n이슈가 발견되지 않았습니다.\n\n*AI Code Reviewer*"
        ));
    }

    let findings_text = comments
        .iter()
        .map(|c| {
            let emoji = match c.severity {
                Severity::Critical => "🚨",
                Severity::Warning => "⚠️",
                Severity::Info => "ℹ️",
            };
            format!(
                "- {emoji} [{:?}] `{}:{}` — {}",
                c.category, c.path, c.line, c.body
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let critical = comments
        .iter()
        .filter(|c| c.severity == Severity::Critical)
        .count();
    let warning = comments
        .iter()
        .filter(|c| c.severity == Severity::Warning)
        .count();
    let info = comments
        .iter()
        .filter(|c| c.severity == Severity::Info)
        .count();
    let security = comments
        .iter()
        .filter(|c| c.category == Category::Security)
        .count();
    let quality = comments
        .iter()
        .filter(|c| c.category == Category::Quality)
        .count();

    let pattern_section = if pattern_hint.is_empty() {
        String::new()
    } else {
        format!("\nKnown recurring patterns in this repository:\n{pattern_hint}\n")
    };

    let prompt = format!(
        r#"You are a senior code reviewer. Based on the following findings from a PR review, write a concise PR review summary in Korean.

Findings:
{findings_text}
{pattern_section}
Write a summary with:
1. Overall assessment (1-2 sentences)
2. Key issues to fix before merging
3. Positive observations if any

Be direct and actionable. Keep it under 200 words."#
    );

    let llm_summary = llm.complete(&prompt).await?;

    let checklist = comments
        .iter()
        .filter(|c| c.severity != Severity::Info)
        .map(|c| format!("- [ ] `{}:{}` — {}", c.path, c.line, c.body))
        .collect::<Vec<_>>()
        .join("\n");

    Ok(format!(
        "## AI Code Review — PR #{pr_number}\n\n\
        ### 요약\n{llm_summary}\n\n\
        ### 발견된 이슈\n\
        | 심각도 | 카테고리 | 건수 |\n\
        |--------|---------|------|\n\
        | 🚨 Critical | Security+Bug | {critical} |\n\
        | ⚠️ Warning | - | {warning} |\n\
        | ℹ️ Info | - | {info} |\n\n\
        ### 체크리스트\n\
        {checklist}\n\n\
        *AI Code Reviewer — 보안 {security}건 · 품질 {quality}건 검토 완료*"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        llm::MockLlmProvider,
        review::context::{Category, ReviewComment, Severity},
    };

    fn make_comments() -> Vec<ReviewComment> {
        vec![
            ReviewComment {
                path: "src/auth.rs".into(),
                line: 10,
                severity: Severity::Critical,
                category: Category::Security,
                body: "SQL injection 위험".into(),
            },
            ReviewComment {
                path: "src/lib.rs".into(),
                line: 5,
                severity: Severity::Warning,
                category: Category::Quality,
                body: "함수명 불명확".into(),
            },
        ]
    }

    #[tokio::test]
    async fn test_summary_with_findings() {
        let mock = MockLlmProvider::new("머지 전 SQL injection 수정 필요합니다.");
        let result = generate_pr_summary(&mock, &make_comments(), "owner/repo", 42, "")
            .await
            .unwrap();
        assert!(result.contains("PR #42"));
        assert!(result.contains("체크리스트"));
        assert!(result.contains("src/auth.rs"));
    }

    #[tokio::test]
    async fn test_summary_no_findings() {
        let mock = MockLlmProvider::new("");
        let result = generate_pr_summary(&mock, &[], "owner/repo", 1, "")
            .await
            .unwrap();
        assert!(result.contains("이슈가 발견되지 않았습니다"));
    }

    #[tokio::test]
    async fn test_summary_counts_severities() {
        let mock = MockLlmProvider::new("요약 텍스트");
        let result = generate_pr_summary(&mock, &make_comments(), "owner/repo", 10, "")
            .await
            .unwrap();
        assert!(result.contains("| 🚨 Critical | Security+Bug | 1 |"));
        assert!(result.contains("| ⚠️ Warning | - | 1 |"));
    }
}
