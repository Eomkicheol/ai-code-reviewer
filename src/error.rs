use thiserror::Error;

#[derive(Error, Debug)]
pub enum ReviewerError {
    #[error("webhook signature invalid")]
    InvalidSignature,
    #[error("github api error: {0}")]
    GithubApi(String),
    #[error("llm error: {0}")]
    Llm(String),
    #[error("config error: {0}")]
    Config(String),
    #[error("diff parse error: {0}")]
    DiffParse(String),
}

pub type Result<T> = std::result::Result<T, ReviewerError>;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_invalid_signature_display() {
        let err = ReviewerError::InvalidSignature;
        assert_eq!(err.to_string(), "webhook signature invalid");
    }
    #[test]
    fn test_github_api_error_display() {
        let err = ReviewerError::GithubApi("rate limit exceeded".to_string());
        assert_eq!(err.to_string(), "github api error: rate limit exceeded");
    }
    #[test]
    fn test_result_type_alias() {
        let ok: Result<i32> = Ok(42);
        assert_eq!(ok.unwrap(), 42);
    }
}
