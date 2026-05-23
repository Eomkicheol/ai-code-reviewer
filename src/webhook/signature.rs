// Task 5에서 테스트 작성 후 구현 예정 — 임시 스텁
use crate::error::{Result, ReviewerError};

pub fn compute_signature(_body: &[u8], _secret: &str) -> String {
    String::new()
}

pub fn verify_signature(_body: &[u8], _header: &str, _secret: &str) -> Result<()> {
    Err(ReviewerError::InvalidSignature)
}
