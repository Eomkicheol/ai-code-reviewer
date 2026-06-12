pub mod claude;
pub mod gemini;
pub mod openai;

use crate::error::Result;
use async_trait::async_trait;

/// API 에러 응답 본문을 최대 200자로 잘라 반환한다.
/// 멀티바이트 문자 경계에서 패닉하지 않도록 chars()로 순회한다.
pub(crate) fn truncate_api_error(text: String) -> String {
    const MAX_CHARS: usize = 200;
    if text.chars().count() <= MAX_CHARS {
        text
    } else {
        let truncated: String = text.chars().take(MAX_CHARS).collect();
        format!("{truncated}…")
    }
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, prompt: &str) -> Result<String>;
    fn model_name(&self) -> &str;
}

/// LlmProvider 위임 구현 — Box<dyn LlmProvider>를 ReviewEngine에 직접 전달할 수 있게 한다.
#[async_trait]
impl LlmProvider for Box<dyn LlmProvider + Send + Sync> {
    async fn complete(&self, prompt: &str) -> Result<String> {
        (**self).complete(prompt).await
    }

    fn model_name(&self) -> &str {
        (**self).model_name()
    }
}

/// provider 이름과 모델명으로 LLM 프로바이더를 생성한다.
/// - "openai" → OpenAiProvider::from_env
/// - "gemini" → GeminiProvider::from_env
/// - 그 외   → ClaudeProvider::from_env (기본값)
pub fn create_provider(name: &str, model: String) -> Box<dyn LlmProvider + Send + Sync> {
    match name {
        "openai" => Box::new(crate::llm::openai::OpenAiProvider::from_env(model)),
        "gemini" => Box::new(crate::llm::gemini::GeminiProvider::from_env(model)),
        _ => Box::new(crate::llm::claude::ClaudeProvider::from_env(model)),
    }
}

pub struct MockLlmProvider {
    response: String,
}

impl MockLlmProvider {
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
        }
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
