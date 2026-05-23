#[derive(Debug, Clone, PartialEq)]
pub struct RepoInfo {
    pub owner: String,
    pub name: String,
    pub pr_number: u64,
    pub commit_sha: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Language {
    Rust,
    TypeScript,
    Python,
    Go,
    Unknown(String),
}

impl Language {
    pub fn from_extension(ext: &str) -> Self {
        match ext {
            "rs" => Language::Rust,
            "ts" | "tsx" => Language::TypeScript,
            "py" => Language::Python,
            "go" => Language::Go,
            other => Language::Unknown(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DiffLineKind {
    Added,
    Removed,
    Context,
}

#[derive(Debug, Clone)]
pub struct DiffLine {
    pub number: u32,
    pub kind: DiffLineKind,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct DiffHunk {
    pub start_line: u32,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Severity {
    Critical,
    Warning,
    Info,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Category {
    Security,
    Bug,
    Quality,
}

#[derive(Debug, Clone)]
pub struct ReviewComment {
    pub path: String,
    pub line: u32,
    pub severity: Severity,
    pub category: Category,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct ReviewContext {
    pub repo: RepoInfo,
    pub file_path: String,
    pub language: Language,
    pub diff_hunks: Vec<DiffHunk>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_language_from_rs_extension() {
        assert_eq!(Language::from_extension("rs"), Language::Rust);
    }
    #[test]
    fn test_language_from_ts_extension() {
        assert_eq!(Language::from_extension("ts"), Language::TypeScript);
    }
    #[test]
    fn test_language_unknown() {
        assert_eq!(
            Language::from_extension("xyz"),
            Language::Unknown("xyz".to_string())
        );
    }
    #[test]
    fn test_severity_ordering() {
        assert_ne!(Severity::Critical, Severity::Warning);
        assert_ne!(Severity::Warning, Severity::Info);
    }
}
