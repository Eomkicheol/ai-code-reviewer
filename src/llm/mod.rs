pub mod claude;
pub mod gemini;
pub mod openai;

use async_trait::async_trait;
use crate::error::Result;

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, prompt: &str) -> Result<String>;
    fn model_name(&self) -> &str;
}

/// 테스트 전용 Mock 구현
pub struct MockLlmProvider {
    response: String,
}

impl MockLlmProvider {
    pub fn new(response: impl Into<String>) -> Self {
        Self { response: response.into() }
    }
}

#[async_trait]
impl LlmProvider for MockLlmProvider {
    async fn complete(&self, _prompt: &str) -> Result<String> {
        Ok(self.response.clone())
    }

    fn model_name(&self) -> &str {
        "mock-model"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_provider_returns_preset_response() {
        let mock = MockLlmProvider::new("security issue found on line 5");
        let result = mock.complete("review this code").await.unwrap();
        assert_eq!(result, "security issue found on line 5");
    }

    #[tokio::test]
    async fn test_mock_provider_model_name() {
        let mock = MockLlmProvider::new("response");
        assert_eq!(mock.model_name(), "mock-model");
    }
}
