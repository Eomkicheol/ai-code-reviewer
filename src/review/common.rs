use serde::Deserialize;

use crate::{
    error::{Result, ReviewerError},
    llm::LlmProvider,
    review::context::{Category, DiffLineKind, ReviewComment, ReviewContext, Severity},
};

pub(crate) async fn llm_review<P: LlmProvider>(
    llm: &P,
    prompt: String,
    file_path: &str,
) -> Result<Vec<ReviewComment>> {
    let raw = llm.complete(&prompt).await?;
    parse_llm_response(&raw, file_path)
}

pub(crate) fn format_context_for_prompt(ctx: &ReviewContext) -> String {
    let diff_code = extract_code(ctx);

    if ctx.dep_snippets.is_empty() {
        return diff_code;
    }

    let deps_text = ctx
        .dep_snippets
        .iter()
        .map(|dep| format!("// File: {}\n{}", dep.path, dep.content))
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");

    format!("{diff_code}\n\n[Related files for context]\n{deps_text}")
}

#[derive(Deserialize)]
pub(crate) struct LlmIssue {
    pub line: u32,
    pub severity: String,
    pub category: String,
    pub body: String,
}

pub(crate) fn extract_code(ctx: &ReviewContext) -> String {
    // Removed 라인은 새 파일에 존재하지 않으므로 제외한다.
    // LLM이 정확한 파일 내 줄 번호를 참조할 수 있도록 번호 prefix를 붙인다.
    ctx.diff_hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .filter(|l| l.kind != DiffLineKind::Removed)
        .map(|l| format!("{}: {}", l.number, l.content))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn parse_llm_response(raw: &str, file_path: &str) -> Result<Vec<ReviewComment>> {
    // 비정상적으로 큰 응답 차단 (512KB 초과 → 오류)
    const MAX_RESPONSE_BYTES: usize = 512 * 1024;
    if raw.len() > MAX_RESPONSE_BYTES {
        return Err(ReviewerError::Llm(format!(
            "LLM response too large ({} bytes)",
            raw.len()
        )));
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::context::{
        DepSnippet, DiffHunk, DiffLine, DiffLineKind, Language, RepoInfo, ReviewContext,
    };

    fn make_ctx_with_deps() -> ReviewContext {
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
            dep_snippets: vec![DepSnippet {
                path: "src/auth.rs".into(),
                content: "pub fn verify() {}".into(),
            }],
        }
    }

    #[test]
    fn test_format_context_includes_deps() {
        let ctx = make_ctx_with_deps();
        let result = format_context_for_prompt(&ctx);
        assert!(result.contains("let x = 1;"));
        assert!(result.contains("src/auth.rs"));
        assert!(result.contains("pub fn verify()"));
    }

    #[test]
    fn test_format_context_no_deps_same_as_extract_code() {
        let mut ctx = make_ctx_with_deps();
        ctx.dep_snippets.clear();
        let result = format_context_for_prompt(&ctx);
        assert_eq!(result, extract_code(&ctx));
    }

    #[test]
    fn test_extract_code_includes_line_number_prefix() {
        let ctx = make_ctx_with_deps();
        let result = extract_code(&ctx);
        // "1: let x = 1;" 형식이어야 한다
        assert!(
            result.starts_with("1:"),
            "line number prefix missing: {result}"
        );
        assert!(result.contains("let x = 1;"));
    }

    #[test]
    fn test_extract_code_excludes_removed_lines() {
        let ctx = ReviewContext {
            repo: RepoInfo {
                owner: "test".into(),
                name: "repo".into(),
                pr_number: 1,
                commit_sha: "abc".into(),
            },
            file_path: "src/main.rs".into(),
            language: Language::Rust,
            diff_hunks: vec![DiffHunk {
                start_line: 10,
                lines: vec![
                    DiffLine {
                        number: 10,
                        kind: DiffLineKind::Added,
                        content: "added".into(),
                    },
                    DiffLine {
                        number: 10,
                        kind: DiffLineKind::Removed,
                        content: "removed".into(),
                    },
                    DiffLine {
                        number: 11,
                        kind: DiffLineKind::Context,
                        content: "context".into(),
                    },
                ],
            }],
            dep_snippets: vec![],
        };
        let result = extract_code(&ctx);
        assert!(result.contains("added"), "added line must be present");
        assert!(!result.contains("removed"), "removed line must be absent");
        assert!(result.contains("context"), "context line must be present");
    }
}
