use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use crate::{error::{Result, ReviewerError}, llm::LlmProvider};

pub struct ClaudeProvider {
    api_key: String,
    model: String,
    base_url: String,
    client: reqwest::Client,
}

impl ClaudeProvider {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>, base_url: &str) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url: base_url.to_string(),
            client: reqwest::Client::new(),
        }
    }

    pub fn from_env(model: impl Into<String>) -> Self {
        Self::new(
            std::env::var("CLAUDE_API_KEY").expect("CLAUDE_API_KEY not set"),
            model,
            "https://api.anthropic.com",
        )
    }
}

#[derive(Serialize)]
struct ClaudeRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    messages: Vec<ClaudeMessage<'a>>,
}

#[derive(Serialize)]
struct ClaudeMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ClaudeResponse {
    content: Vec<ClaudeContent>,
}

#[derive(Deserialize)]
struct ClaudeContent {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

#[async_trait]
impl LlmProvider for ClaudeProvider {
    async fn complete(&self, prompt: &str) -> Result<String> {
        let url = format!("{}/v1/messages", self.base_url);
        let body = ClaudeRequest {
            model: &self.model,
            max_tokens: 2048,
            messages: vec![ClaudeMessage { role: "user", content: prompt }],
        };

        let resp = self.client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ReviewerError::Llm(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ReviewerError::Llm(format!("Claude API {status}: {text}")));
        }

        let data: ClaudeResponse = resp.json().await
            .map_err(|e| ReviewerError::Llm(e.to_string()))?;

        data.content.into_iter()
            .find(|c| c.kind == "text")
            .and_then(|c| c.text)
            .ok_or_else(|| ReviewerError::Llm("no text content in response".into()))
    }

    fn model_name(&self) -> &str { &self.model }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{matchers::{method, path, header}, Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_claude_complete_sends_correct_request() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("x-api-key", "test-key"))
            .and(header("anthropic-version", "2023-06-01"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "content": [{"type": "text", "text": "review result"}]
                }))
            )
            .mount(&mock_server)
            .await;

        let provider = ClaudeProvider::new("test-key", "claude-sonnet-4-6", &mock_server.uri());
        let result = provider.complete("test prompt").await.unwrap();
        assert_eq!(result, "review result");
    }
}
