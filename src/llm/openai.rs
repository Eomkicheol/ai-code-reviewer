use crate::{
    error::{Result, ReviewerError},
    llm::LlmProvider,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub struct OpenAiProvider {
    api_key: String,
    model: String,
    base_url: String,
    client: reqwest::Client,
}

impl OpenAiProvider {
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
            std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set"),
            model,
            &std::env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com".to_string()),
        )
    }
}

#[derive(Serialize)]
struct OpenAiRequest<'a> {
    model: &'a str,
    messages: Vec<OpenAiMessage<'a>>,
    max_tokens: u32,
}

#[derive(Serialize)]
struct OpenAiMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiResponseMessage,
}

#[derive(Deserialize)]
struct OpenAiResponseMessage {
    content: String,
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn complete(&self, prompt: &str) -> Result<String> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let body = OpenAiRequest {
            model: &self.model,
            max_tokens: 2048,
            messages: vec![OpenAiMessage {
                role: "user",
                content: prompt,
            }],
        };

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ReviewerError::Llm(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = crate::llm::truncate_api_error(resp.text().await.unwrap_or_default());
            return Err(ReviewerError::Llm(format!("OpenAI API {status}: {text}")));
        }

        let data: OpenAiResponse = resp
            .json()
            .await
            .map_err(|e| ReviewerError::Llm(e.to_string()))?;

        data.choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| ReviewerError::Llm("no choices in response".into()))
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}
