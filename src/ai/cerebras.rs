use crate::ai::types::{ChatRequest, ChatResponse, LlmError, LlmProvider};
use async_trait::async_trait;
use reqwest::StatusCode;
use serde_json::Value;

#[derive(Clone)]
pub struct CerebrasProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl CerebrasProvider {
    pub fn from_env() -> Result<Self, LlmError> {
        let api_key = std::env::var("CEREBRAS_API_KEY")
            .map_err(|_| LlmError::MissingEnv("CEREBRAS_API_KEY"))?;
        let base_url = std::env::var("CEREBRAS_BASE_URL")
            .unwrap_or_else(|_| "https://api.cerebras.ai/v1".to_string());

        Ok(Self {
            client: reqwest::Client::new(),
            api_key,
            base_url,
        })
    }

    pub fn new(api_key: String, base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            base_url,
        }
    }
}

#[async_trait]
impl LlmProvider for CerebrasProvider {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, LlmError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": req.model,
            "temperature": req.temperature,
            "max_completion_tokens": req.max_tokens,
            "messages": [
                {"role": "system", "content": req.system},
                {"role": "user", "content": req.user}
            ],
            "stream": false
        });

        let resp = self
            .client
            .post(url)
            .bearer_auth(&self.api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::Http(e.to_string()))?;

        match resp.status() {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => return Err(LlmError::Unauthorized),
            StatusCode::TOO_MANY_REQUESTS => return Err(LlmError::RateLimited),
            _ => {}
        }

        let status = resp.status();
        let raw = resp
            .text()
            .await
            .map_err(|e| LlmError::Http(e.to_string()))?;

        if !status.is_success() {
            return Err(LlmError::Http(format!("{} {}", status.as_u16(), raw)));
        }

        let v: Value = serde_json::from_str(&raw)
            .map_err(|e| LlmError::InvalidResponse(format!("json parse failed: {e}, raw={raw}")))?;

        // 兼容不同模型的返回结构：
        // - 标准：choices[0].message.content (string 或 content parts 数组，含 text)
        // - 一些模型：choices[0].text
        // - 兼容：choices[0].content (直接内容或数组)
        let choice0 = v
            .get("choices")
            .and_then(|c| c.get(0))
            .ok_or_else(|| LlmError::InvalidResponse(format!("missing choices[0], raw={raw}")))?;

        // 优先 message.content
        let content = choice0
            .get("message")
            .and_then(|m| m.get("content"))
            .or_else(|| choice0.get("content"));

        let text = if let Some(content) = content {
            match content {
                Value::String(s) => s.clone(),
                Value::Array(arr) => {
                    let mut parts = Vec::new();
                    for it in arr {
                        if let Some(t) = it.get("text").and_then(|x| x.as_str()) {
                            parts.push(t.to_string());
                        } else if let Some(t) = it.as_str() {
                            parts.push(t.to_string());
                        }
                    }
                    parts.join("\n")
                }
                _ => {
                    return Err(LlmError::InvalidResponse(format!(
                        "unexpected message.content type, raw={raw}"
                    )))
                }
            }
        } else if let Some(Value::String(s)) = choice0.get("text") {
            s.clone()
        } else {
            return Err(LlmError::InvalidResponse(format!(
                "missing content/text in choices[0], raw={raw}"
            )));
        };

        Ok(ChatResponse {
            text,
            raw: Some(raw),
        })
    }
}
