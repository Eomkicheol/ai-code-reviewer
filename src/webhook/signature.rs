use crate::error::{Result, ReviewerError};
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// body에 대한 HMAC-SHA256 hex 문자열 반환 (테스트용 공개)
pub fn compute_signature(body: &[u8], secret: &str) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

/// GitHub X-Hub-Signature-256 헤더 검증.
/// constant-time 비교로 타이밍 어택 방지.
pub fn verify_signature(body: &[u8], header: &str, secret: &str) -> Result<()> {
    let provided_hex = header
        .strip_prefix("sha256=")
        .ok_or(ReviewerError::InvalidSignature)?;

    let expected = compute_signature(body, secret);

    // constant-time 비교
    let expected_bytes = hex::decode(&expected).map_err(|_| ReviewerError::InvalidSignature)?;
    let provided_bytes = hex::decode(provided_hex).map_err(|_| ReviewerError::InvalidSignature)?;

    if expected_bytes.len() != provided_bytes.len() {
        return Err(ReviewerError::InvalidSignature);
    }

    let valid = expected_bytes
        .iter()
        .zip(provided_bytes.iter())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0;

    if valid {
        Ok(())
    } else {
        Err(ReviewerError::InvalidSignature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const SECRET: &str = "test-secret-key";

    #[test]
    fn test_valid_signature_passes() {
        let body = b"hello world";
        let sig = compute_signature(body, SECRET);
        let header = format!("sha256={sig}");
        assert!(verify_signature(body, &header, SECRET).is_ok());
    }

    #[test]
    fn test_invalid_signature_rejected() {
        let body = b"hello world";
        let header = "sha256=0000000000000000000000000000000000000000000000000000000000000000";
        assert!(verify_signature(body, header, SECRET).is_err());
    }

    #[test]
    fn test_missing_sha256_prefix_rejected() {
        let body = b"hello world";
        let sig = compute_signature(body, SECRET);
        assert!(verify_signature(body, &sig, SECRET).is_err());
    }

    #[test]
    fn test_empty_body_valid_signature() {
        let body = b"";
        let sig = compute_signature(body, SECRET);
        let header = format!("sha256={sig}");
        assert!(verify_signature(body, &header, SECRET).is_ok());
    }
}
