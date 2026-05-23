use crate::{
    config::ReviewConfig,
    error::Result,
    github::GithubClient,
    review::context::{Language, RepoInfo, ReviewContext},
};

/// PR diff를 가져와서 파일별 ReviewContext 목록으로 변환한다.
pub async fn fetch_review_contexts(
    client: &GithubClient,
    repo: &RepoInfo,
    config: &ReviewConfig,
) -> Result<Vec<ReviewContext>> {
    let raw_diff = client
        .get_pr_diff(&repo.owner, &repo.name, repo.pr_number)
        .await?;

    let contexts = parse_diff_to_contexts(&raw_diff, repo, config);
    Ok(contexts)
}

fn parse_diff_to_contexts(
    raw_diff: &str,
    repo: &RepoInfo,
    config: &ReviewConfig,
) -> Vec<ReviewContext> {
    use crate::diff::parse_diff;

    let mut contexts = Vec::new();
    let mut current_file: Option<String> = None;
    let mut current_diff = String::new();

    for line in raw_diff.lines() {
        if line.starts_with("diff --git ") {
            // 이전 파일 처리
            if let Some(ref file_path) = current_file {
                if !should_ignore(file_path, config) {
                    if let Ok(hunks) = parse_diff(&current_diff) {
                        if !hunks.is_empty() {
                            let lang = detect_language(file_path);
                            contexts.push(ReviewContext {
                                repo: repo.clone(),
                                file_path: file_path.clone(),
                                language: lang,
                                diff_hunks: hunks,
                            });
                        }
                    }
                }
            }
            // 새 파일 시작
            current_file = extract_file_path(line);
            current_diff = String::new();
        } else {
            current_diff.push_str(line);
            current_diff.push('\n');
        }
    }

    // 마지막 파일 처리
    if let Some(ref file_path) = current_file {
        if !should_ignore(file_path, config) {
            if let Ok(hunks) = parse_diff(&current_diff) {
                if !hunks.is_empty() {
                    let lang = detect_language(file_path);
                    contexts.push(ReviewContext {
                        repo: repo.clone(),
                        file_path: file_path.clone(),
                        language: lang,
                        diff_hunks: hunks,
                    });
                }
            }
        }
    }

    contexts
}

/// "diff --git a/src/main.rs b/src/main.rs" → "src/main.rs"
fn extract_file_path(diff_line: &str) -> Option<String> {
    let parts: Vec<&str> = diff_line.split_whitespace().collect();
    parts.get(3).map(|s| s.trim_start_matches("b/").to_string())
}

fn detect_language(path: &str) -> Language {
    let ext = path.rsplit('.').next().unwrap_or("");
    Language::from_extension(ext)
}

fn should_ignore(path: &str, config: &ReviewConfig) -> bool {
    config.ignore.paths.iter().any(|pattern| path.contains(pattern.as_str()))
}
