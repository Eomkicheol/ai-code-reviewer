// Task 2에서 구현 예정 — 임시 스텁
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
