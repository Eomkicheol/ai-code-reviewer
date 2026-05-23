use crate::{
    config::ReviewConfig,
    diff::deps::extract_dep_candidates,
    error::Result,
    github::GithubClient,
    review::context::{DepSnippet, Language, RepoInfo, ReviewContext},
};

pub fn parse_into_contexts(
    raw_diff: &str,
    repo: &RepoInfo,
    config: &ReviewConfig,
) -> crate::error::Result<Vec<ReviewContext>> {
    Ok(parse_diff_to_contexts(raw_diff, repo, config))
}

pub async fn fetch_review_contexts(
    client: &GithubClient,
    repo: &RepoInfo,
    config: &ReviewConfig,
) -> Result<Vec<ReviewContext>> {
    let raw_diff = client
        .get_pr_diff(&repo.owner, &repo.name, repo.pr_number)
        .await?;

    let base_contexts = parse_diff_to_contexts(&raw_diff, repo, config);

    let mut enriched = Vec::new();
    for mut ctx in base_contexts {
        ctx.dep_snippets = fetch_dep_snippets(client, repo, &ctx).await;
        enriched.push(ctx);
    }

    Ok(enriched)
}

async fn fetch_dep_snippets(
    client: &GithubClient,
    repo: &RepoInfo,
    ctx: &ReviewContext,
) -> Vec<DepSnippet> {
    let full_content = match client
        .get_file_content(&repo.owner, &repo.name, &ctx.file_path)
        .await
    {
        Ok(c) if !c.is_empty() => c,
        _ => return vec![],
    };

    let candidates = extract_dep_candidates(&full_content, &ctx.language, &ctx.file_path);
    let mut snippets = Vec::new();

    for candidate_path in candidates {
        if snippets.len() >= 5 {
            break; // 최대 5개
        }

        match client
            .get_file_content(&repo.owner, &repo.name, &candidate_path)
            .await
        {
            Ok(content) if !content.is_empty() => {
                // 최대 80줄만 포함 (프롬프트 크기 제한)
                let truncated = content.lines().take(80).collect::<Vec<_>>().join("\n");

                snippets.push(DepSnippet {
                    path: candidate_path,
                    content: truncated,
                });
            }
            _ => {} // 404 또는 에러 → 스킵
        }
    }

    snippets
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
                if !should_ignore(file_path, &current_diff, config) {
                    if let Ok(hunks) = parse_diff(&current_diff) {
                        if !hunks.is_empty() {
                            let lang = detect_language(file_path);
                            contexts.push(ReviewContext {
                                repo: repo.clone(),
                                file_path: file_path.clone(),
                                language: lang,
                                diff_hunks: hunks,
                                dep_snippets: vec![],
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

    if let Some(ref file_path) = current_file {
        if !should_ignore(file_path, &current_diff, config) {
            if let Ok(hunks) = parse_diff(&current_diff) {
                if !hunks.is_empty() {
                    let lang = detect_language(file_path);
                    contexts.push(ReviewContext {
                        repo: repo.clone(),
                        file_path: file_path.clone(),
                        language: lang,
                        diff_hunks: hunks,
                        dep_snippets: vec![],
                    });
                }
            }
        }
    }

    contexts
}

fn extract_file_path(diff_line: &str) -> Option<String> {
    let parts: Vec<&str> = diff_line.split_whitespace().collect();
    parts.get(3).map(|s| s.trim_start_matches("b/").to_string())
}

fn detect_language(path: &str) -> Language {
    let ext = path.rsplit('.').next().unwrap_or("");
    Language::from_extension(ext)
}

fn should_ignore(path: &str, diff_content: &str, config: &ReviewConfig) -> bool {
    if config
        .ignore
        .paths
        .iter()
        .any(|pattern| glob_match(pattern, path))
    {
        return true;
    }
    // diff 크기가 max_file_size_kb를 초과하면 건너뜀
    let size_kb = diff_content.len() as u64 / 1024;
    size_kb > config.ignore.max_file_size_kb
}

fn glob_match(pattern: &str, path: &str) -> bool {
    glob_match_inner(pattern.as_bytes(), path.as_bytes())
}

fn glob_match_inner(pat: &[u8], s: &[u8]) -> bool {
    match (pat.first(), s.first()) {
        (None, None) => true,
        (None, _) => false,
        (Some(b'*'), _) => {
            // `**` → 경로 구분자 포함 모든 문자 소비
            if pat.get(1) == Some(&b'*') {
                let rest_pat = if pat.get(2) == Some(&b'/') {
                    &pat[3..]
                } else {
                    &pat[2..]
                };
                // 현재 위치부터 끝까지 모든 위치 시도
                for i in 0..=s.len() {
                    if glob_match_inner(rest_pat, &s[i..]) {
                        return true;
                    }
                }
                false
            } else {
                // `*` → `/` 제외 임의 문자 소비
                let rest_pat = &pat[1..];
                for i in 0..=s.len() {
                    if s[..i].contains(&b'/') {
                        break;
                    }
                    if glob_match_inner(rest_pat, &s[i..]) {
                        return true;
                    }
                }
                false
            }
        }
        (Some(&p), Some(&c)) => p == c && glob_match_inner(&pat[1..], &s[1..]),
        _ => false,
    }
}

#[cfg(test)]
mod fetcher_tests {
    use super::*;

    #[test]
    fn test_glob_star_matches_within_segment() {
        assert!(glob_match("*.test.rs", "foo.test.rs"));
        assert_eq!(glob_match("*.test.rs", "src/foo.test.rs"), false); // * 는 / 불포함
    }

    #[test]
    fn test_glob_double_star_matches_across_dirs() {
        assert!(glob_match("migrations/**", "migrations/v1/up.sql"));
        assert!(glob_match("migrations/**", "migrations/up.sql"));
    }

    #[test]
    fn test_glob_exact_match() {
        assert!(glob_match("vendor", "vendor"));
        assert!(!glob_match("vendor", "src/vendor"));
    }

    #[test]
    fn test_should_ignore_glob_pattern() {
        use crate::config::{IgnoreConfig, ProviderConfig, ReviewConfig, ReviewersConfig};
        let config = ReviewConfig {
            provider: ProviderConfig {
                name: "claude".into(),
                model: "claude-sonnet-4-6".into(),
            },
            reviewers: ReviewersConfig::default(),
            ignore: IgnoreConfig {
                paths: vec!["*.test.rs".into()],
                max_file_size_kb: 500,
            },
        };
        assert!(should_ignore("foo.test.rs", "", &config));
        assert!(!should_ignore("src/main.rs", "", &config));
    }
}
