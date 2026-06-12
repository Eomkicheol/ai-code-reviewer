use crate::{
    error::{Result, ReviewerError},
    llm::LlmProvider,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub struct GeminiProvider {
    api_key: String,
    model: String,
    base_url: String,
    client: reqwest::Client,
}

impl GeminiProvider {
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
            std::env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not set"),
            model,
            &std::env::var("GEMINI_BASE_URL")
                .unwrap_or_else(|_| "https://generativelanguage.googleapis.com".to_string()),
        )
    }
}

#[derive(Serialize)]
struct GeminiRequest<'a> {
    contents: Vec<GeminiContent<'a>>,
}

#[derive(Serialize)]
struct GeminiContent<'a> {
    parts: Vec<GeminiPart<'a>>,
}

#[derive(Serialize)]
struct GeminiPart<'a> {
    text: &'a str,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: GeminiResponseContent,
}

#[derive(Deserialize)]
struct GeminiResponseContent {
    parts: Vec<GeminiResponsePart>,
}

#[derive(Deserialize)]
struct GeminiResponsePart {
    text: String,
}

#[async_trait]
impl LlmProvider for GeminiProvider {
    async fn complete(&self, prompt: &str) -> Result<String> {
        // API 키를 URL 쿼리 파라미터가 아닌 헤더로 전달 (로그/프록시 노출 방지)
        let url = format!(
            "{}/v1beta/models/{}:generateContent",
            self.base_url, self.model
        );
        let body = GeminiRequest {
            contents: vec![GeminiContent {
                parts: vec![GeminiPart { text: prompt }],
            }],
        };

        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("x-goog-api-key", &self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| ReviewerError::Llm(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = crate::llm::truncate_api_error(resp.text().await.unwrap_or_default());
            return Err(ReviewerError::Llm(format!("Gemini API {status}: {text}")));
        }

        let data: GeminiResponse = resp
            .json()
            .await
            .map_err(|e| ReviewerError::Llm(e.to_string()))?;

        data.candidates
            .into_iter()
            .next()
            .and_then(|c| c.content.parts.into_iter().next())
            .map(|p| p.text)
            .ok_or_else(|| ReviewerError::Llm("no content in Gemini response".into()))
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}
